//! Local filesystem `GroupFileStore`: project-contained paths, the Section 6.8
//! transaction protocol (intent journal, staged validated successors, backups,
//! atomic renames, commit marker, deterministic recovery), and bounded prior
//! revision history under `.archaeodash/history`.
//!
//! Transactions are crash-safe: recovery at startup either finishes a committed
//! transaction (cleanup) or restores originals from the journal backups. A
//! transaction without a commit marker and without backups.json never published
//! anything, so dropping its directory is a complete rollback.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use archaeodash_data_io::{
    read_group_file, scan_project, sync_dir, validate_group_file, write_group_rows, GroupFileData,
    GroupProfile, ScanCandidate,
};
use archaeodash_storage::{
    BackupRecord, GroupFileStore, StoreError, Transaction, TransactionIntent,
};

/// Reserved project metadata area (Section 6.3 layout).
pub const ARCHAEODASH_DIR: &str = ".archaeodash";
pub const TRANSACTIONS_DIR: &str = ".archaeodash/transactions";
pub const HISTORY_DIR: &str = ".archaeodash/history";
const COMMIT_MARKER: &str = "COMMIT";
const INTENT_FILE: &str = "intent.json";
const BACKUPS_FILE: &str = "backups.json";
const ORIGINALS_DIR: &str = "originals";
const STAGED_PREFIX: &str = "staged-";
/// Bounded prior-revision archive per group path (Section 6.8).
const MAX_HISTORY_PER_GROUP: usize = 3;

#[cfg(test)]
thread_local! { static FAIL_PUBLISH_INDEX: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
thread_local! { static OUTSIDER_BEFORE_PUBLISH: std::cell::RefCell<Option<(usize, PathBuf)>> = const { std::cell::RefCell::new(None) }; }

fn io_err(e: std::io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

/// Local filesystem implementation of the group-file store.
pub struct FsGroupFileStore {
    root: PathBuf,
}

/// RAII guard for an exclusive project-wide filesystem lock. Dropping the
/// guard releases the lock.
pub struct ProjectLockGuard {
    _lock: fs::File,
}

impl FsGroupFileStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(io_err)?;
        let root = fs::canonicalize(root).map_err(io_err)?;
        Ok(Self { root })
    }

    /// Lexically contains the project root: rejects absolute paths, `..`,
    /// and root/prefix components (Section 6.3 path-containment rule).
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, StoreError> {
        let rel_path = Path::new(rel);
        let mut out = self.root.clone();
        for component in rel_path.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                other => {
                    return Err(StoreError::Io(format!(
                        "path {rel:?} escapes the project boundary: {other:?}"
                    )))
                }
            }
        }
        Ok(out)
    }

    fn transactions_root(&self) -> PathBuf {
        self.root.join(TRANSACTIONS_DIR)
    }

    fn history_root(&self) -> PathBuf {
        self.root.join(HISTORY_DIR)
    }

    fn project_lock(&self) -> Result<fs::File, StoreError> {
        let dir = self.root.join(ARCHAEODASH_DIR);
        fs::create_dir_all(&dir).map_err(io_err)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("project.lock"))
            .map_err(io_err)?;
        lock.lock().map_err(io_err)?;
        Ok(lock)
    }

    /// Acquires an exclusive project lock for coordinated filesystem work
    /// performed by another application service. The lock is released when
    /// the returned guard is dropped.
    pub fn lock_exclusive(&self) -> Result<ProjectLockGuard, StoreError> {
        Ok(ProjectLockGuard {
            _lock: self.project_lock()?,
        })
    }

    fn project_read_lock(&self) -> Result<fs::File, StoreError> {
        let dir = self.root.join(ARCHAEODASH_DIR);
        fs::create_dir_all(&dir).map_err(io_err)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("project.lock"))
            .map_err(io_err)?;
        lock.lock_shared().map_err(io_err)?;
        Ok(lock)
    }

    // Used by execute while it already owns the exclusive project lock.
    fn validate_group_unlocked(&self, path: &str) -> Result<GroupProfile, StoreError> {
        Ok(validate_group_file(&self.resolve(path)?)?)
    }

    fn read_group_unlocked(&self, path: &str) -> Result<GroupFileData, StoreError> {
        Ok(read_group_file(&self.resolve(path)?)?)
    }

    /// Reads an ordered set of group paths while holding one shared project
    /// lock. Missing paths are represented as `None`, so callers can plan
    /// create-only outputs from the same filesystem view as their inputs.
    pub fn read_groups_snapshot(
        &self,
        paths: &[String],
    ) -> Result<Vec<Option<GroupFileData>>, StoreError> {
        let _project_lock = self.project_read_lock()?;
        paths
            .iter()
            .map(|path| {
                let absolute = self.resolve(path)?;
                match fs::metadata(&absolute) {
                    Ok(_) => self.read_group_unlocked(path).map(Some),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(io_err(error)),
                }
            })
            .collect()
    }

    /// Discovers and profiles project candidates while holding one shared
    /// project lock. This coordinates with transactions made through this
    /// store; external writers that ignore the lock remain outside it.
    pub fn scan_candidates_snapshot(&self) -> Result<Vec<ScanCandidate>, StoreError> {
        let _project_lock = self.project_read_lock()?;
        scan_project(&self.root).map_err(StoreError::from)
    }

    fn rollback_transaction(
        &self,
        tx_dir: &Path,
        backups: &[BackupRecord],
        created: &[String],
    ) -> Result<(), StoreError> {
        for rel in created {
            let path = self.resolve(rel)?;
            if path.exists() {
                fs::remove_file(path).map_err(io_err)?;
            }
        }
        for record in backups {
            let target = self.resolve(&record.path)?;
            let backup = tx_dir.join(ORIGINALS_DIR).join(&record.backup_file);
            if backup.exists() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(io_err)?;
                }
                #[cfg(unix)]
                fs::rename(backup, target).map_err(io_err)?;
                #[cfg(not(unix))]
                {
                    if target.exists() {
                        fs::remove_file(&target).map_err(io_err)?;
                    }
                    fs::rename(backup, target).map_err(io_err)?;
                }
            }
        }
        sync_dir(&self.root);
        Ok(())
    }

    fn same_file(left: &Path, right: &Path) -> bool {
        same_file::is_same_file(left, right).unwrap_or(false)
    }

    /// Stable per-relative-path key for history/originals naming.
    fn path_key(rel: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(rel.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> Result<(), StoreError> {
        let body = serde_json::to_string_pretty(value)
            .map_err(|e| StoreError::Io(format!("journal serialize: {e}")))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, body.as_bytes()).map_err(io_err)?;
        if let Ok(f) = std::fs::File::open(&tmp) {
            let _ = f.sync_all();
        }
        std::fs::rename(&tmp, path).map_err(io_err)?;
        if let Some(parent) = path.parent() {
            sync_dir(parent);
        }
        Ok(())
    }

    /// Copies an existing file into the transaction's originals area and
    /// archives one bounded prior revision under `.archaeodash/history`.
    fn backup_original(
        &self,
        tx_dir: &Path,
        rel: &str,
        revision_id: &str,
    ) -> Result<Option<BackupRecord>, StoreError> {
        let current = self.resolve(rel)?;
        if !current.exists() {
            return Ok(None);
        }
        let key = Self::path_key(rel);
        let originals = tx_dir.join(ORIGINALS_DIR);
        fs::create_dir_all(&originals).map_err(io_err)?;
        let backup_file = format!("{key}.parquet");
        fs::copy(&current, originals.join(&backup_file)).map_err(io_err)?;

        let history_group = self.history_root().join(&key);
        fs::create_dir_all(&history_group).map_err(io_err)?;
        let entries = match fs::read_dir(&history_group) {
            Ok(entries) => entries,
            Err(e) => return Err(io_err(e)),
        };
        let mut seq: usize = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                e.file_name()
                    .to_string_lossy()
                    .split('-')
                    .next()?
                    .parse::<usize>()
                    .ok()
            })
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        // Prune to the bounded archive before writing the newest revision.
        let mut archived: Vec<PathBuf> = fs::read_dir(&history_group)
            .map_err(io_err)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        archived.sort();
        while archived.len() >= MAX_HISTORY_PER_GROUP {
            let oldest = archived.remove(0);
            fs::remove_file(oldest).map_err(io_err)?;
        }
        let _ = &mut seq;
        let history_name = format!("{seq:06}-{revision_id}.parquet");
        fs::copy(&current, history_group.join(history_name)).map_err(io_err)?;

        Ok(Some(BackupRecord {
            path: rel.to_string(),
            backup_file,
        }))
    }
}

impl GroupFileStore for FsGroupFileStore {
    fn validate_group(&self, path: &str) -> Result<GroupProfile, StoreError> {
        let _project_lock = self.project_read_lock()?;
        self.validate_group_unlocked(path)
    }

    fn read_group(&self, path: &str) -> Result<GroupFileData, StoreError> {
        let _project_lock = self.project_read_lock()?;
        self.read_group_unlocked(path)
    }

    fn execute(&self, tx: &Transaction) -> Result<String, StoreError> {
        let _project_lock = self.project_lock()?;
        let input_paths: std::collections::HashSet<&str> =
            tx.inputs.iter().map(|i| i.path.as_str()).collect();
        let create_only: std::collections::HashSet<&str> = tx
            .outputs
            .iter()
            .map(|o| o.path.as_str())
            .filter(|p| !input_paths.contains(p))
            .collect();
        // 1. Preconditions: every input must still be at its expected
        //    revision with an unchanged measured checksum (Section 6.8
        //    `expected_revision`).
        for input in &tx.inputs {
            let profile = self.validate_group_unlocked(&input.path)?;
            if profile.revision_id != input.revision_id {
                return Err(StoreError::RevisionConflict {
                    path: input.path.clone(),
                    expected: input.revision_id.clone(),
                    found: profile.revision_id,
                });
            }
            if profile.measured_elemental_checksum != input.measured_elemental_checksum {
                return Err(StoreError::RevisionConflict {
                    path: input.path.clone(),
                    expected: input.measured_elemental_checksum.clone(),
                    found: profile.measured_elemental_checksum,
                });
            }
        }
        // New outputs must remain absent through preflight; publishing them
        // later uses a no-clobber hard link to close the race with other writers.
        for path in &create_only {
            if self.resolve(path)?.exists() {
                return Err(StoreError::Invariant(format!(
                    "new destination already exists: {path}"
                )));
            }
        }

        // 2. Journal the intent before touching any data file.
        let txid = Uuid::now_v7().simple().to_string();
        let tx_dir = self.transactions_root().join(&txid);
        fs::create_dir_all(tx_dir.join(ORIGINALS_DIR)).map_err(io_err)?;
        let started_at_unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let intent = TransactionIntent {
            transaction_id: txid.clone(),
            action: tx.action,
            inputs: tx.inputs.clone(),
            outputs: tx
                .outputs
                .iter()
                .map(|out| {
                    // This is the caller's planned create-vs-replace intent.
                    // Do not infer it from disk here: a new file may have
                    // appeared since planning and must remain a no-clobber path.
                    let created = create_only.contains(out.path.as_str());
                    Ok(archaeodash_storage::FileOutput {
                        path: out.path.clone(),
                        group_id: out.group_id.clone(),
                        group_name: out.group_name.clone(),
                        created,
                    })
                })
                .collect::<Result<Vec<_>, StoreError>>()?,
            selected_uuids: tx.selected_uuids.clone(),
            started_at_unix_secs,
        };
        Self::write_json_atomic(&tx_dir.join(INTENT_FILE), &intent)?;

        let mut backups: Vec<BackupRecord> = Vec::new();
        let mut created_by_tx: Vec<String> = Vec::new();
        let abort = |tx_dir: &Path,
                     backups: &[BackupRecord],
                     created: &[String]|
         -> Result<(), StoreError> {
            self.rollback_transaction(tx_dir, backups, created)?;
            fs::remove_dir_all(tx_dir).map_err(io_err)
        };

        // 3. Stage every successor file and validate it in full before any
        //    publish (Section 6.8: "build every successor Parquet file in a
        //    same-filesystem staging area ... validate each staged file").
        let mut staged: Vec<(PathBuf, &archaeodash_storage::PlannedOutput)> = Vec::new();
        for (index, out) in tx.outputs.iter().enumerate() {
            let profile = GroupProfile {
                profile_version: archaeodash_data_io::PROFILE_VERSION,
                file_kind: "group".to_string(),
                group_id: out.group_id.clone(),
                group_name: out.group_name.clone(),
                revision_id: out.revision_id.clone(),
                roles: out.roles.clone(),
                row_count: out.rows.len(),
                source_path: out.source_path.clone(),
                source_sha256: out.source_sha256.clone(),
                import_recipe: out.recipe.clone(),
                measured_elemental_checksum: String::new(),
            };
            let staged_path = tx_dir.join(format!("{STAGED_PREFIX}{index}.parquet"));
            if let Err(e) = write_group_rows(&staged_path, profile, &out.rows)
                .and_then(|_| validate_group_file(&staged_path).map(|_| ()))
            {
                abort(&tx_dir, &backups, &created_by_tx)?;
                return Err(e.into());
            }
            staged.push((staged_path, out));
        }

        // 4. Backup originals of every overwritten or deleted path, then
        //    record the backup mapping so recovery can restore exactly.
        let mut publish_paths: Vec<(PathBuf, String)> = Vec::new();
        for (staged_path, out) in &staged {
            let destination = self.resolve(&out.path)?;
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(io_err)?;
            }
            if create_only.contains(out.path.as_str()) {
                publish_paths.push((
                    staged_path.clone(),
                    destination.to_string_lossy().to_string(),
                ));
                continue;
            }
            let record = match self.backup_original(&tx_dir, &out.path, &out.revision_id) {
                Ok(record) => record,
                Err(e) => {
                    abort(&tx_dir, &backups, &created_by_tx)?;
                    return Err(e);
                }
            };
            if record.is_some() {
                backups.extend(record);
            }
            publish_paths.push((
                staged_path.clone(),
                destination.to_string_lossy().to_string(),
            ));
        }
        for rel in &tx.delete_paths {
            let record = match self.backup_original(&tx_dir, rel, "deleted") {
                Ok(record) => record,
                Err(e) => {
                    abort(&tx_dir, &backups, &created_by_tx)?;
                    return Err(e);
                }
            };
            if record.is_some() {
                backups.extend(record);
            }
        }
        if let Err(e) = Self::write_json_atomic(&tx_dir.join(BACKUPS_FILE), &backups) {
            abort(&tx_dir, &backups, &created_by_tx)?;
            return Err(e);
        }

        // 5. Publish: atomic renames of validated staged files.
        for (index, (staged_path, destination)) in publish_paths.iter().enumerate() {
            #[cfg(test)]
            let injection = OUTSIDER_BEFORE_PUBLISH.with(|hook| {
                if let Some((at, path)) = hook.borrow().as_ref() {
                    if *at == index {
                        return Some(fs::write(path, b"outsider-created-after-preflight"));
                    }
                }
                None
            });
            #[cfg(test)]
            if let Some(Err(e)) = injection {
                abort(&tx_dir, &backups, &created_by_tx)?;
                return Err(io_err(e));
            }
            #[cfg(test)]
            if FAIL_PUBLISH_INDEX.with(|fail| fail.get() == Some(index)) {
                abort(&tx_dir, &backups, &created_by_tx)?;
                return Err(StoreError::Io("injected publish failure".into()));
            }
            let rel = &tx.outputs[index].path;
            let publish_result = if create_only.contains(rel.as_str()) {
                match fs::hard_link(staged_path, destination) {
                    Ok(()) => {
                        created_by_tx.push(rel.clone());
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            } else {
                fs::rename(staged_path, destination)
            };
            if let Err(e) = publish_result {
                abort(&tx_dir, &backups, &created_by_tx)?;
                return Err(StoreError::Io(format!("publish failed: {e}")));
            }
        }
        // 6. Delete vacated source files.
        for rel in &tx.delete_paths {
            let path = self.resolve(rel)?;
            if path.exists() {
                if let Err(e) = fs::remove_file(&path) {
                    abort(&tx_dir, &backups, &created_by_tx)?;
                    return Err(io_err(e));
                }
            }
        }
        for parent in [&self.transactions_root(), &self.root] {
            sync_dir(parent);
        }

        // 7. Commit marker, then cleanup: the transaction is durable.
        if let Err(e) = fs::write(tx_dir.join(COMMIT_MARKER), b"committed") {
            abort(&tx_dir, &backups, &created_by_tx)?;
            return Err(io_err(e));
        }
        sync_dir(&tx_dir);
        // The commit marker makes this transaction durable. A cleanup failure
        // is recovered by startup and must not report an already-published move
        // as failed to its caller.
        let _ = fs::remove_dir_all(&tx_dir);
        Ok(txid)
    }

    fn recover_interrupted(&self) -> Result<usize, StoreError> {
        let _project_lock = self.project_lock()?;
        let transactions_root = self.transactions_root();
        if !transactions_root.exists() {
            return Ok(0);
        }
        let mut recovered = 0usize;
        let entries = fs::read_dir(&transactions_root).map_err(io_err)?;
        for entry in entries {
            let entry = entry.map_err(io_err)?;
            let tx_dir = entry.path();
            if !tx_dir.is_dir() {
                continue;
            }
            recovered += 1;
            if tx_dir.join(COMMIT_MARKER).exists() {
                // Committed: only cleanup remains.
                fs::remove_dir_all(&tx_dir).map_err(io_err)?;
                continue;
            }
            let backups: Vec<BackupRecord> = match fs::read_to_string(tx_dir.join(BACKUPS_FILE)) {
                Ok(text) => serde_json::from_str(&text)
                    .map_err(|e| StoreError::Io(format!("backups journal: {e}")))?,
                Err(_) => Vec::new(),
            };
            let intent: Option<TransactionIntent> =
                match fs::read_to_string(tx_dir.join(INTENT_FILE)) {
                    Ok(text) => Some(
                        serde_json::from_str(&text)
                            .map_err(|e| StoreError::Io(format!("intent journal: {e}")))?,
                    ),
                    Err(_) => None,
                };
            // Restore every backed-up original over its published successor.
            for record in &backups {
                let target = self.resolve(&record.path)?;
                let backup = tx_dir.join(ORIGINALS_DIR).join(&record.backup_file);
                if backup.exists() {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(io_err)?;
                    }
                    if target.exists() {
                        fs::remove_file(&target).map_err(io_err)?;
                    }
                    fs::rename(&backup, &target).map_err(io_err)?;
                }
            }
            // Outputs marked created=true without a backup never existed
            // before the transaction; a crash between publish and commit may
            // have left them behind.
            if let Some(intent) = &intent {
                let backed: std::collections::HashSet<&str> =
                    backups.iter().map(|b| b.path.as_str()).collect();
                for (index, out) in intent.outputs.iter().enumerate() {
                    if out.created && !backed.contains(&out.path.as_str()) {
                        let path = self.resolve(&out.path)?;
                        let staged = tx_dir.join(format!("{STAGED_PREFIX}{index}.parquet"));
                        if Self::same_file(&path, &staged) {
                            fs::remove_file(&path).map_err(io_err)?;
                        } else if path.exists() {
                            return Err(StoreError::Invariant(format!(
                                "cannot safely recover created destination {}; journal preserved",
                                out.path
                            )));
                        }
                    }
                }
            }
            fs::remove_dir_all(&tx_dir).map_err(io_err)?;
        }
        Ok(recovered)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_data_io::{ColumnRoles, GroupRow, ImportRecipe};
    use archaeodash_storage::{assert_measured_values_preserved, FileInput, PlannedOutput};
    use std::collections::HashSet;
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    fn profile(group_id: &str) -> GroupProfile {
        GroupProfile {
            profile_version: archaeodash_data_io::PROFILE_VERSION,
            file_kind: "group".to_string(),
            group_id: group_id.to_string(),
            group_name: group_id.to_string(),
            revision_id: "rev-1".to_string(),
            roles: ColumnRoles {
                identity: "analytical_uuid".to_string(),
                visible_id: "anid".to_string(),
                legacy_rowid: "legacy_rowid".to_string(),
                descriptive: vec!["site".to_string()],
                elemental: vec!["as".to_string(), "fe".to_string()],
            },
            row_count: 0,
            source_path: None,
            source_sha256: None,
            import_recipe: ImportRecipe::default(),
            measured_elemental_checksum: String::new(),
        }
    }

    fn row(uuid: Uuid, as_val: f64, fe_val: Option<f64>) -> GroupRow {
        GroupRow {
            uuid,
            visible: Some(format!("A-{uuid}")),
            legacy_rowid: None,
            descriptive: vec![Some("SiteA".to_string())],
            elemental: vec![Some(as_val), fe_val],
        }
    }

    fn write_initial(path: &Path, group_id: &str, rows: &[GroupRow]) -> GroupProfile {
        write_group_rows(path, profile(group_id), rows).expect("initial group write")
    }

    fn input_of(path: &str, data: &GroupFileData) -> FileInput {
        FileInput {
            path: path.to_string(),
            revision_id: data.profile.revision_id.clone(),
            measured_elemental_checksum: data.profile.measured_elemental_checksum.clone(),
        }
    }

    fn planned(
        path: &str,
        group_id: &str,
        revision: &str,
        rows: Vec<GroupRow>,
        roles: &ColumnRoles,
    ) -> PlannedOutput {
        PlannedOutput {
            path: path.to_string(),
            group_id: group_id.to_string(),
            group_name: group_id.to_string(),
            revision_id: revision.to_string(),
            rows,
            roles: roles.clone(),
            recipe: ImportRecipe::default(),
            source_path: None,
            source_sha256: None,
        }
    }

    fn uuids(data: &GroupFileData) -> HashSet<Uuid> {
        data.rows.iter().map(|r| r.uuid).collect()
    }

    #[test]
    fn resolve_rejects_path_escape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FsGroupFileStore::new(dir.path()).expect("store");
        assert!(store.resolve("groups/g1.parquet").is_ok());
        assert!(store.resolve("../outside.parquet").is_err());
        assert!(store.resolve("/etc/passwd").is_err());
        assert!(store.resolve("groups/../../escape.parquet").is_err());
    }

    #[test]
    fn group_reads_wait_for_writers_and_share_read_locks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let first = FsGroupFileStore::new(root).expect("first store");
        let second = FsGroupFileStore::new(root).expect("second store");
        let rel = "groups/read-lock.parquet";
        write_initial(
            &root.join(rel),
            "read-lock",
            &[row(Uuid::now_v7(), 1.0, None)],
        );

        let writer_guard = first.project_lock().expect("exclusive lock");
        let (started_tx, started_rx) = mpsc::channel();
        let (read_tx, read_rx) = mpsc::channel();
        let reader =
            std::thread::spawn(move || {
                started_tx.send(()).expect("signal started");
                read_tx
                    .send(second.read_groups_snapshot(&[
                        rel.to_string(),
                        "groups/missing.parquet".to_string(),
                    ]))
                    .expect("send read result");
            });
        started_rx.recv().expect("reader started");
        assert!(read_rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(writer_guard);
        let data = read_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("read completes after writer releases")
            .expect("groups remain readable");
        assert_eq!(
            data[0].as_ref().expect("present group").profile.group_id,
            "read-lock"
        );
        assert!(
            data[1].is_none(),
            "missing path is represented in the snapshot"
        );
        reader.join().expect("reader joins");

        let reader_guard = first.project_read_lock().expect("shared lock");
        let (read_tx, read_rx) = mpsc::channel();
        let second_reader = FsGroupFileStore::new(root).expect("second reader store");
        let reader = std::thread::spawn(move || {
            read_tx
                .send(second_reader.validate_group(rel))
                .expect("send validation result");
        });
        assert!(read_rx.recv_timeout(Duration::from_secs(3)).is_ok());
        drop(reader_guard);
        reader.join().expect("reader joins");
    }

    // Invoked by process_read_waits_for_exclusive_project_lock in a separate
    // test process. The marker means the child is about to call the real store
    // read API; it then blocks there until the parent drops its exclusive lock.
    #[test]
    fn process_reader_child() {
        let Some(root) = std::env::var_os("ARCHAEODASH_LOCK_CHILD_ROOT") else {
            return;
        };
        let ready = std::env::var_os("ARCHAEODASH_LOCK_CHILD_READY").expect("ready marker path");
        fs::write(ready, b"ready").expect("write child ready marker");
        FsGroupFileStore::new(root)
            .expect("child store")
            .read_group("groups/process-lock.parquet")
            .expect("read group after exclusive lock releases");
    }

    struct ChildCleanup(Child);

    impl Drop for ChildCleanup {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }

    #[test]
    fn process_read_waits_for_exclusive_project_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_initial(
            &root.join("groups/process-lock.parquet"),
            "process-lock",
            &[row(Uuid::now_v7(), 1.0, None)],
        );
        let store = FsGroupFileStore::new(root).expect("store");
        let writer_guard = store.lock_exclusive().expect("exclusive lock");
        let ready = root.join("child-ready");
        let child = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "tests::process_reader_child", "--nocapture"])
            .env("ARCHAEODASH_LOCK_CHILD_ROOT", root)
            .env("ARCHAEODASH_LOCK_CHILD_READY", &ready)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn child test process");
        let mut child = ChildCleanup(child);

        let start_deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            if let Some(status) = child.0.try_wait().expect("check child status") {
                panic!("child exited before attempting read: {status}");
            }
            assert!(
                Instant::now() < start_deadline,
                "child did not become ready"
            );
            thread::sleep(Duration::from_millis(10));
        }

        // The readiness marker is written immediately before read_group.
        // Give that call time to enter its blocking lock acquisition.
        thread::sleep(Duration::from_millis(200));
        assert!(
            child.0.try_wait().expect("check child status").is_none(),
            "child read completed while exclusive lock was held"
        );
        drop(writer_guard);

        let finish_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.0.try_wait().expect("check child status") {
                assert!(status.success(), "child read failed: {status}");
                break;
            }
            assert!(
                Instant::now() < finish_deadline,
                "child read did not finish after release"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn candidate_scan_waits_for_exclusive_project_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let first = FsGroupFileStore::new(root).expect("first store");
        let second = FsGroupFileStore::new(root).expect("second store");
        let rel = "groups/scan-lock.parquet";
        write_initial(
            &root.join(rel),
            "scan-lock",
            &[row(Uuid::now_v7(), 1.0, None)],
        );

        let writer_guard = first.project_lock().expect("exclusive lock");
        let (started_tx, started_rx) = mpsc::channel();
        let (scan_tx, scan_rx) = mpsc::channel();
        let scanner = std::thread::spawn(move || {
            started_tx.send(()).expect("signal started");
            scan_tx
                .send(second.scan_candidates_snapshot())
                .expect("send scan result");
        });
        started_rx.recv().expect("scanner started");
        assert!(scan_rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(writer_guard);
        let candidates = scan_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("scan completes after writer releases")
            .expect("project scan succeeds");
        assert!(candidates.iter().any(|candidate| {
            candidate.path.ends_with(rel)
                && matches!(
                    candidate.status,
                    archaeodash_data_io::CandidateStatus::ReadyToAdd(_)
                )
        }));
        scanner.join().expect("scanner joins");
    }

    #[test]
    fn move_executes_transactionally_and_preserves_measured_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let src_rel = "groups/g1.parquet";
        let dst_rel = "groups/g2.parquet";
        let src_path = root.join(src_rel);
        let dst_path = root.join(dst_rel);

        let u = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
        write_initial(
            &src_path,
            "g1",
            &[row(u[0], 1.0, None), row(u[1], 2.0, Some(3.0))],
        );
        write_initial(&dst_path, "g2", &[row(u[2], 9.0, Some(8.0))]);
        let src_data = read_group_file(&src_path).expect("read src");
        let dst_data = read_group_file(&dst_path).expect("read dst");

        let (source_out, dest_out) = archaeodash_storage::plan_move(
            src_rel,
            &src_data,
            Some((dst_rel, Some(&dst_data))),
            &[u[0]],
            "rev-2",
            "g2",
            "g2",
            "rev-2",
        )
        .expect("plan");

        let tx = Transaction {
            action: archaeodash_storage::TransactionAction::MoveUnits,
            inputs: vec![input_of(src_rel, &src_data), input_of(dst_rel, &dst_data)],
            outputs: vec![
                source_out.expect("source keeps rows"),
                dest_out.expect("dest"),
            ],
            delete_paths: vec![],
            selected_uuids: vec![u[0]],
        };
        store.execute(&tx).expect("execute");

        let new_src = read_group_file(&src_path).expect("source successor");
        let new_dst = read_group_file(&dst_path).expect("destination successor");
        assert_eq!(new_src.profile.revision_id, "rev-2");
        assert_eq!(new_dst.profile.revision_id, "rev-2");
        // No lost or duplicated identities.
        let all: HashSet<Uuid> = uuids(&new_src).union(&uuids(&new_dst)).copied().collect();
        let expected: HashSet<Uuid> = u.into_iter().collect();
        assert_eq!(all, expected);
        // Measured values byte-stable for the moved unit.
        assert_measured_values_preserved(&[u[0]], &src_data, &new_dst.rows)
            .expect("measured values preserved");
        // Both successors validate in full (checksum included).
        store.validate_group(src_rel).expect("source validates");
        store.validate_group(dst_rel).expect("dest validates");
        // No derived columns leaked into successors.
        assert_no_derived_columns_ok(&new_src.profile);
        assert_no_derived_columns_ok(&new_dst.profile);
        // Transaction journal cleaned up after commit.
        assert!(!root.join(TRANSACTIONS_DIR).join("x").exists());
        assert!(root.join(TRANSACTIONS_DIR).read_dir().expect("txs").count() == 0);
    }

    fn assert_no_derived_columns_ok(profile: &GroupProfile) {
        archaeodash_data_io::assert_no_derived_columns(profile).expect("no derived columns");
    }

    #[test]
    fn revision_conflict_aborts_without_touching_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let src_rel = "groups/g1.parquet";
        let src_path = root.join(src_rel);
        let u = Uuid::now_v7();
        let src_profile = write_initial(&src_path, "g1", &[row(u, 1.0, None)]);
        let before = std::fs::read(&src_path).expect("read before");

        let data = read_group_file(&src_path).expect("read");
        let mut stale = input_of(src_rel, &data);
        stale.revision_id = "rev-stale".to_string();
        let tx = Transaction {
            action: archaeodash_storage::TransactionAction::MoveUnits,
            inputs: vec![stale],
            outputs: vec![planned(
                "groups/g2.parquet",
                "g2",
                "rev-1",
                data.rows.clone(),
                &src_profile.roles,
            )],
            delete_paths: vec![],
            selected_uuids: vec![u],
        };
        let err = store.execute(&tx).expect_err("stale revision rejected");
        assert!(matches!(err, StoreError::RevisionConflict { .. }));
        assert_eq!(std::fs::read(&src_path).expect("read after"), before);
        // No transaction directory survives the abort.
        assert_eq!(
            root.join(TRANSACTIONS_DIR)
                .read_dir()
                .map(|d| d.count())
                .unwrap_or(0),
            0
        );
    }

    #[test]
    fn checksum_mismatch_aborts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = FsGroupFileStore::new(dir.path()).expect("store");
        let src_rel = "groups/g1.parquet";
        let u = Uuid::now_v7();
        write_initial(&dir.path().join(src_rel), "g1", &[row(u, 1.0, None)]);
        let data = read_group_file(&dir.path().join(src_rel)).expect("read");
        let mut bad = input_of(src_rel, &data);
        bad.measured_elemental_checksum = "deadbeef".to_string();
        let tx = Transaction {
            action: archaeodash_storage::TransactionAction::MoveUnits,
            inputs: vec![bad],
            outputs: vec![planned(
                "groups/g2.parquet",
                "g2",
                "rev-1",
                data.rows.clone(),
                &data.profile.roles,
            )],
            delete_paths: vec![],
            selected_uuids: vec![u],
        };
        let err = store.execute(&tx).expect_err("checksum mismatch rejected");
        assert!(matches!(err, StoreError::RevisionConflict { .. }));
    }

    #[test]
    fn publish_failure_restores_prior_outputs_and_preserves_outsider_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let source_rel = "groups/source.parquet";
        let existing_rel = "groups/existing.parquet";
        let new_rel = "groups/new.parquet";
        let u1 = Uuid::now_v7();
        let u2 = Uuid::now_v7();
        write_initial(&root.join(source_rel), "source", &[row(u1, 1.0, None)]);
        write_initial(&root.join(existing_rel), "existing", &[row(u2, 2.0, None)]);
        let source = read_group_file(&root.join(source_rel)).expect("source read");
        let existing = read_group_file(&root.join(existing_rel)).expect("existing read");
        let original = fs::read(root.join(existing_rel)).expect("existing bytes");
        let outsider = root.join(new_rel);
        let dest_rows = vec![existing.rows[0].clone(), source.rows[0].clone()];
        let tx = Transaction {
            action: archaeodash_storage::TransactionAction::MoveUnits,
            inputs: vec![
                input_of(source_rel, &source),
                input_of(existing_rel, &existing),
            ],
            outputs: vec![
                planned(
                    existing_rel,
                    "existing",
                    "rev-2",
                    dest_rows.clone(),
                    &existing.profile.roles,
                ),
                planned(new_rel, "new", "rev-1", dest_rows, &source.profile.roles),
            ],
            delete_paths: vec![],
            selected_uuids: vec![u1],
        };
        FAIL_PUBLISH_INDEX.with(|fail| fail.set(Some(1)));
        let result = store.execute(&tx);
        FAIL_PUBLISH_INDEX.with(|fail| fail.set(None));
        assert!(matches!(result, Err(StoreError::Io(_))));
        assert_eq!(
            fs::read(root.join(existing_rel)).expect("restored existing"),
            original
        );
        assert!(
            !outsider.exists(),
            "transaction-created output is removed on rollback"
        );
        assert_eq!(
            root.join(TRANSACTIONS_DIR)
                .read_dir()
                .expect("transactions")
                .count(),
            0
        );
    }

    #[test]
    fn create_only_output_that_appeared_after_planning_is_preserved() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let source_rel = "groups/source.parquet";
        let new_rel = "groups/new.parquet";
        let u = Uuid::now_v7();
        write_initial(&root.join(source_rel), "source", &[row(u, 1.0, None)]);
        let source = read_group_file(&root.join(source_rel)).expect("source");
        let new_path = root.join(new_rel);
        fs::write(&new_path, b"outsider").expect("outside file appears");
        let err = store
            .execute(&Transaction {
                action: archaeodash_storage::TransactionAction::MoveUnits,
                inputs: vec![input_of(source_rel, &source)],
                outputs: vec![planned(
                    new_rel,
                    "new",
                    "rev-1",
                    source.rows.clone(),
                    &source.profile.roles,
                )],
                delete_paths: vec![],
                selected_uuids: vec![u],
            })
            .expect_err("create-only collision rejected");
        assert!(matches!(err, StoreError::Invariant(_)));
        assert_eq!(fs::read(new_path).expect("outsider intact"), b"outsider");
    }

    #[test]
    fn create_only_collision_during_publish_restores_prior_outputs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let source_rel = "groups/source.parquet";
        let existing_rel = "groups/existing.parquet";
        let new_rel = "groups/new.parquet";
        let u1 = Uuid::now_v7();
        let u2 = Uuid::now_v7();
        write_initial(&root.join(source_rel), "source", &[row(u1, 1.0, None)]);
        write_initial(&root.join(existing_rel), "existing", &[row(u2, 2.0, None)]);
        let source = read_group_file(&root.join(source_rel)).expect("source");
        let existing = read_group_file(&root.join(existing_rel)).expect("existing");
        let original = fs::read(root.join(existing_rel)).expect("existing bytes");
        let outsider = root.join(new_rel);
        let tx = Transaction {
            action: archaeodash_storage::TransactionAction::MoveUnits,
            inputs: vec![
                input_of(source_rel, &source),
                input_of(existing_rel, &existing),
            ],
            outputs: vec![
                planned(
                    existing_rel,
                    "existing",
                    "rev-2",
                    vec![existing.rows[0].clone(), source.rows[0].clone()],
                    &existing.profile.roles,
                ),
                planned(
                    new_rel,
                    "new",
                    "rev-1",
                    source.rows.clone(),
                    &source.profile.roles,
                ),
            ],
            delete_paths: vec![],
            selected_uuids: vec![u1],
        };
        OUTSIDER_BEFORE_PUBLISH.with(|hook| *hook.borrow_mut() = Some((1, outsider.clone())));
        let result = store.execute(&tx);
        OUTSIDER_BEFORE_PUBLISH.with(|hook| *hook.borrow_mut() = None);
        assert!(matches!(result, Err(StoreError::Io(_))));
        assert_eq!(
            fs::read(root.join(existing_rel)).expect("restored existing"),
            original
        );
        assert_eq!(
            fs::read(&outsider).expect("outsider preserved"),
            b"outsider-created-after-preflight"
        );
        assert_eq!(
            root.join(TRANSACTIONS_DIR)
                .read_dir()
                .expect("transactions")
                .count(),
            0
        );
    }

    #[test]
    fn recovery_preserves_unowned_collision_and_its_journal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let rel = "groups/new.parquet";
        let destination = root.join(rel);
        fs::create_dir_all(destination.parent().expect("parent")).expect("dirs");
        fs::write(&destination, b"outsider").expect("outsider");
        let tx_dir = root.join(TRANSACTIONS_DIR).join("tx-ambiguous");
        fs::create_dir_all(tx_dir.join(ORIGINALS_DIR)).expect("tx dir");
        fs::write(
            tx_dir.join(format!("{STAGED_PREFIX}0.parquet")),
            b"transaction output",
        )
        .expect("staged");
        let intent = TransactionIntent {
            transaction_id: "tx-ambiguous".into(),
            action: archaeodash_storage::TransactionAction::MoveUnits,
            inputs: vec![],
            outputs: vec![archaeodash_storage::FileOutput {
                path: rel.into(),
                group_id: "new".into(),
                group_name: "new".into(),
                created: true,
            }],
            selected_uuids: vec![],
            started_at_unix_secs: 0,
        };
        FsGroupFileStore::write_json_atomic(&tx_dir.join(INTENT_FILE), &intent).expect("intent");
        FsGroupFileStore::write_json_atomic(
            &tx_dir.join(BACKUPS_FILE),
            &Vec::<BackupRecord>::new(),
        )
        .expect("backups");
        assert!(matches!(
            store.recover_interrupted(),
            Err(StoreError::Invariant(_))
        ));
        assert_eq!(fs::read(destination).expect("outsider intact"), b"outsider");
        assert!(
            tx_dir.exists(),
            "ambiguous journal remains for safe recovery"
        );
    }

    #[test]
    fn interrupted_transaction_recovers_originals() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let rel = "groups/g1.parquet";
        let path = root.join(rel);
        let u = Uuid::now_v7();
        let u2 = Uuid::now_v7();
        write_initial(&path, "g1", &[row(u, 1.0, None), row(u2, 2.0, Some(3.0))]);
        let original_bytes = std::fs::read(&path).expect("original bytes");

        // Simulate a crash after backup but before commit: the live file is
        // corrupted by a partial publish and a transaction directory holds
        // the backup journal without a COMMIT marker.
        std::fs::create_dir_all(
            root.join(TRANSACTIONS_DIR)
                .join("tx-crash")
                .join(ORIGINALS_DIR),
        )
        .expect("tx dir");
        let key = FsGroupFileStore::path_key(rel);
        std::fs::write(
            root.join(TRANSACTIONS_DIR)
                .join("tx-crash")
                .join(ORIGINALS_DIR)
                .join(format!("{key}.parquet")),
            &original_bytes,
        )
        .expect("backup copy");
        let backups = vec![BackupRecord {
            path: rel.to_string(),
            backup_file: format!("{key}.parquet"),
        }];
        FsGroupFileStore::write_json_atomic(
            &root
                .join(TRANSACTIONS_DIR)
                .join("tx-crash")
                .join(BACKUPS_FILE),
            &backups,
        )
        .expect("backups journal");
        std::fs::write(&path, b"corrupted partial parquet").expect("corrupt live file");
        assert!(validate_group_file(&path).is_err());

        let recovered = store.recover_interrupted().expect("recovery");
        assert_eq!(recovered, 1);
        let restored = read_group_file(&path).expect("restored group validates");
        assert_eq!(restored.profile.group_id, "g1");
        assert_eq!(restored.rows.len(), 2);
        assert!(!root.join(TRANSACTIONS_DIR).join("tx-crash").exists());
    }

    #[test]
    fn interrupted_created_output_is_removed_on_recovery() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let created_rel = "groups/new-group.parquet";
        let created_path = root.join(created_rel);
        std::fs::create_dir_all(created_path.parent().expect("parent")).expect("dirs");

        // Crash between publish and commit of a transaction whose output was
        // newly created (no backup record exists for it).
        let tx_dir = root.join(TRANSACTIONS_DIR).join("tx-created");
        std::fs::create_dir_all(tx_dir.join(ORIGINALS_DIR)).expect("tx dir");
        let staged_path = tx_dir.join(format!("{STAGED_PREFIX}0.parquet"));
        std::fs::write(&staged_path, b"half-published successor").expect("staged output");
        std::fs::hard_link(&staged_path, &created_path).expect("published output");
        let intent = TransactionIntent {
            transaction_id: "tx-created".to_string(),
            action: archaeodash_storage::TransactionAction::MoveUnits,
            inputs: vec![],
            outputs: vec![archaeodash_storage::FileOutput {
                path: created_rel.to_string(),
                group_id: "g-new".to_string(),
                group_name: "g-new".to_string(),
                created: true,
            }],
            selected_uuids: vec![],
            started_at_unix_secs: 0,
        };
        FsGroupFileStore::write_json_atomic(&tx_dir.join(INTENT_FILE), &intent).expect("intent");
        FsGroupFileStore::write_json_atomic(
            &tx_dir.join(BACKUPS_FILE),
            &Vec::<BackupRecord>::new(),
        )
        .expect("backups");

        let recovered = store.recover_interrupted().expect("recovery");
        assert_eq!(recovered, 1);
        assert!(!created_path.exists(), "created output rolled back");
        assert!(!tx_dir.exists());
    }

    #[test]
    fn history_archive_keeps_bounded_revisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let store = FsGroupFileStore::new(root).expect("store");
        let rel = "groups/g1.parquet";
        let u = Uuid::now_v7();

        // Five sequential single-row rewrites; history must hold at most the
        // newest three prior versions after the fifth publish.
        for rev in 0..5u32 {
            let path = root.join(rel);
            let current = read_group_file(&path);
            let inputs = match &current {
                Ok(data) => vec![input_of(rel, data)],
                // First write has no predecessor: no preconditions.
                Err(_) => vec![],
            };
            let rows = vec![row(u, f64::from(rev), None)];
            let out = planned(rel, "g1", &format!("rev-{rev}"), rows, &profile("g1").roles);
            let tx = Transaction {
                action: archaeodash_storage::TransactionAction::MoveUnits,
                inputs,
                outputs: vec![out],
                delete_paths: vec![],
                selected_uuids: vec![],
            };
            store.execute(&tx).expect("execute");
        }
        let key = FsGroupFileStore::path_key(rel);
        let history_count = std::fs::read_dir(root.join(HISTORY_DIR).join(&key))
            .expect("history dir")
            .filter_map(|e| e.ok())
            .count();
        assert!(
            history_count <= MAX_HISTORY_PER_GROUP,
            "history bounded, found {history_count}"
        );
    }
}
