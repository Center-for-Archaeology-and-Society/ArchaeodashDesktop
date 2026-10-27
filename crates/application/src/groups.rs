//! Group use cases shared by the HTTP and Tauri adapters (Section 10.2,
//! Phase 2 local form): candidate discovery, full validation-on-add, and the
//! journaled transfer/merge operations over the transactional group store.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use archaeodash_contracts::{
    AddDescriptiveColumnRequest, ApplyClustersMode, ApplyClustersRequest,
    BatchTransferUnitsRequest, CopyGroupsRequest, CreateFolderRequest, DeleteFolderRequest,
    DeleteGroupRequest, DuplicateGroupRequest, GroupCandidate, GroupFolder, GroupRowDto,
    GroupRowsResponse, GroupSummary, MergeGroupsRequest, PatchDescriptiveValuesRequest,
    RenameFolderRequest, RenameGroupRequest, TransactionResponse, TransferAction,
    TransferUnitsRequest, DEFAULT_CLUSTER_NAME_PREFIX,
};
use archaeodash_data_io::{sanitize_group_name, GroupFileData, GroupProfile, GroupRow};
use archaeodash_file_store_fs::FsGroupFileStore;
use archaeodash_storage::{
    assert_schema_compatible, plan_copy, plan_merge, plan_move, FileInput, GroupFileStore,
    PlannedOutput, StoreError, Transaction, TransactionAction,
};
use uuid::Uuid;

/// Marker file that keeps an empty group folder listed.
pub const FOLDER_MARKER: &str = ".archaeodash-folder";

/// Top-level directories that are never group folders.
const RESERVED_FOLDERS: &[&str] = &["sources"];

/// The folder of a project-relative group path (`/`-separated; `""` for a
/// file in the project root).
pub fn folder_of(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    }
}

/// The file name of a project-relative path.
fn file_name_of(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or(normalized)
}

/// A `/`-separated project-relative folder spelled as it already exists on
/// disk. On a case-insensitive file system (Windows, macOS) "Groups" names an
/// existing "groups" folder; folder checks compare listed paths exactly, so
/// they must see the on-disk spelling. Components that do not exist yet keep
/// the requested spelling.
pub(crate) fn on_disk_folder(root: &Path, folder: &str) -> String {
    let mut dir = root.to_path_buf();
    let mut parts: Vec<String> = Vec::new();
    let mut existing = true;
    for part in folder.split('/').filter(|part| !part.is_empty()) {
        let mut name = part.to_string();
        existing = existing && dir.join(part).is_dir();
        if existing {
            let names: Vec<String> = std::fs::read_dir(&dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| entry.path().is_dir())
                        .map(|entry| entry.file_name().to_string_lossy().to_string())
                        .collect()
                })
                .unwrap_or_default();
            if !names.iter().any(|n| n == part) {
                let lower = part.to_lowercase();
                if let Some(found) = names.iter().find(|n| n.to_lowercase() == lower) {
                    name = found.clone();
                }
            }
        }
        dir.push(&name);
        parts.push(name);
    }
    parts.join("/")
}

/// Checks a new folder name: one path component, portable on Windows,
/// macOS, and Linux, not hidden or reserved. Returns the trimmed name.
pub fn validate_folder_name(name: &str) -> Result<String, StoreError> {
    let name = name.trim().trim_end_matches(['.', ' ']).to_string();
    let invalid = |why: &str| StoreError::Validation(format!("folder name {name:?} {why}"));
    if name.is_empty() {
        return Err(StoreError::Validation("folder name is empty".to_string()));
    }
    if let Some(why) = portable_name_problem(&name) {
        return Err(invalid(why));
    }
    if windows_reserved(&name)
        || RESERVED_FOLDERS
            .iter()
            .any(|r| r.eq_ignore_ascii_case(&name))
    {
        return Err(invalid("is reserved"));
    }
    Ok(name)
}

/// Why `name` cannot be a portable file or folder name (`None` when it
/// can): at most 100 characters, not hidden, and no path separators or
/// characters invalid in Windows, macOS, or Linux file names.
fn portable_name_problem(name: &str) -> Option<&'static str> {
    if name.chars().count() > 100 {
        Some("is longer than 100 characters")
    } else if name.starts_with('.') {
        Some("cannot start with a dot")
    } else if name
        .chars()
        .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        Some("cannot contain < > : \" / \\ | ? *")
    } else {
        None
    }
}

/// Checks a new name for one group (rename): not empty after trimming, and
/// the portable-name rules of folder names, applied to the name and to the
/// group file it gives. Returns the trimmed name.
pub fn validate_group_name(name: &str) -> Result<String, StoreError> {
    let name = name.trim();
    let invalid = |why: &str| StoreError::Validation(format!("group name {name:?} {why}"));
    if name.is_empty() {
        return Err(StoreError::Validation("group name is empty".to_string()));
    }
    if let Some(why) = portable_name_problem(name) {
        return Err(invalid(why));
    }
    if windows_reserved(&sanitize_group_name(name)) {
        return Err(invalid("gives a group file that Windows reserves"));
    }
    Ok(name.to_string())
}

/// A file or folder name Windows reserves (`CON`, `COM1`, `LPT3.txt`, ...).
fn windows_reserved(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or("");
    matches!(stem, "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Checks the text put before each cluster number in the names of new
/// cluster groups (`None` is [`DEFAULT_CLUSTER_NAME_PREFIX`]). The names it
/// gives (`<prefix>1`, ...) follow the folder-name rules: not empty, at most
/// 100 characters, not hidden, no path separators or characters invalid in
/// file names, and no Windows-reserved file. Leading spaces are dropped; a
/// trailing space is kept (`Cluster ` gives `Cluster 1`).
pub fn cluster_name_prefix(prefix: Option<&str>) -> Result<String, StoreError> {
    let Some(prefix) = prefix else {
        return Ok(DEFAULT_CLUSTER_NAME_PREFIX.to_string());
    };
    let prefix = prefix.trim_start();
    let invalid =
        |why: &str| StoreError::Validation(format!("cluster name prefix {prefix:?} {why}"));
    if prefix.is_empty() {
        return Err(StoreError::Validation(
            "cluster name prefix is empty".to_string(),
        ));
    }
    if let Some(why) = portable_name_problem(prefix) {
        return Err(invalid(why));
    }
    if windows_reserved(&sanitize_group_name(&format!("{prefix}1"))) {
        return Err(invalid("gives group files that Windows reserves"));
    }
    Ok(prefix.to_string())
}

/// Group operations rooted at one local project directory. Construction runs
/// the Section 6.8 startup recovery before any other use case.
pub struct GroupService {
    store: FsGroupFileStore,
    root: PathBuf,
}

struct BatchDestination {
    path: String,
    data: Option<GroupFileData>,
    selected: Vec<Uuid>,
    group_id: String,
    group_name: String,
    revision: String,
}

impl GroupService {
    /// Creates the service and recovers any interrupted transactions.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        let store = FsGroupFileStore::new(&root)?;
        store.recover_interrupted()?;
        Ok(Self { store, root })
    }

    /// Manifest-free discovery of Parquet candidates under the project root,
    /// sorted by path (Section 6: metadata earns "ready to add"; catalog
    /// membership is never an eligibility gate).
    pub fn scan_candidates(&self) -> Result<Vec<GroupCandidate>, StoreError> {
        let candidates = self.store.scan_candidates_snapshot()?;
        Ok(self.to_candidates(candidates))
    }

    /// Candidates directly inside one folder (`""`: the project root only).
    fn folder_candidates(&self, folder: &str) -> Result<Vec<GroupCandidate>, StoreError> {
        let candidates = self.store.scan_folder_snapshot(folder)?;
        Ok(self.to_candidates(candidates))
    }

    fn to_candidates(
        &self,
        candidates: Vec<archaeodash_data_io::ScanCandidate>,
    ) -> Vec<GroupCandidate> {
        let mut out = Vec::new();
        for candidate in candidates {
            let path = candidate
                .path
                .strip_prefix(&self.root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| candidate.path.to_string_lossy().to_string());
            let ready = matches!(
                candidate.status,
                archaeodash_data_io::CandidateStatus::ReadyToAdd(_)
            );
            let group = match candidate.status {
                archaeodash_data_io::CandidateStatus::ReadyToAdd(profile) => {
                    Some(Self::summary(&path, &profile))
                }
                _ => None,
            };
            out.push(GroupCandidate { path, ready, group });
        }
        out
    }

    /// Runs a transaction after checking that no folder it writes would hold
    /// the same sample (analytical UUID) twice. Descriptive edits, column
    /// additions, renames, and deletes never add rows, so they skip the check.
    fn execute(&self, tx: &Transaction) -> Result<String, StoreError> {
        if !matches!(
            tx.action,
            TransactionAction::PatchDescriptive
                | TransactionAction::DeleteGroup
                | TransactionAction::RenameGroup
                | TransactionAction::AddDescriptiveColumn
        ) {
            self.ensure_unique_samples_per_folder(tx)?;
        }
        self.store.execute(tx)
    }

    /// A folder is one grouping of the samples, so it may hold each sample at
    /// most once. Only rows the transaction writes are checked (existing
    /// overlaps between untouched files do not block unrelated work).
    fn ensure_unique_samples_per_folder(&self, tx: &Transaction) -> Result<(), StoreError> {
        let replaced: HashSet<String> = tx
            .outputs
            .iter()
            .map(|o| o.path.replace('\\', "/"))
            .chain(tx.delete_paths.iter().map(|p| p.replace('\\', "/")))
            .collect();
        let mut by_folder: BTreeMap<String, Vec<&PlannedOutput>> = BTreeMap::new();
        for out in &tx.outputs {
            by_folder.entry(folder_of(&out.path)).or_default().push(out);
        }
        if by_folder.is_empty() {
            return Ok(());
        }
        let candidates = self.scan_candidates()?;
        for (folder, outputs) in by_folder {
            let others: Vec<String> = candidates
                .iter()
                .filter(|c| c.ready && folder_of(&c.path) == folder)
                .map(|c| c.path.clone())
                .filter(|p| !replaced.contains(&p.replace('\\', "/")))
                .collect();
            let mut owner: HashMap<Uuid, String> = HashMap::new();
            for data in self
                .store
                .read_groups_snapshot(&others)?
                .into_iter()
                .flatten()
            {
                for row in &data.rows {
                    owner
                        .entry(row.uuid)
                        .or_insert_with(|| data.profile.group_name.clone());
                }
            }
            for out in outputs {
                let mut clashes: BTreeMap<String, usize> = BTreeMap::new();
                for row in &out.rows {
                    match owner.get(&row.uuid) {
                        Some(other) => *clashes.entry(other.clone()).or_default() += 1,
                        None => {
                            owner.insert(row.uuid, out.group_name.clone());
                        }
                    }
                }
                if let Some((other, count)) = clashes.into_iter().next() {
                    let shown = if folder.is_empty() {
                        "the project root"
                    } else {
                        &folder
                    };
                    return Err(StoreError::Validation(format!(
                        "folder \u{201c}{shown}\u{201d} would hold the same samples twice: \
                         {count} row{} of \u{201c}{}\u{201d} {} already in \u{201c}{other}\u{201d}. \
                         A folder holds each sample once; copy the group into another folder instead",
                        if count == 1 { "" } else { "s" },
                        out.group_name,
                        if count == 1 { "is" } else { "are" },
                    )));
                }
            }
        }
        Ok(())
    }

    /// Group folders: directories holding group files, plus empty folders
    /// created here (kept by a marker file). Sorted by path.
    pub fn list_folders(&self) -> Result<Vec<GroupFolder>, StoreError> {
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        for candidate in self.scan_candidates()? {
            if candidate.ready {
                *counts.entry(folder_of(&candidate.path)).or_default() += 1;
            }
        }
        let root = self.store.resolve("")?;
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') && entry.path().join(FOLDER_MARKER).is_file() {
                    counts.entry(name).or_default();
                }
            }
        }
        Ok(counts
            .into_iter()
            .map(|(path, group_count)| GroupFolder {
                name: path.rsplit('/').next().unwrap_or(&path).to_string(),
                path,
                group_count,
            })
            .collect())
    }

    /// Creates an empty top-level group folder.
    pub fn create_folder(&self, req: &CreateFolderRequest) -> Result<GroupFolder, StoreError> {
        let name = validate_folder_name(&req.name)?;
        let dir = self.store.resolve(&name)?;
        if dir.exists() {
            return Err(StoreError::Validation(format!(
                "a folder named \u{201c}{name}\u{201d} already exists"
            )));
        }
        let _lock = self.store.lock_exclusive()?;
        std::fs::create_dir_all(&dir).map_err(|e| StoreError::Io(e.to_string()))?;
        std::fs::write(dir.join(FOLDER_MARKER), b"").map_err(|e| StoreError::Io(e.to_string()))?;
        Ok(GroupFolder {
            path: name.clone(),
            name,
            group_count: 0,
        })
    }

    /// Renames a group folder (same parent). Group files keep their contents
    /// and identities; only their paths change.
    pub fn rename_folder(&self, req: &RenameFolderRequest) -> Result<GroupFolder, StoreError> {
        let from = req.path.replace('\\', "/");
        if from.is_empty() || from.split('/').any(|part| part.starts_with('.')) {
            return Err(StoreError::Validation(
                "choose a group folder to rename".to_string(),
            ));
        }
        let name = validate_folder_name(&req.new_name)?;
        let parent = folder_of(&from);
        let to = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent}/{name}")
        };
        if to == from {
            return self.folder_summary(&to);
        }
        let from_abs = self.store.resolve(&from)?;
        let to_abs = self.store.resolve(&to)?;
        if !from_abs.is_dir() {
            return Err(StoreError::Validation(format!(
                "folder \u{201c}{from}\u{201d} does not exist"
            )));
        }
        // A case-only rename is allowed on case-insensitive file systems.
        if to_abs.exists() && !from.eq_ignore_ascii_case(&to) {
            return Err(StoreError::Validation(format!(
                "a folder named \u{201c}{name}\u{201d} already exists"
            )));
        }
        {
            let _lock = self.store.lock_exclusive()?;
            std::fs::rename(&from_abs, &to_abs).map_err(|e| {
                StoreError::Io(format!(
                    "could not rename the folder (is a file in it open?): {e}"
                ))
            })?;
        }
        self.folder_summary(&to)
    }

    /// Deletes an empty group folder (no group files inside).
    pub fn delete_folder(&self, req: &DeleteFolderRequest) -> Result<(), StoreError> {
        let path = req.path.replace('\\', "/");
        if path.is_empty() || path.split('/').any(|part| part.starts_with('.')) {
            return Err(StoreError::Validation(
                "choose a group folder to delete".to_string(),
            ));
        }
        let groups = self
            .scan_candidates()?
            .into_iter()
            .filter(|c| {
                folder_of(&c.path) == path
                    || c.path.replace('\\', "/").starts_with(&format!("{path}/"))
            })
            .count();
        if groups > 0 {
            return Err(StoreError::Validation(format!(
                "folder \u{201c}{path}\u{201d} still holds {groups} group file{}; delete or move them first",
                if groups == 1 { "" } else { "s" }
            )));
        }
        let dir = self.store.resolve(&path)?;
        let _lock = self.store.lock_exclusive()?;
        let marker = dir.join(FOLDER_MARKER);
        if marker.exists() {
            std::fs::remove_file(&marker).map_err(|e| StoreError::Io(e.to_string()))?;
        }
        std::fs::remove_dir(&dir).map_err(|e| {
            StoreError::Validation(format!(
                "folder \u{201c}{path}\u{201d} is not empty (it holds other files): {e}"
            ))
        })
    }

    fn folder_summary(&self, path: &str) -> Result<GroupFolder, StoreError> {
        Ok(self
            .list_folders()?
            .into_iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| GroupFolder {
                path: path.to_string(),
                name: path.rsplit('/').next().unwrap_or(path).to_string(),
                group_count: 0,
            }))
    }

    /// Copies (or moves) whole groups into a folder in one transaction,
    /// keeping file names, group names, sample identities, and lineage.
    /// Refused when the folder would hold any sample twice.
    pub fn copy_groups(&self, req: &CopyGroupsRequest) -> Result<TransactionResponse, StoreError> {
        if req.sources.is_empty() {
            return Err(StoreError::Validation(
                "choose at least one group to copy".to_string(),
            ));
        }
        let folder = req.destination_folder.replace('\\', "/");
        let folder = folder.trim_matches('/').to_string();
        for part in folder.split('/') {
            validate_folder_name(part)?;
        }
        let folder = on_disk_folder(&self.root, &folder);
        let paths: Vec<String> = req.sources.iter().map(|s| s.path.clone()).collect();
        let sources = self.store.read_groups_snapshot(&paths)?;
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        let mut selected = Vec::new();
        let mut destinations = HashSet::new();
        for (source, data) in req.sources.iter().zip(sources) {
            let data = data.ok_or_else(|| {
                StoreError::Validation(format!("group file {} no longer exists", source.path))
            })?;
            if data.profile.revision_id != source.expected_revision {
                return Err(StoreError::RevisionConflict {
                    path: source.path.clone(),
                    expected: source.expected_revision.clone(),
                    found: data.profile.revision_id.clone(),
                });
            }
            if req.remove_sources && folder_of(&source.path) == folder {
                return Err(StoreError::Validation(format!(
                    "\u{201c}{}\u{201d} is already in folder \u{201c}{folder}\u{201d}",
                    data.profile.group_name
                )));
            }
            let destination = format!("{folder}/{}", file_name_of(&source.path));
            if self.store.resolve(&destination)?.exists()
                || !destinations.insert(destination.clone())
            {
                return Err(StoreError::Validation(format!(
                    "folder \u{201c}{folder}\u{201d} already has a group file named {}",
                    file_name_of(&source.path)
                )));
            }
            selected.extend(data.rows.iter().map(|r| r.uuid));
            inputs.push(Self::input_of(&source.path, &data));
            outputs.push(PlannedOutput {
                path: destination,
                group_id: data.profile.group_id.clone(),
                group_name: data.profile.group_name.clone(),
                revision_id: "rev-1".to_string(),
                rows: data.rows.clone(),
                roles: data.profile.roles.clone(),
                recipe: data.profile.import_recipe.clone(),
                source_path: data.profile.source_path.clone(),
                source_sha256: data.profile.source_sha256.clone(),
            });
        }
        let dir = self.store.resolve(&folder)?;
        let created_dir = !dir.exists();
        if created_dir {
            std::fs::create_dir_all(&dir).map_err(|e| StoreError::Io(e.to_string()))?;
            std::fs::write(dir.join(FOLDER_MARKER), b"")
                .map_err(|e| StoreError::Io(e.to_string()))?;
        }
        let tx = Transaction {
            action: if req.remove_sources {
                TransactionAction::MoveGroups
            } else {
                TransactionAction::CopyGroups
            },
            inputs,
            outputs: outputs.clone(),
            delete_paths: if req.remove_sources {
                paths.clone()
            } else {
                Vec::new()
            },
            selected_uuids: selected,
        };
        let result = self.execute(&tx);
        if result.is_err() && created_dir {
            // Leave no empty folder behind when the copy is refused.
            let _ = std::fs::remove_file(dir.join(FOLDER_MARKER));
            let _ = std::fs::remove_dir(&dir);
        }
        let transaction_id = result?;
        Ok(TransactionResponse {
            transaction_id,
            action: if req.remove_sources {
                "move_groups"
            } else {
                "copy_groups"
            }
            .to_string(),
            outputs: outputs
                .iter()
                .map(|out| Self::summary_of_output(&out.path, out))
                .collect(),
            deleted_paths: if req.remove_sources {
                paths
            } else {
                Vec::new()
            },
        })
    }

    /// Full validation-on-add: profile, schema, identities, checksum.
    pub fn validate(&self, path: &str) -> Result<GroupSummary, StoreError> {
        let profile = self.store.validate_group(path)?;
        Ok(Self::summary(path, &profile))
    }

    fn summary(path: &str, profile: &GroupProfile) -> GroupSummary {
        GroupSummary {
            path: path.to_string(),
            group_id: profile.group_id.clone(),
            group_name: profile.group_name.clone(),
            revision_id: profile.revision_id.clone(),
            row_count: profile.row_count as u64,
            source_path: profile.source_path.clone(),
            source_sha256: profile.source_sha256.clone(),
            elemental_columns: profile.roles.elemental.clone(),
            descriptive_columns: profile.roles.descriptive.clone(),
        }
    }

    /// Full row data for the client dataset table (Section 9.4 Explore):
    /// hidden UUIDs for edit addressing, role columns for table headers.
    pub fn rows(&self, path: &str) -> Result<GroupRowsResponse, StoreError> {
        let data = self.store.read_group(path)?;
        let roles = &data.profile.roles;
        Ok(GroupRowsResponse {
            path: path.to_string(),
            revision_id: data.profile.revision_id.clone(),
            visible_id_column: roles.visible_id.clone(),
            legacy_rowid_column: roles.legacy_rowid.clone(),
            descriptive_columns: roles.descriptive.clone(),
            elemental_columns: roles.elemental.clone(),
            rows: data
                .rows
                .iter()
                .map(|row| GroupRowDto {
                    analytical_uuid: row.uuid.to_string(),
                    legacy_rowid: row.legacy_rowid.clone(),
                    visible_id: row.visible.clone(),
                    descriptive: row.descriptive.clone(),
                    elemental: row.elemental.clone(),
                })
                .collect(),
        })
    }

    fn summary_of_output(path: &str, out: &PlannedOutput) -> GroupSummary {
        GroupSummary {
            path: path.to_string(),
            group_id: out.group_id.clone(),
            group_name: out.group_name.clone(),
            revision_id: out.revision_id.clone(),
            row_count: out.rows.len() as u64,
            source_path: out.source_path.clone(),
            source_sha256: out.source_sha256.clone(),
            elemental_columns: out.roles.elemental.clone(),
            descriptive_columns: out.roles.descriptive.clone(),
        }
    }

    /// `rev-N` bumps to `rev-(N+1)`; any other revision format falls back to
    /// a fresh UUIDv7 so every publish gets a distinct revision id.
    fn next_revision(current: &str) -> String {
        current
            .strip_prefix("rev-")
            .and_then(|n| n.parse::<u64>().ok())
            .map(|n| format!("rev-{}", n + 1))
            .unwrap_or_else(|| Uuid::now_v7().simple().to_string())
    }

    fn input_of(path: &str, data: &GroupFileData) -> FileInput {
        FileInput {
            path: path.to_string(),
            revision_id: data.profile.revision_id.clone(),
            measured_elemental_checksum: data.profile.measured_elemental_checksum.clone(),
        }
    }

    fn parse_uuids(raw: &[String]) -> Result<Vec<Uuid>, StoreError> {
        raw.iter()
            .map(|s| {
                Uuid::parse_str(s)
                    .map_err(|e| StoreError::Invariant(format!("invalid analytical UUID: {e}")))
            })
            .collect()
    }

    fn normalized_path(path: &str) -> Result<String, StoreError> {
        let resolved = PathBuf::from(path);
        let mut parts = Vec::new();
        for component in resolved.components() {
            match component {
                std::path::Component::Normal(part) => {
                    parts.push(part.to_string_lossy().to_string())
                }
                std::path::Component::CurDir => {}
                other => {
                    return Err(StoreError::Invariant(format!(
                        "invalid group path {path:?}: {other:?}"
                    )))
                }
            }
        }
        if parts.is_empty() {
            return Err(StoreError::Invariant("group path cannot be empty".into()));
        }
        Ok(parts.join("/"))
    }

    fn reject_symlink_path(&self, path: &str) -> Result<(), StoreError> {
        let mut current = self.root.clone();
        for component in PathBuf::from(path).components() {
            if let std::path::Component::Normal(part) = component {
                current.push(part);
                match std::fs::symlink_metadata(&current) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        return Err(StoreError::Invariant(format!(
                            "symlink paths are not supported for batch transfers: {path}"
                        )))
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                    Err(e) => return Err(StoreError::Io(e.to_string())),
                }
            }
        }
        Ok(())
    }

    /// Turns a clustering of whole groups into groups named `Cluster 1`,
    /// `Cluster 2`, ... (or `<name_prefix>1`, ...) in one journaled
    /// transaction. Rows keep their identities, values, and descriptive
    /// cells, in source order.
    ///
    /// - `Replace`: the analyzed groups are replaced by the cluster groups in
    ///   their folder; the folder's other groups are untouched, and a cluster
    ///   name they already use gets a ` (2)`, ` (3)`, ... suffix.
    /// - `NewFolder`: the cluster groups go to a new folder beside the
    ///   analyzed one (`<folder> clusters`, then ` 2`, ...); the analyzed
    ///   groups are untouched.
    ///
    /// The clusters must cover every row of the sources exactly once, so a
    /// stale clustering (rows added or removed since) is refused.
    pub fn apply_clusters(
        &self,
        req: &ApplyClustersRequest,
    ) -> Result<TransactionResponse, StoreError> {
        let invalid = |message: &str| StoreError::Validation(message.to_string());
        let prefix = cluster_name_prefix(req.name_prefix.as_deref())?;
        if req.sources.is_empty() {
            return Err(invalid("choose at least one analyzed group"));
        }
        if req.clusters.is_empty() || req.clusters.len() > 1000 {
            return Err(invalid("a clustering needs between 1 and 1,000 clusters"));
        }
        let folder = folder_of(&req.sources[0].path);
        let mut listed = HashSet::new();
        for source in &req.sources {
            if folder_of(&source.path) != folder {
                return Err(invalid(
                    "the analyzed groups must all be in one folder to be replaced by clusters",
                ));
            }
            if !listed.insert(source.path.replace('\\', "/").to_lowercase()) {
                return Err(invalid("an analyzed group is listed twice"));
            }
            Self::normalized_path(&source.path)?;
            self.reject_symlink_path(&source.path)?;
        }
        let paths: Vec<String> = req.sources.iter().map(|s| s.path.clone()).collect();
        let mut sources: Vec<(String, GroupFileData)> = Vec::new();
        let mut inputs = Vec::new();
        for (source, data) in req
            .sources
            .iter()
            .zip(self.store.read_groups_snapshot(&paths)?)
        {
            let data = data.ok_or_else(|| {
                StoreError::Validation(format!("group file {} no longer exists", source.path))
            })?;
            if data.profile.revision_id != source.expected_revision {
                return Err(StoreError::RevisionConflict {
                    path: source.path.clone(),
                    expected: source.expected_revision.clone(),
                    found: data.profile.revision_id.clone(),
                });
            }
            inputs.push(Self::input_of(&source.path, &data));
            sources.push((source.path.clone(), data));
        }
        for (path, data) in &sources[1..] {
            assert_schema_compatible(&sources[0].0, &sources[0].1, path, data)?;
        }

        // Rows per cluster, in source order.
        let mut cluster_of: HashMap<Uuid, usize> = HashMap::new();
        for (cluster, uuids) in req.clusters.iter().enumerate() {
            for uuid in Self::parse_uuids(uuids)? {
                if cluster_of.insert(uuid, cluster).is_some() {
                    return Err(invalid("a sample is listed in more than one cluster"));
                }
            }
        }
        let mut rows_of: Vec<Vec<GroupRow>> = vec![Vec::new(); req.clusters.len()];
        let mut missing = 0usize;
        let mut placed = 0usize;
        for (_, data) in &sources {
            for row in &data.rows {
                match cluster_of.get(&row.uuid) {
                    Some(&cluster) => {
                        rows_of[cluster].push(row.clone());
                        placed += 1;
                    }
                    None => missing += 1,
                }
            }
        }
        if missing > 0 || placed != cluster_of.len() {
            return Err(StoreError::Validation(format!(
                "the clustering does not match the analyzed groups ({missing} of their \
                 samples are not in it); run the analysis again"
            )));
        }

        // Destination folder.
        let replace = req.mode == ApplyClustersMode::Replace;
        let source_folder = on_disk_folder(&self.root, &folder);
        let destination = if replace {
            source_folder.clone()
        } else {
            self.free_cluster_folder(&source_folder)?
        };
        let dir = self.store.resolve(&destination)?;
        let created_dir = !replace && !dir.exists();
        let lower = |text: &str| text.to_lowercase();
        let source_by_file: HashMap<String, (String, String)> = sources
            .iter()
            .map(|(path, data)| {
                (
                    lower(&file_name_of(path)),
                    (path.clone(), data.profile.revision_id.clone()),
                )
            })
            .collect();
        // Names and files in use by groups that stay (Replace keeps the
        // folder's other groups; a new folder starts empty).
        let mut taken_names: HashSet<String> = HashSet::new();
        let mut taken_files: HashSet<String> = HashSet::new();
        if replace {
            for candidate in self.scan_candidates()? {
                if folder_of(&candidate.path) == folder_of(&sources[0].0)
                    && !source_by_file.contains_key(&lower(&file_name_of(&candidate.path)))
                {
                    if let Some(group) = &candidate.group {
                        taken_names.insert(lower(&group.group_name));
                    }
                }
            }
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = lower(&entry.file_name().to_string_lossy());
                    if !source_by_file.contains_key(&name) {
                        taken_files.insert(name);
                    }
                }
            }
        }
        let first = &sources[0].1.profile;
        let mut outputs = Vec::new();
        for (cluster, rows) in rows_of.into_iter().enumerate() {
            if rows.is_empty() {
                continue;
            }
            let base = format!("{prefix}{}", cluster + 1);
            let mut name = base.clone();
            let mut suffix = 2;
            let file = loop {
                let file = format!("{}.parquet", sanitize_group_name(&name));
                if !taken_names.contains(&lower(&name)) && !taken_files.contains(&lower(&file)) {
                    break file;
                }
                name = format!("{base} ({suffix})");
                suffix += 1;
            };
            taken_names.insert(lower(&name));
            taken_files.insert(lower(&file));
            // Replacing: a cluster that reuses an analyzed group's file name
            // overwrites that file instead of deleting and recreating it.
            let (path, revision_id) = match source_by_file.get(&lower(&file)) {
                Some((path, revision)) if replace => (path.clone(), Self::next_revision(revision)),
                _ if destination.is_empty() => (file, "rev-1".to_string()),
                _ => (format!("{destination}/{file}"), "rev-1".to_string()),
            };
            outputs.push(PlannedOutput {
                path,
                group_id: sanitize_group_name(&name),
                group_name: name,
                revision_id,
                rows,
                roles: first.roles.clone(),
                recipe: first.import_recipe.clone(),
                source_path: first.source_path.clone(),
                source_sha256: first.source_sha256.clone(),
            });
        }
        let written: HashSet<String> = outputs
            .iter()
            .map(|o| o.path.replace('\\', "/").to_lowercase())
            .collect();
        let delete_paths: Vec<String> = if replace {
            paths
                .iter()
                .filter(|p| !written.contains(&p.replace('\\', "/").to_lowercase()))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        if created_dir {
            std::fs::create_dir_all(&dir).map_err(|e| StoreError::Io(e.to_string()))?;
            std::fs::write(dir.join(FOLDER_MARKER), b"")
                .map_err(|e| StoreError::Io(e.to_string()))?;
        }
        let tx = Transaction {
            action: TransactionAction::ApplyClusters,
            inputs,
            outputs: outputs.clone(),
            delete_paths: delete_paths.clone(),
            selected_uuids: cluster_of.keys().copied().collect(),
        };
        let result = self.execute(&tx);
        if result.is_err() && created_dir {
            // Leave no empty folder behind when the transaction is refused.
            let _ = std::fs::remove_file(dir.join(FOLDER_MARKER));
            let _ = std::fs::remove_dir(&dir);
        }
        Ok(TransactionResponse {
            transaction_id: result?,
            action: "apply_clusters".to_string(),
            outputs: outputs
                .iter()
                .map(|out| Self::summary_of_output(&out.path, out))
                .collect(),
            deleted_paths: delete_paths,
        })
    }

    /// An unused folder beside `folder` for cluster groups: `<name> clusters`,
    /// then `<name> clusters 2`, ... (`Clusters` for the project root).
    fn free_cluster_folder(&self, folder: &str) -> Result<String, StoreError> {
        let (parent, name) = match folder.rsplit_once('/') {
            Some((parent, name)) => (parent.to_string(), name.to_string()),
            None => (String::new(), folder.to_string()),
        };
        let base = if name.is_empty() {
            "Clusters".to_string()
        } else {
            format!("{name} clusters")
        };
        let parent_dir = if parent.is_empty() {
            self.root.clone()
        } else {
            self.store.resolve(&parent)?
        };
        let existing: HashSet<String> = std::fs::read_dir(&parent_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        for n in 1..10_000 {
            let candidate = if n == 1 {
                base.clone()
            } else {
                format!("{base} {n}")
            };
            let candidate = validate_folder_name(&candidate)?;
            if !existing.contains(&candidate.to_lowercase()) {
                return Ok(if parent.is_empty() {
                    candidate
                } else {
                    format!("{parent}/{candidate}")
                });
            }
        }
        Err(StoreError::Validation(
            "no free folder name for the cluster groups".to_string(),
        ))
    }

    /// Moves one or more UUID selections to distinct destinations using one
    /// journaled transaction, so validation failure leaves every group intact.
    pub fn batch_transfer_units(
        &self,
        req: &BatchTransferUnitsRequest,
    ) -> Result<TransactionResponse, StoreError> {
        if req.targets.is_empty() || req.targets.len() > 1000 {
            return Err(StoreError::Invariant(
                "batch transfer requires between 1 and 1000 targets".into(),
            ));
        }
        let source_key = Self::normalized_path(&req.source_path)?;
        self.reject_symlink_path(&req.source_path)?;
        let mut snapshot_paths = vec![req.source_path.clone()];
        let mut checked_paths = std::collections::HashSet::new();
        for target in &req.targets {
            let key = Self::normalized_path(&target.destination_path)?;
            if key == source_key || !checked_paths.insert(key) {
                return Err(StoreError::Invariant(
                    "batch transfer destinations must be distinct from each other and the source"
                        .into(),
                ));
            }
            self.reject_symlink_path(&target.destination_path)?;
            snapshot_paths.push(target.destination_path.clone());
        }
        let mut snapshot = self
            .store
            .read_groups_snapshot(&snapshot_paths)?
            .into_iter();
        let original_source = snapshot
            .next()
            .flatten()
            .ok_or_else(|| StoreError::Io(format!("group not found: {}", req.source_path)))?;
        let source_abs = self.store.resolve(&req.source_path)?;
        if original_source.profile.revision_id != req.expected_source_revision {
            return Err(StoreError::RevisionConflict {
                path: req.source_path.clone(),
                expected: req.expected_source_revision.clone(),
                found: original_source.profile.revision_id.clone(),
            });
        }

        let mut seen_paths = std::collections::HashSet::new();
        let mut seen_uuids = std::collections::HashSet::new();
        let mut selected_all = Vec::new();
        let mut destinations: Vec<BatchDestination> = Vec::with_capacity(req.targets.len());
        let mut inputs = vec![Self::input_of(&req.source_path, &original_source)];
        let source_ids: std::collections::HashSet<Uuid> =
            original_source.rows.iter().map(|r| r.uuid).collect();
        let mut create_only_paths = Vec::new();

        for target in &req.targets {
            let key = Self::normalized_path(&target.destination_path)?;
            if key == source_key || !seen_paths.insert(key) {
                return Err(StoreError::Invariant(
                    "batch transfer destinations must be distinct from each other and the source"
                        .into(),
                ));
            }
            self.reject_symlink_path(&target.destination_path)?;
            if target.selected_uuids.is_empty() {
                return Err(StoreError::Invariant(
                    "each batch transfer target needs at least one UUID".into(),
                ));
            }
            let selected = Self::parse_uuids(&target.selected_uuids)?;
            for uuid in &selected {
                if !seen_uuids.insert(*uuid) {
                    return Err(StoreError::Invariant(
                        "an analytical UUID is assigned more than once".into(),
                    ));
                }
                if !source_ids.contains(uuid) {
                    return Err(StoreError::Invariant(
                        "one or more analytical UUIDs are not present in the source".into(),
                    ));
                }
            }
            selected_all.extend(selected.iter().copied());

            let abs = self.store.resolve(&target.destination_path)?;
            let data = snapshot.next().flatten();
            let exists = data.is_some();
            if exists {
                let aliases_source = same_file::is_same_file(&source_abs, &abs).unwrap_or(false);
                let aliases_destination = destinations.iter().any(|prior| {
                    prior.data.as_ref().is_some_and(|_| {
                        self.store
                            .resolve(&prior.path)
                            .ok()
                            .is_some_and(|prior_abs| {
                                same_file::is_same_file(&prior_abs, &abs).unwrap_or(false)
                            })
                    })
                });
                if aliases_source || aliases_destination {
                    return Err(StoreError::Invariant(
                        "batch transfer paths must refer to distinct files".into(),
                    ));
                }
            }
            let (id, name, revision) = match &data {
                Some(data) => {
                    let expected = target.expected_destination_revision.as_deref().ok_or_else(|| StoreError::Invariant(format!("expected_destination_revision is required for existing destination {}", target.destination_path)))?;
                    if expected != data.profile.revision_id {
                        return Err(StoreError::RevisionConflict {
                            path: target.destination_path.clone(),
                            expected: expected.to_string(),
                            found: data.profile.revision_id.clone(),
                        });
                    }
                    if target.destination_group_name.is_some() {
                        return Err(StoreError::Invariant(
                            "destination_group_name is only valid for a new destination".into(),
                        ));
                    }
                    (
                        data.profile.group_id.clone(),
                        data.profile.group_name.clone(),
                        Self::next_revision(&data.profile.revision_id),
                    )
                }
                None => {
                    if target.expected_destination_revision.is_some() {
                        return Err(StoreError::Invariant(format!(
                            "new destination {} must not specify a revision",
                            target.destination_path
                        )));
                    }
                    let name = target
                        .destination_group_name
                        .as_deref()
                        .filter(|n| !n.trim().is_empty())
                        .ok_or_else(|| {
                            StoreError::Invariant(
                                "new destination requires a nonempty destination_group_name".into(),
                            )
                        })?;
                    create_only_paths.push(target.destination_path.clone());
                    let id = sanitize_group_name(name);
                    (id.clone(), name.to_string(), "rev-1".to_string())
                }
            };
            if let Some(data) = &data {
                inputs.push(Self::input_of(&target.destination_path, data));
            }
            destinations.push(BatchDestination {
                path: target.destination_path.clone(),
                data,
                selected,
                group_id: id,
                group_name: name,
                revision,
            });
        }

        let mut current_source = original_source.clone();
        let source_revision = Self::next_revision(&original_source.profile.revision_id);
        let mut destination_outputs = Vec::new();
        for destination in destinations {
            let (source_out, dest_out) = plan_move(
                &req.source_path,
                &current_source,
                Some((&destination.path, destination.data.as_ref())),
                &destination.selected,
                &source_revision,
                &destination.group_id,
                &destination.group_name,
                &destination.revision,
            )?;
            current_source = match source_out {
                Some(out) => GroupFileData {
                    profile: original_source.profile.clone(),
                    rows: out.rows,
                },
                None => GroupFileData {
                    profile: original_source.profile.clone(),
                    rows: Vec::new(),
                },
            };
            if let Some(out) = dest_out {
                destination_outputs.push(out);
            }
        }
        let mut delete_paths = Vec::new();
        let source_out = if current_source.rows.is_empty() {
            delete_paths.push(req.source_path.clone());
            None
        } else {
            Some(PlannedOutput {
                path: req.source_path.clone(),
                group_id: original_source.profile.group_id.clone(),
                group_name: original_source.profile.group_name.clone(),
                revision_id: source_revision,
                rows: current_source.rows,
                roles: original_source.profile.roles.clone(),
                recipe: original_source.profile.import_recipe.clone(),
                source_path: original_source.profile.source_path.clone(),
                source_sha256: original_source.profile.source_sha256.clone(),
            })
        };
        // Detect a newly appeared create-only path before handing off to the executor.
        for path in &create_only_paths {
            if self.store.resolve(path)?.exists() {
                return Err(StoreError::Invariant(format!(
                    "new destination appeared during planning: {path}"
                )));
            }
        }
        let mut outputs: Vec<PlannedOutput> = source_out.into_iter().collect();
        outputs.extend(destination_outputs);
        let tx = Transaction {
            action: TransactionAction::MoveUnits,
            inputs,
            outputs,
            delete_paths,
            selected_uuids: selected_all,
        };
        let transaction_id = self.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "move_units".into(),
            outputs: tx
                .outputs
                .iter()
                .map(|out| Self::summary_of_output(&out.path, out))
                .collect(),
            deleted_paths: tx.delete_paths.clone(),
        })
    }

    /// Moves or copies selected analytical units between groups through one
    /// journaled transaction (Section 10.2 `POST /groups/transfer-units`).
    pub fn transfer_units(
        &self,
        req: &TransferUnitsRequest,
    ) -> Result<TransactionResponse, StoreError> {
        let action = match req.action {
            TransferAction::Move => TransactionAction::MoveUnits,
            TransferAction::Copy => TransactionAction::CopyUnits,
        };
        let selected = Self::parse_uuids(&req.selected_uuids)?;

        let dest_path = req.destination_path.as_str();
        let snapshot_paths = vec![req.source_path.clone(), req.destination_path.clone()];
        let mut snapshot = self
            .store
            .read_groups_snapshot(&snapshot_paths)?
            .into_iter();
        let source = snapshot
            .next()
            .flatten()
            .ok_or_else(|| StoreError::Io(format!("group not found: {}", req.source_path)))?;
        if source.profile.revision_id != req.expected_source_revision {
            return Err(StoreError::RevisionConflict {
                path: req.source_path.clone(),
                expected: req.expected_source_revision.clone(),
                found: source.profile.revision_id.clone(),
            });
        }

        if req.action == TransferAction::Copy && dest_path == req.source_path {
            return Err(StoreError::Invariant(
                "copy destination must differ from the source".to_string(),
            ));
        }
        let dest_data = snapshot.next().flatten();

        let (destination_group_id, destination_group_name, dest_revision) = match &dest_data {
            Some(data) => (
                data.profile.group_id.clone(),
                data.profile.group_name.clone(),
                Self::next_revision(&data.profile.revision_id),
            ),
            None => {
                let name = req.destination_group_name.as_deref().ok_or_else(|| {
                    StoreError::Invariant(
                        "destination_group_name is required for a new destination group"
                            .to_string(),
                    )
                })?;
                let id = sanitize_group_name(name);
                (id.clone(), name.to_string(), "rev-1".to_string())
            }
        };

        let source_revision = Self::next_revision(&source.profile.revision_id);
        let mut inputs = vec![Self::input_of(&req.source_path, &source)];
        let mut delete_paths = Vec::new();

        let (source_out, dest_out) = match action {
            TransactionAction::MoveUnits => {
                let (source_out, dest_out) = plan_move(
                    &req.source_path,
                    &source,
                    Some((dest_path, dest_data.as_ref())),
                    &selected,
                    &source_revision,
                    &destination_group_id,
                    &destination_group_name,
                    &dest_revision,
                )?;
                if source_out.is_none() {
                    // Every unit moved out; the source file ceases to exist.
                    delete_paths.push(req.source_path.clone());
                }
                (source_out, dest_out)
            }
            TransactionAction::CopyUnits => {
                let dest_out = plan_copy(
                    &req.source_path,
                    &source,
                    dest_path,
                    dest_data.as_ref(),
                    &selected,
                    &destination_group_id,
                    &destination_group_name,
                    &dest_revision,
                )?;
                (None, dest_out)
            }
            TransactionAction::MergeGroups => {
                return Err(StoreError::Invariant(
                    "merge_groups uses merge_groups, not transfer_units".to_string(),
                ));
            }
            TransactionAction::CopyGroups | TransactionAction::MoveGroups => {
                return Err(StoreError::Invariant(
                    "copy_groups uses copy_groups, not transfer_units".to_string(),
                ));
            }
            TransactionAction::ApplyClusters => {
                return Err(StoreError::Invariant(
                    "apply_clusters uses apply_clusters, not transfer_units".to_string(),
                ));
            }
            TransactionAction::DeleteGroup => {
                return Err(StoreError::Invariant(
                    "delete_group uses delete_group, not transfer_units".to_string(),
                ));
            }
            TransactionAction::PatchDescriptive => {
                return Err(StoreError::Invariant(
                    "patch_descriptive_values uses patch_descriptive_values, not transfer_units"
                        .to_string(),
                ));
            }
            TransactionAction::DuplicateGroup => {
                return Err(StoreError::Invariant(
                    "duplicate_group uses duplicate_group, not transfer_units".to_string(),
                ));
            }
            TransactionAction::RenameGroup
            | TransactionAction::AddDescriptiveColumn
            | TransactionAction::ReplaceImport
            | TransactionAction::AppendImport => {
                return Err(StoreError::Invariant(format!(
                    "{action:?} is not a unit transfer"
                )));
            }
        };

        if let Some(dest) = &dest_data {
            inputs.push(Self::input_of(dest_path, dest));
        }

        let outputs: Vec<PlannedOutput> = [source_out, dest_out].into_iter().flatten().collect();
        let tx = Transaction {
            action,
            inputs,
            outputs,
            delete_paths,
            selected_uuids: selected,
        };
        let transaction_id = self.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: match req.action {
                TransferAction::Move => "move_units".to_string(),
                TransferAction::Copy => "copy_units".to_string(),
            },
            outputs: tx
                .outputs
                .iter()
                .map(|out| Self::summary_of_output(&out.path, out))
                .collect(),
            deleted_paths: tx.delete_paths.clone(),
        })
    }

    /// Merges whole groups into the first source (the target) through one
    /// journaled transaction; the other sources cease to exist.
    pub fn merge_groups(
        &self,
        req: &MergeGroupsRequest,
    ) -> Result<TransactionResponse, StoreError> {
        if req.sources.len() < 2 {
            return Err(StoreError::Invariant(
                "merge requires at least two groups".to_string(),
            ));
        }
        let mut sources: Vec<(String, GroupFileData)> = Vec::new();
        let mut inputs = Vec::new();
        let snapshot = self.store.read_groups_snapshot(&req.sources)?;
        for (path, data) in req.sources.iter().zip(snapshot) {
            let data = data.ok_or_else(|| StoreError::Io(format!("group not found: {path}")))?;
            inputs.push(Self::input_of(path, &data));
            sources.push((path.clone(), data));
        }
        let target_revision = Self::next_revision(&sources[0].1.profile.revision_id);
        let group_id = sanitize_group_name(&req.new_group_name);
        let out = plan_merge(
            &sources,
            0,
            &group_id,
            &req.new_group_name,
            &target_revision,
        )?;
        let delete_paths: Vec<String> = req.sources[1..].to_vec();
        let tx = Transaction {
            action: TransactionAction::MergeGroups,
            inputs,
            outputs: vec![out.clone()],
            delete_paths,
            selected_uuids: out.rows.iter().map(|r| r.uuid).collect(),
        };
        let transaction_id = self.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "merge_groups".to_string(),
            outputs: vec![Self::summary_of_output(&out.path, &out)],
            deleted_paths: req.sources[1..].to_vec(),
        })
    }

    /// Deletes one group file through a journaled transaction (Section 10.2
    /// `DELETE /groups/{id}`, local Phase-2 form). Requires exact-path
    /// confirmation and the revision the caller last read; the deleted file
    /// is archived under the bounded history before removal, so recovery can
    /// restore it if the transaction never committed.
    pub fn delete_group(
        &self,
        req: &DeleteGroupRequest,
    ) -> Result<TransactionResponse, StoreError> {
        if req.confirm_path != req.path {
            return Err(StoreError::Invariant(
                "confirm_path must exactly equal the path being deleted".to_string(),
            ));
        }
        let data = self.store.read_group(&req.path)?;
        if data.profile.revision_id != req.expected_revision {
            return Err(StoreError::RevisionConflict {
                path: req.path.clone(),
                expected: req.expected_revision.clone(),
                found: data.profile.revision_id.clone(),
            });
        }
        let tx = Transaction {
            action: TransactionAction::DeleteGroup,
            inputs: vec![Self::input_of(&req.path, &data)],
            outputs: Vec::new(),
            delete_paths: vec![req.path.clone()],
            selected_uuids: Vec::new(),
        };
        let transaction_id = self.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "delete_group".to_string(),
            outputs: Vec::new(),
            deleted_paths: vec![req.path.clone()],
        })
    }

    /// Batch hidden-UUID-addressed descriptive edits through one journaled
    /// transaction (Section 10.2 `PATCH /groups/{id}/descriptive-values`).
    /// Elemental and identity columns are locked: only `roles.descriptive`
    /// columns accept edits, and the measured-elemental checksum precondition
    /// fails the transaction if the elemental values moved underneath.
    pub fn patch_descriptive_values(
        &self,
        req: &PatchDescriptiveValuesRequest,
    ) -> Result<TransactionResponse, StoreError> {
        if req.edits.is_empty() {
            return Err(StoreError::Invariant(
                "descriptive edit batch is empty".to_string(),
            ));
        }
        let data = self.store.read_group(&req.path)?;
        if data.profile.revision_id != req.expected_revision {
            return Err(StoreError::RevisionConflict {
                path: req.path.clone(),
                expected: req.expected_revision.clone(),
                found: data.profile.revision_id.clone(),
            });
        }
        let roles = &data.profile.roles;

        // Column locks: elemental values are never edited, and the hidden
        // identity/visible-id/legacy-rowid columns are not descriptive data.
        let mut column_indices = Vec::with_capacity(req.edits.len());
        for edit in &req.edits {
            let idx = roles
                .descriptive
                .iter()
                .position(|c| c == &edit.column)
                .ok_or_else(|| {
                    StoreError::Invariant(format!(
                        "column {:?} is not a descriptive column and cannot be edited",
                        edit.column
                    ))
                })?;
            column_indices.push(idx);
        }

        // Resolve every edit target up front so a bad UUID or unknown column
        // aborts before any write.
        let mut row_by_uuid: std::collections::HashMap<Uuid, usize> =
            std::collections::HashMap::new();
        for (index, row) in data.rows.iter().enumerate() {
            row_by_uuid.insert(row.uuid, index);
        }
        let mut applied: Vec<(usize, usize, Option<String>)> = Vec::with_capacity(req.edits.len());
        let mut edited_uuids: Vec<Uuid> = Vec::with_capacity(req.edits.len());
        for (edit, column_index) in req.edits.iter().zip(&column_indices) {
            let uuid = Self::parse_uuids(std::slice::from_ref(&edit.analytical_uuid))?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    StoreError::Invariant(
                        "parse_uuids returned no uuid for the edit target".to_string(),
                    )
                })?;
            let row_index = *row_by_uuid.get(&uuid).ok_or_else(|| {
                StoreError::Invariant(format!(
                    "analytical uuid {:?} is not present in {}",
                    edit.analytical_uuid, req.path
                ))
            })?;
            applied.push((row_index, *column_index, edit.value.clone()));
            edited_uuids.push(uuid);
        }

        let mut rows = data.rows.clone();
        for (row_index, column_index, value) in applied {
            rows[row_index].descriptive[column_index] = value;
        }

        let out = PlannedOutput {
            path: req.path.clone(),
            group_id: data.profile.group_id.clone(),
            group_name: data.profile.group_name.clone(),
            // Stamped fresh by the executor when publishing the successor.
            revision_id: Self::next_revision(&data.profile.revision_id),
            rows,
            roles: data.profile.roles.clone(),
            recipe: data.profile.import_recipe.clone(),
            source_path: data.profile.source_path.clone(),
            source_sha256: data.profile.source_sha256.clone(),
        };
        let tx = Transaction {
            action: TransactionAction::PatchDescriptive,
            inputs: vec![Self::input_of(&req.path, &data)],
            outputs: vec![out],
            delete_paths: Vec::new(),
            selected_uuids: edited_uuids,
        };
        let transaction_id = self.execute(&tx)?;
        let updated = self.store.read_group(&req.path)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "patch_descriptive_values".to_string(),
            outputs: vec![Self::summary(&req.path, &updated.profile)],
            deleted_paths: Vec::new(),
        })
    }

    /// Duplicates one whole group to a new path through a journaled
    /// transaction (Section 10.2 `POST /groups/{id}/duplicate`). UUIDs and
    /// source lineage are preserved by default; `preserve_uuids = false`
    /// mints fresh UUIDv7 identities for every row.
    pub fn duplicate_group(
        &self,
        req: &DuplicateGroupRequest,
    ) -> Result<TransactionResponse, StoreError> {
        let source = self.store.read_group(&req.source_path)?;
        if source.profile.revision_id != req.expected_revision {
            return Err(StoreError::RevisionConflict {
                path: req.source_path.clone(),
                expected: req.expected_revision.clone(),
                found: source.profile.revision_id.clone(),
            });
        }
        let group_id = sanitize_group_name(&req.new_group_name);
        if group_id.is_empty() {
            return Err(StoreError::Invariant(
                "new_group_name sanitizes to an empty group id".to_string(),
            ));
        }
        let destination_path = match &req.destination_path {
            Some(path) => path.clone(),
            None => {
                let folder = folder_of(&req.source_path);
                if folder.is_empty() {
                    format!("{group_id}.parquet")
                } else {
                    format!("{folder}/{group_id}.parquet")
                }
            }
        };
        if destination_path == req.source_path {
            return Err(StoreError::Invariant(
                "duplicate destination must differ from the source".to_string(),
            ));
        }
        let dest_abs = self.store.resolve(&destination_path)?;
        if dest_abs.exists() {
            return Err(StoreError::Invariant(format!(
                "duplicate destination {destination_path} already exists"
            )));
        }

        let rows: Vec<GroupRow> = source
            .rows
            .iter()
            .map(|row| GroupRow {
                uuid: if req.preserve_uuids {
                    row.uuid
                } else {
                    Uuid::now_v7()
                },
                visible: row.visible.clone(),
                legacy_rowid: row.legacy_rowid.clone(),
                descriptive: row.descriptive.clone(),
                elemental: row.elemental.clone(),
            })
            .collect();
        let out = PlannedOutput {
            path: destination_path,
            group_id: group_id.clone(),
            group_name: req.new_group_name.clone(),
            revision_id: "rev-1".to_string(),
            rows,
            roles: source.profile.roles.clone(),
            recipe: source.profile.import_recipe.clone(),
            // Lineage: the duplicate keeps pointing at the original import
            // source, matching "preserve UUIDs and lineage by default".
            source_path: source.profile.source_path.clone(),
            source_sha256: source.profile.source_sha256.clone(),
        };
        let tx = Transaction {
            action: TransactionAction::DuplicateGroup,
            inputs: vec![Self::input_of(&req.source_path, &source)],
            outputs: vec![out.clone()],
            delete_paths: Vec::new(),
            selected_uuids: source.rows.iter().map(|r| r.uuid).collect(),
        };
        let transaction_id = self.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "duplicate_group".to_string(),
            outputs: vec![Self::summary_of_output(&out.path, &out)],
            deleted_paths: Vec::new(),
        })
    }

    /// Renames one group: its profile name and id, and its file name in the
    /// same folder, through one journaled transaction (rows, identities,
    /// and lineage are unchanged). Refused when the folder already has a
    /// group of that name or a file of the new name.
    pub fn rename_group(
        &self,
        req: &RenameGroupRequest,
    ) -> Result<TransactionResponse, StoreError> {
        let name = validate_group_name(&req.new_name)?;
        let name = name.as_str();
        let data = self.store.read_group(&req.path)?;
        if data.profile.revision_id != req.expected_revision {
            return Err(StoreError::RevisionConflict {
                path: req.path.clone(),
                expected: req.expected_revision.clone(),
                found: data.profile.revision_id.clone(),
            });
        }
        if data.profile.group_name == name {
            return Err(StoreError::Validation(format!(
                "the group is already named \u{201c}{name}\u{201d}"
            )));
        }
        let from = req.path.replace('\\', "/");
        let folder = folder_of(&from);
        // Only this folder can clash; scanning the whole project made each
        // rename read every group file's footer.
        let candidates = self.folder_candidates(&folder)?;
        let taken = candidates.iter().any(|c| {
            let path = c.path.replace('\\', "/");
            let group_name = c.group.as_ref().map(|g| g.group_name.as_str());
            path != from
                && folder_of(&path) == folder
                && group_name.is_some_and(|n| n.eq_ignore_ascii_case(name))
        });
        if taken {
            let shown = if folder.is_empty() { "/" } else { &folder };
            return Err(StoreError::Validation(format!(
                "folder \u{201c}{shown}\u{201d} already has a group named \u{201c}{name}\u{201d}"
            )));
        }
        let group_id = sanitize_group_name(name);
        let file = format!("{group_id}.parquet");
        let candidate = if folder.is_empty() {
            file.clone()
        } else {
            format!("{folder}/{file}")
        };
        // A new name that maps to the same file (or differs only in case,
        // which case-insensitive file systems treat as the same file) keeps
        // the current path.
        let destination = if candidate.eq_ignore_ascii_case(&from) {
            req.path.clone()
        } else {
            if self.store.resolve(&candidate)?.exists() {
                return Err(StoreError::Validation(format!(
                    "a group file named {file} already exists in that folder"
                )));
            }
            candidate
        };
        let moved = destination != req.path;
        let out = PlannedOutput {
            path: destination,
            group_id,
            group_name: name.to_string(),
            revision_id: Self::next_revision(&data.profile.revision_id),
            rows: data.rows.clone(),
            roles: data.profile.roles.clone(),
            recipe: data.profile.import_recipe.clone(),
            source_path: data.profile.source_path.clone(),
            source_sha256: data.profile.source_sha256.clone(),
        };
        let deleted_paths = if moved {
            vec![req.path.clone()]
        } else {
            Vec::new()
        };
        let tx = Transaction {
            action: TransactionAction::RenameGroup,
            inputs: vec![Self::input_of(&req.path, &data)],
            outputs: vec![out.clone()],
            delete_paths: deleted_paths.clone(),
            selected_uuids: data.rows.iter().map(|r| r.uuid).collect(),
        };
        let transaction_id = self.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "rename_group".to_string(),
            outputs: vec![Self::summary_of_output(&out.path, &out)],
            deleted_paths,
        })
    }

    /// Adds a descriptive column with one value to every group file in a
    /// folder through one journaled transaction (legacy "Add column",
    /// `apply_new_column`). Measured, ID, and identity column names are
    /// refused; a column that already exists is refused unless `overwrite`
    /// is set, which replaces its values. Measured values never change.
    pub fn add_descriptive_column(
        &self,
        req: &AddDescriptiveColumnRequest,
    ) -> Result<TransactionResponse, StoreError> {
        let column = req.column.trim();
        if column.is_empty() {
            return Err(StoreError::Validation("column name is empty".to_string()));
        }
        let folder = req.folder.replace('\\', "/").trim_matches('/').to_string();
        let shown = if folder.is_empty() {
            "the project root".to_string()
        } else {
            format!("\u{201c}{folder}\u{201d}")
        };
        let reserved = [
            archaeodash_data_io::IDENTITY_COLUMN,
            archaeodash_data_io::LEGACY_ROWID_COLUMN,
            "rowid",
        ];
        if reserved.iter().any(|r| r.eq_ignore_ascii_case(column)) {
            return Err(StoreError::Validation(format!(
                "\u{201c}{column}\u{201d} is reserved for sample identity; choose another name"
            )));
        }
        let paths: Vec<String> = self
            .scan_candidates()?
            .into_iter()
            .filter(|c| c.ready && folder_of(&c.path) == folder)
            .map(|c| c.path)
            .collect();
        if paths.is_empty() {
            return Err(StoreError::Validation(format!(
                "folder {shown} has no groups"
            )));
        }
        let snapshot = self.store.read_groups_snapshot(&paths)?;
        let mut groups = Vec::with_capacity(paths.len());
        for (path, data) in paths.iter().zip(snapshot) {
            let data = data.ok_or_else(|| {
                StoreError::Validation(format!("group file {path} no longer exists"))
            })?;
            let roles = &data.profile.roles;
            if roles.elemental.iter().any(|c| c == column) {
                return Err(StoreError::Validation(format!(
                    "\u{201c}{column}\u{201d} is a measured column in \u{201c}{}\u{201d}; \
                     measured values cannot be replaced",
                    data.profile.group_name
                )));
            }
            if roles.visible_id == column {
                return Err(StoreError::Validation(format!(
                    "\u{201c}{column}\u{201d} is the ID column of \u{201c}{}\u{201d}",
                    data.profile.group_name
                )));
            }
            groups.push((path.clone(), data));
        }
        let existing = groups
            .iter()
            .filter(|(_, d)| d.profile.roles.descriptive.iter().any(|c| c == column))
            .count();
        if existing > 0 && !req.overwrite {
            return Err(StoreError::Validation(format!(
                "column \u{201c}{column}\u{201d} already exists in {existing} of {} groups in \
                 folder {shown}; confirm to overwrite its values",
                groups.len()
            )));
        }

        let mut inputs = Vec::with_capacity(groups.len());
        let mut outputs = Vec::with_capacity(groups.len());
        for (path, data) in &groups {
            let mut roles = data.profile.roles.clone();
            let index = match roles.descriptive.iter().position(|c| c == column) {
                Some(index) => index,
                None => {
                    roles.descriptive.push(column.to_string());
                    roles.descriptive.len() - 1
                }
            };
            let rows = data
                .rows
                .iter()
                .cloned()
                .map(|mut row| {
                    row.descriptive.resize(roles.descriptive.len(), None);
                    row.descriptive[index] = req.value.clone();
                    row
                })
                .collect();
            inputs.push(Self::input_of(path, data));
            outputs.push(PlannedOutput {
                path: path.clone(),
                group_id: data.profile.group_id.clone(),
                group_name: data.profile.group_name.clone(),
                revision_id: Self::next_revision(&data.profile.revision_id),
                rows,
                roles,
                recipe: data.profile.import_recipe.clone(),
                source_path: data.profile.source_path.clone(),
                source_sha256: data.profile.source_sha256.clone(),
            });
        }
        let tx = Transaction {
            action: TransactionAction::AddDescriptiveColumn,
            inputs,
            outputs,
            delete_paths: Vec::new(),
            selected_uuids: Vec::new(),
        };
        let transaction_id = self.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "add_descriptive_column".to_string(),
            outputs: tx
                .outputs
                .iter()
                .map(|out| Self::summary_of_output(&out.path, out))
                .collect(),
            deleted_paths: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use crate::import::ImportService;
    use archaeodash_contracts::{
        BatchTransferTarget, BatchTransferUnitsRequest, CopyGroupsRequest, CreateFolderRequest,
        DeleteFolderRequest, DescriptiveEdit, DuplicateGroupRequest, ImportCommitRequest,
        PatchDescriptiveValuesRequest, RenameFolderRequest,
    };
    use archaeodash_data_io::{read_group_file, write_group_rows};

    fn commit_two_groups(root: &std::path::Path) -> (GroupService, Vec<String>) {
        std::fs::write(
            root.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,5,6\n",
        )
        .expect("write source");
        let import = ImportService::new(root).expect("import service");
        let resp = import
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "mini.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        let paths: Vec<String> = resp.groups.iter().map(|g| g.path.clone()).collect();
        (GroupService::new(root).expect("group service"), paths)
    }

    #[test]
    fn scan_lists_ready_candidates_and_validate_returns_summary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        assert_eq!(paths.len(), 2);

        let candidates = service.scan_candidates().expect("scan");
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|c| c.ready));
        let summary = service.validate("groups/Baca.parquet").expect("validate");
        assert_eq!(summary.group_name, "Baca");
        assert_eq!(summary.row_count, 2);
        assert_eq!(summary.elemental_columns, vec!["as", "fe"]);
        assert_eq!(summary.revision_id, "rev-1");
    }

    #[test]
    fn move_units_transfers_and_bumps_revisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());

        let baca = read_group_file(&dir.path().join(&paths[0])).expect("read baca");
        let hooper = read_group_file(&dir.path().join(&paths[1])).expect("read hooper");
        let uuid = baca.rows[0].uuid.to_string();
        let resp = service
            .transfer_units(&TransferUnitsRequest {
                action: TransferAction::Move,
                source_path: paths[0].clone(),
                destination_path: paths[1].clone(),
                destination_group_name: None,
                selected_uuids: vec![uuid.clone()],
                expected_source_revision: baca.profile.revision_id.clone(),
            })
            .expect("move");
        assert_eq!(resp.action, "move_units");
        assert_eq!(resp.deleted_paths, Vec::<String>::new());

        let src = read_group_file(&dir.path().join(&paths[0])).expect("src");
        let dst = read_group_file(&dir.path().join(&paths[1])).expect("dst");
        assert_eq!(src.rows.len(), 1, "source keeps its remaining unit");
        assert_eq!(dst.rows.len(), 2, "destination gained the moved unit");
        assert_eq!(dst.profile.revision_id, "rev-2");
        assert_eq!(src.profile.revision_id, "rev-2");
        // All three identities preserved across both files, no duplicates.
        let mut uuids: Vec<String> = src
            .rows
            .iter()
            .chain(dst.rows.iter())
            .map(|r| r.uuid.to_string())
            .collect();
        uuids.sort();
        let mut expected: Vec<String> = baca
            .rows
            .iter()
            .chain(hooper.rows.iter())
            .map(|r| r.uuid.to_string())
            .collect();
        expected.sort();
        assert_eq!(uuids, expected);
    }

    #[test]
    fn move_all_units_deletes_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let baca = read_group_file(&dir.path().join(&paths[0])).expect("read baca");
        let uuids: Vec<String> = baca.rows.iter().map(|r| r.uuid.to_string()).collect();
        let resp = service
            .transfer_units(&TransferUnitsRequest {
                action: TransferAction::Move,
                source_path: paths[0].clone(),
                destination_path: paths[1].clone(),
                destination_group_name: None,
                selected_uuids: uuids,
                expected_source_revision: baca.profile.revision_id.clone(),
            })
            .expect("move");
        assert_eq!(resp.deleted_paths, vec![paths[0].clone()]);
        assert!(
            !dir.path().join(&paths[0]).exists(),
            "emptied source deleted"
        );
    }

    #[test]
    fn batch_move_mixes_existing_and_new_destinations_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let source = read_group_file(&dir.path().join(&paths[0])).expect("source");
        let existing = read_group_file(&dir.path().join(&paths[1])).expect("existing");
        let a = source.rows[0].uuid.to_string();
        let b = source.rows[1].uuid.to_string();
        let response = service
            .batch_transfer_units(&BatchTransferUnitsRequest {
                source_path: paths[0].clone(),
                expected_source_revision: source.profile.revision_id,
                targets: vec![
                    BatchTransferTarget {
                        destination_path: paths[1].clone(),
                        destination_group_name: None,
                        expected_destination_revision: Some(existing.profile.revision_id),
                        selected_uuids: vec![a.clone()],
                    },
                    BatchTransferTarget {
                        destination_path: "groups/New.parquet".into(),
                        destination_group_name: Some("New".into()),
                        expected_destination_revision: None,
                        selected_uuids: vec![b.clone()],
                    },
                ],
            })
            .expect("atomic batch move");
        assert_eq!(response.outputs.len(), 2);
        assert!(!dir.path().join(&paths[0]).exists());
        let moved_existing =
            read_group_file(&dir.path().join(&paths[1])).expect("existing successor");
        let moved_new =
            read_group_file(&dir.path().join("groups/New.parquet")).expect("new successor");
        assert!(moved_existing.rows.iter().any(|r| r.uuid.to_string() == a));
        assert!(moved_new.rows.iter().any(|r| r.uuid.to_string() == b));
    }

    #[test]
    fn batch_move_late_stale_destination_writes_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let source = read_group_file(&dir.path().join(&paths[0])).expect("source");
        let existing = read_group_file(&dir.path().join(&paths[1])).expect("existing");
        std::fs::copy(
            dir.path().join(&paths[1]),
            dir.path().join("groups/Third.parquet"),
        )
        .expect("copy third destination");
        let before_source = std::fs::read(dir.path().join(&paths[0])).expect("source bytes");
        let before_dest = std::fs::read(dir.path().join(&paths[1])).expect("dest bytes");
        let before_third =
            std::fs::read(dir.path().join("groups/Third.parquet")).expect("third bytes");
        let err = service
            .batch_transfer_units(&BatchTransferUnitsRequest {
                source_path: paths[0].clone(),
                expected_source_revision: source.profile.revision_id,
                targets: vec![
                    BatchTransferTarget {
                        destination_path: paths[1].clone(),
                        destination_group_name: None,
                        expected_destination_revision: Some(existing.profile.revision_id),
                        selected_uuids: vec![source.rows[0].uuid.to_string()],
                    },
                    BatchTransferTarget {
                        destination_path: "groups/Third.parquet".into(),
                        destination_group_name: None,
                        expected_destination_revision: Some("stale".into()),
                        selected_uuids: vec![source.rows[1].uuid.to_string()],
                    },
                ],
            })
            .expect_err("duplicate path must reject preflight");
        assert!(matches!(err, StoreError::RevisionConflict { .. }));
        assert_eq!(
            std::fs::read(dir.path().join(&paths[0])).expect("source unchanged"),
            before_source
        );
        assert_eq!(
            std::fs::read(dir.path().join(&paths[1])).expect("dest unchanged"),
            before_dest
        );
        assert_eq!(
            std::fs::read(dir.path().join("groups/Third.parquet")).expect("third unchanged"),
            before_third
        );
    }

    #[test]
    fn batch_move_rejects_duplicate_and_unknown_uuids() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let source = read_group_file(&dir.path().join(&paths[0])).expect("source");
        let duplicate = source.rows[0].uuid.to_string();
        let req = |one: String, two: String| BatchTransferUnitsRequest {
            source_path: paths[0].clone(),
            expected_source_revision: source.profile.revision_id.clone(),
            targets: vec![
                BatchTransferTarget {
                    destination_path: "groups/One.parquet".into(),
                    destination_group_name: Some("One".into()),
                    expected_destination_revision: None,
                    selected_uuids: vec![one],
                },
                BatchTransferTarget {
                    destination_path: "groups/Two.parquet".into(),
                    destination_group_name: Some("Two".into()),
                    expected_destination_revision: None,
                    selected_uuids: vec![two],
                },
            ],
        };
        assert!(matches!(
            service.batch_transfer_units(&req(duplicate.clone(), duplicate)),
            Err(StoreError::Invariant(_))
        ));
        assert!(matches!(
            service.batch_transfer_units(&req(
                Uuid::now_v7().to_string(),
                source.rows[1].uuid.to_string()
            )),
            Err(StoreError::Invariant(_))
        ));
        assert!(!dir.path().join("groups/One.parquet").exists());
        assert!(!dir.path().join("groups/Two.parquet").exists());
    }

    #[cfg(unix)]
    #[test]
    fn batch_move_rejects_hard_link_destination_aliases() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let source = read_group_file(&dir.path().join(&paths[0])).expect("source");
        std::fs::hard_link(
            dir.path().join(&paths[0]),
            dir.path().join("groups/SourceAlias.parquet"),
        )
        .expect("hard link alias");
        let err = service
            .batch_transfer_units(&BatchTransferUnitsRequest {
                source_path: paths[0].clone(),
                expected_source_revision: source.profile.revision_id,
                targets: vec![BatchTransferTarget {
                    destination_path: "groups/SourceAlias.parquet".into(),
                    destination_group_name: None,
                    expected_destination_revision: Some("rev-1".into()),
                    selected_uuids: vec![source.rows[0].uuid.to_string()],
                }],
            })
            .expect_err("hard link alias rejected");
        assert!(matches!(err, StoreError::Invariant(_)));
    }

    #[test]
    fn batch_move_rejects_schema_mismatch_before_any_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let source = read_group_file(&dir.path().join(&paths[0])).expect("source");
        let destination = read_group_file(&dir.path().join(&paths[1])).expect("destination");
        let mut mismatched_profile = destination.profile.clone();
        mismatched_profile.roles.elemental = vec!["zn".into()];
        let mut rows = destination.rows.clone();
        for row in &mut rows {
            row.elemental = vec![Some(9.0)];
        }
        write_group_rows(&dir.path().join(&paths[1]), mismatched_profile, &rows)
            .expect("rewrite schema");
        let mismatch =
            read_group_file(&dir.path().join(&paths[1])).expect("valid mismatched group");
        let before_source = std::fs::read(dir.path().join(&paths[0])).expect("source bytes");
        let before_destination =
            std::fs::read(dir.path().join(&paths[1])).expect("destination bytes");
        let err = service
            .batch_transfer_units(&BatchTransferUnitsRequest {
                source_path: paths[0].clone(),
                expected_source_revision: source.profile.revision_id,
                targets: vec![BatchTransferTarget {
                    destination_path: paths[1].clone(),
                    destination_group_name: None,
                    expected_destination_revision: Some(mismatch.profile.revision_id),
                    selected_uuids: vec![source.rows[0].uuid.to_string()],
                }],
            })
            .expect_err("schema mismatch");
        assert!(matches!(err, StoreError::SchemaMismatch { .. }));
        assert_eq!(
            std::fs::read(dir.path().join(&paths[0])).expect("source unchanged"),
            before_source
        );
        assert_eq!(
            std::fs::read(dir.path().join(&paths[1])).expect("destination unchanged"),
            before_destination
        );
    }

    #[test]
    fn stale_revision_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let err = service
            .transfer_units(&TransferUnitsRequest {
                action: TransferAction::Move,
                source_path: paths[0].clone(),
                destination_path: paths[1].clone(),
                destination_group_name: None,
                selected_uuids: vec![Uuid::now_v7().to_string()],
                expected_source_revision: "rev-999".into(),
            })
            .expect_err("stale revision rejected");
        assert!(matches!(err, StoreError::RevisionConflict { .. }));
    }

    #[test]
    fn copy_units_keeps_uuid_in_both_groups() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let baca = read_group_file(&dir.path().join(&paths[0])).expect("read baca");
        let uuid = baca.rows[0].uuid.to_string();
        service
            .transfer_units(&TransferUnitsRequest {
                action: TransferAction::Copy,
                source_path: paths[0].clone(),
                destination_path: "groups/copy-target.parquet".into(),
                destination_group_name: Some("Copy Target".into()),
                selected_uuids: vec![uuid.clone()],
                expected_source_revision: baca.profile.revision_id.clone(),
            })
            .expect_err("copying units within one folder repeats samples");
        service
            .transfer_units(&TransferUnitsRequest {
                action: TransferAction::Copy,
                source_path: paths[0].clone(),
                destination_path: "subsets/copy-target.parquet".into(),
                destination_group_name: Some("Copy Target".into()),
                selected_uuids: vec![uuid.clone()],
                expected_source_revision: baca.profile.revision_id.clone(),
            })
            .expect("copy into another folder");
        let dst =
            read_group_file(&dir.path().join("subsets/copy-target.parquet")).expect("new group");
        assert_eq!(dst.profile.group_id, "Copy_Target");
        assert_eq!(dst.rows.len(), 1);
        assert_eq!(dst.rows[0].uuid.to_string(), uuid);
        // Source unchanged: same revision, still two rows.
        let src = read_group_file(&dir.path().join(&paths[0])).expect("src");
        assert_eq!(src.profile.revision_id, "rev-1");
        assert_eq!(src.rows.len(), 2);
    }

    #[test]
    fn merge_groups_unifies_and_removes_sources() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let resp = service
            .merge_groups(&MergeGroupsRequest {
                sources: paths.clone(),
                new_group_name: "Merged".into(),
            })
            .expect("merge");
        assert_eq!(resp.deleted_paths, vec![paths[1].clone()]);
        let merged = read_group_file(&dir.path().join(&paths[0])).expect("merged");
        assert_eq!(merged.rows.len(), 3);
        assert_eq!(merged.profile.group_name, "Merged");
        assert!(!dir.path().join(&paths[1]).exists(), "source removed");
    }

    #[test]
    fn delete_group_requires_confirmation_and_current_revision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let baca = read_group_file(&dir.path().join(&paths[0])).expect("read baca");

        // Exact-path confirmation is mandatory (Section 10.4).
        let err = service
            .delete_group(&DeleteGroupRequest {
                path: paths[0].clone(),
                expected_revision: baca.profile.revision_id.clone(),
                confirm_path: "groups/other.parquet".into(),
            })
            .expect_err("mismatched confirmation rejected");
        assert!(matches!(err, StoreError::Invariant(_)));
        assert!(dir.path().join(&paths[0]).exists(), "nothing deleted");

        // Stale revision conflicts before any file is touched.
        let err = service
            .delete_group(&DeleteGroupRequest {
                path: paths[0].clone(),
                expected_revision: "rev-999".into(),
                confirm_path: paths[0].clone(),
            })
            .expect_err("stale revision rejected");
        assert!(matches!(err, StoreError::RevisionConflict { .. }));
        assert!(dir.path().join(&paths[0]).exists(), "nothing deleted");
    }

    #[test]
    fn delete_group_removes_file_and_archives_original() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let baca = read_group_file(&dir.path().join(&paths[0])).expect("read baca");

        let resp = service
            .delete_group(&DeleteGroupRequest {
                path: paths[0].clone(),
                expected_revision: baca.profile.revision_id.clone(),
                confirm_path: paths[0].clone(),
            })
            .expect("delete");
        assert_eq!(resp.action, "delete_group");
        assert_eq!(resp.outputs, Vec::new());
        assert_eq!(resp.deleted_paths, vec![paths[0].clone()]);
        assert!(!dir.path().join(&paths[0]).exists(), "file removed");
        // The other group is untouched and no transaction journal remains.
        assert!(dir.path().join(&paths[1]).exists());
        let tx_root = dir.path().join(archaeodash_file_store_fs::TRANSACTIONS_DIR);
        assert_eq!(
            std::fs::read_dir(&tx_root)
                .expect("transactions root")
                .filter_map(|e| e.ok())
                .count(),
            0,
            "no journal entries remain"
        );
        // The deleted original is archived under the bounded history.
        let history = dir.path().join(archaeodash_file_store_fs::HISTORY_DIR);
        let archived: usize = std::fs::read_dir(&history)
            .expect("history root")
            .filter_map(|e| e.ok())
            .filter_map(|e| std::fs::read_dir(e.path()).ok())
            .flat_map(|entries| entries.filter_map(|e| e.ok()))
            .count();
        assert_eq!(archived, 1, "deleted original archived");
        // The scan no longer lists the deleted group.
        let candidates = service.scan_candidates().expect("scan");
        assert!(!candidates.iter().any(|c| c.path == paths[0]));
    }

    #[test]
    fn patch_descriptive_values_edits_by_uuid_and_bumps_revision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let before = read_group_file(&dir.path().join(&paths[0])).expect("read before");
        let uuid = before.rows[0].uuid.to_string();
        let original_elemental = before.rows[0].elemental.clone();

        let resp = service
            .patch_descriptive_values(&PatchDescriptiveValuesRequest {
                path: paths[0].clone(),
                expected_revision: before.profile.revision_id.clone(),
                edits: vec![DescriptiveEdit {
                    analytical_uuid: uuid.clone(),
                    column: "Site".into(),
                    value: Some("Zed".into()),
                }],
            })
            .expect("patch");
        assert_eq!(resp.action, "patch_descriptive_values");
        assert_eq!(resp.deleted_paths, Vec::<String>::new());
        assert_eq!(resp.outputs.len(), 1);
        assert_eq!(resp.outputs[0].revision_id, "rev-2");
        assert_eq!(resp.outputs[0].group_name, "Baca");

        let after = read_group_file(&dir.path().join(&paths[0])).expect("read after");
        assert_eq!(after.rows[0].descriptive[0].as_deref(), Some("Zed"));
        assert_eq!(
            after.rows[0].elemental, original_elemental,
            "elemental locked"
        );
        assert_eq!(after.profile.revision_id, "rev-2");
        assert_eq!(after.rows[0].uuid, before.rows[0].uuid);

        // Back-to-back saves of same-length values (same file size, possibly
        // the same coarse timestamp) are still seen: the read cache keys on
        // the Parquet footer, which carries the new revision.
        service
            .patch_descriptive_values(&PatchDescriptiveValuesRequest {
                path: paths[0].clone(),
                expected_revision: "rev-2".into(),
                edits: vec![DescriptiveEdit {
                    analytical_uuid: uuid.clone(),
                    column: "Site".into(),
                    value: Some("Zee".into()),
                }],
            })
            .expect("second patch");
        let again = read_group_file(&dir.path().join(&paths[0])).expect("read again");
        assert_eq!(again.rows[0].descriptive[0].as_deref(), Some("Zee"));
        assert_eq!(again.profile.revision_id, "rev-3");
        let rows = service.rows(&paths[0]).expect("rows");
        assert_eq!(rows.revision_id, "rev-3");
        assert_eq!(rows.rows[0].descriptive[0].as_deref(), Some("Zee"));

        // Elemental columns are locked.
        let elemental_edit = PatchDescriptiveValuesRequest {
            path: paths[0].clone(),
            expected_revision: "rev-3".into(),
            edits: vec![DescriptiveEdit {
                analytical_uuid: uuid.clone(),
                column: "as".into(),
                value: Some("9".into()),
            }],
        };
        let err = service
            .patch_descriptive_values(&elemental_edit)
            .unwrap_err();
        assert!(err.to_string().contains("not a descriptive column"));

        // Unknown analytical uuid aborts.
        let unknown = PatchDescriptiveValuesRequest {
            path: paths[0].clone(),
            expected_revision: "rev-3".into(),
            edits: vec![DescriptiveEdit {
                analytical_uuid: "01900000-0000-7000-8000-00000000000f".into(),
                column: "Site".into(),
                value: None,
            }],
        };
        let err = service.patch_descriptive_values(&unknown).unwrap_err();
        assert!(err.to_string().contains("not present"));

        // Stale expected revision is a conflict.
        let stale = PatchDescriptiveValuesRequest {
            path: paths[0].clone(),
            expected_revision: "rev-1".into(),
            edits: vec![DescriptiveEdit {
                analytical_uuid: uuid,
                column: "Site".into(),
                value: Some("Zed".into()),
            }],
        };
        let err = service.patch_descriptive_values(&stale).unwrap_err();
        assert!(err.to_string().contains("revision"), "{err}");
    }

    #[test]
    fn folders_create_rename_copy_move_and_refuse_repeated_samples() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let rev = |p: &str| {
            read_group_file(&dir.path().join(p))
                .expect("read")
                .profile
                .revision_id
        };
        let refs = |ps: &[String]| -> Vec<archaeodash_contracts::GroupRevisionRef> {
            ps.iter()
                .map(|p| archaeodash_contracts::GroupRevisionRef {
                    path: p.clone(),
                    expected_revision: rev(p),
                })
                .collect()
        };

        // Names are checked; an empty folder is listed through its marker.
        assert!(service
            .create_folder(&CreateFolderRequest { name: "a/b".into() })
            .is_err());
        assert!(service
            .create_folder(&CreateFolderRequest {
                name: "sources".into()
            })
            .is_err());
        assert!(service
            .create_folder(&CreateFolderRequest {
                name: ".hidden".into()
            })
            .is_err());
        let made = service
            .create_folder(&CreateFolderRequest {
                name: " Clusters ".into(),
            })
            .expect("create");
        assert_eq!(made.path, "Clusters");
        assert!(service
            .create_folder(&CreateFolderRequest {
                name: "Clusters".into()
            })
            .is_err());
        let folders = service.list_folders().expect("list");
        assert_eq!(
            folders
                .iter()
                .map(|f| (f.path.as_str(), f.group_count))
                .collect::<Vec<_>>(),
            [("Clusters", 0), ("groups", 2)]
        );

        // Copying into the same folder would repeat every sample: refused.
        let err = service
            .copy_groups(&CopyGroupsRequest {
                sources: refs(&paths[..1]),
                destination_folder: "groups".into(),
                remove_sources: false,
            })
            .expect_err("same folder");
        assert!(
            err.to_string().contains("already has a group file")
                || err.to_string().contains("same samples twice"),
            "{err}"
        );

        // Copy both groups out; identities are kept.
        let copied = service
            .copy_groups(&CopyGroupsRequest {
                sources: refs(&paths),
                destination_folder: "Clusters".into(),
                remove_sources: false,
            })
            .expect("copy");
        assert_eq!(copied.action, "copy_groups");
        assert_eq!(copied.outputs.len(), 2);
        let original = read_group_file(&dir.path().join(&paths[0])).expect("original");
        let copy = read_group_file(&dir.path().join(&copied.outputs[0].path)).expect("copy");
        assert_eq!(
            original.rows.iter().map(|r| r.uuid).collect::<Vec<_>>(),
            copy.rows.iter().map(|r| r.uuid).collect::<Vec<_>>()
        );

        // Copying a group back into a folder that already has its samples is
        // refused even under a different file name.
        std::fs::create_dir_all(dir.path().join("other")).expect("dir");
        std::fs::copy(
            dir.path().join(&paths[0]),
            dir.path().join("other/Renamed.parquet"),
        )
        .expect("copy file");
        let err = service
            .copy_groups(&CopyGroupsRequest {
                sources: refs(&["other/Renamed.parquet".to_string()]),
                destination_folder: "Clusters".into(),
                remove_sources: false,
            })
            .expect_err("repeated samples");
        assert!(err.to_string().contains("same samples twice"), "{err}");

        // Moving a group into a new folder removes it from the old one.
        let moved = service
            .copy_groups(&CopyGroupsRequest {
                sources: refs(&["other/Renamed.parquet".to_string()]),
                destination_folder: "Moved".into(),
                remove_sources: true,
            })
            .expect("move");
        assert_eq!(moved.deleted_paths, ["other/Renamed.parquet"]);
        assert!(dir.path().join("Moved/Renamed.parquet").exists());

        // Rename keeps the group files; the old name is gone.
        let renamed = service
            .rename_folder(&RenameFolderRequest {
                path: "Clusters".into(),
                new_name: "k-means 3".into(),
            })
            .expect("rename");
        assert_eq!(renamed.path, "k-means 3");
        assert_eq!(renamed.group_count, 2);
        assert!(!dir.path().join("Clusters").exists());
        assert!(service
            .rename_folder(&RenameFolderRequest {
                path: "k-means 3".into(),
                new_name: "groups".into()
            })
            .is_err());

        // Only empty folders can be deleted.
        assert!(service
            .delete_folder(&DeleteFolderRequest {
                path: "k-means 3".into()
            })
            .is_err());
        let empty = service
            .create_folder(&CreateFolderRequest {
                name: "Empty".into(),
            })
            .expect("create");
        service
            .delete_folder(&DeleteFolderRequest { path: empty.path })
            .expect("delete empty");
        assert!(!dir.path().join("Empty").exists());
    }

    #[test]
    fn apply_clusters_replaces_selected_groups_or_writes_a_new_folder() {
        use archaeodash_contracts::{ApplyClustersMode, ApplyClustersRequest, GroupRevisionRef};
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, _) = commit_two_groups(dir.path());
        let read = |p: &str| read_group_file(&dir.path().join(p)).expect("read");
        let reference = |p: &str| GroupRevisionRef {
            path: p.to_string(),
            expected_revision: read(p).profile.revision_id,
        };
        let uuids =
            |p: &str| -> Vec<String> { read(p).rows.iter().map(|r| r.uuid.to_string()).collect() };
        let names = |p: &str| -> Vec<String> {
            read(p)
                .rows
                .iter()
                .filter_map(|r| r.visible.clone())
                .collect()
        };
        let baca = uuids("groups/Baca.parquet");
        let hooper = uuids("groups/Hooper.parquet");

        // New folder: both groups clustered; the originals stay.
        let request = ApplyClustersRequest {
            sources: vec![
                reference("groups/Baca.parquet"),
                reference("groups/Hooper.parquet"),
            ],
            clusters: vec![
                vec![baca[0].clone(), hooper[0].clone()],
                vec![baca[1].clone()],
            ],
            mode: ApplyClustersMode::NewFolder,
            name_prefix: None,
        };
        let created = service.apply_clusters(&request).expect("new folder");
        let paths: Vec<&str> = created.outputs.iter().map(|o| o.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "groups clusters/Cluster_1.parquet",
                "groups clusters/Cluster_2.parquet"
            ]
        );
        assert!(created.deleted_paths.is_empty());
        assert_eq!(names("groups clusters/Cluster_1.parquet"), ["A1", "A3"]);
        assert_eq!(
            read("groups clusters/Cluster_2.parquet").profile.group_name,
            "Cluster 2"
        );
        assert_eq!(uuids("groups/Baca.parquet"), baca);
        // A second run picks the next free folder.
        let again = service.apply_clusters(&request).expect("second folder");
        assert!(again.outputs[0].path.starts_with("groups clusters 2/"));

        // Replace only Baca: its samples become Cluster 1 and Cluster 2 in
        // its folder; Hooper is untouched.
        let replaced = service
            .apply_clusters(&ApplyClustersRequest {
                sources: vec![reference("groups/Baca.parquet")],
                clusters: vec![vec![baca[0].clone()], vec![baca[1].clone()]],
                mode: ApplyClustersMode::Replace,
                name_prefix: None,
            })
            .expect("replace");
        assert_eq!(replaced.deleted_paths, ["groups/Baca.parquet"]);
        assert!(!dir.path().join("groups/Baca.parquet").exists());
        assert_eq!(names("groups/Cluster_1.parquet"), ["A1"]);
        assert_eq!(names("groups/Cluster_2.parquet"), ["A2"]);
        assert_eq!(uuids("groups/Hooper.parquet"), hooper);

        // Replacing Hooper alone: "Cluster 1" belongs to a group that stays,
        // so the new group gets a suffix.
        let suffixed = service
            .apply_clusters(&ApplyClustersRequest {
                sources: vec![reference("groups/Hooper.parquet")],
                clusters: vec![hooper.clone()],
                mode: ApplyClustersMode::Replace,
                name_prefix: None,
            })
            .expect("suffix");
        assert_eq!(suffixed.outputs[0].group_name, "Cluster 1 (2)");
        assert_eq!(suffixed.outputs[0].path, "groups/Cluster_1__2_.parquet");

        // Re-clustering the cluster groups overwrites a reused file in place.
        let rerun = service
            .apply_clusters(&ApplyClustersRequest {
                sources: vec![
                    reference("groups/Cluster_1.parquet"),
                    reference("groups/Cluster_2.parquet"),
                ],
                clusters: vec![baca.clone()],
                mode: ApplyClustersMode::Replace,
                name_prefix: None,
            })
            .expect("rerun");
        assert_eq!(rerun.outputs[0].path, "groups/Cluster_1.parquet");
        assert_eq!(rerun.outputs[0].revision_id, "rev-2");
        assert_eq!(rerun.deleted_paths, ["groups/Cluster_2.parquet"]);
        assert_eq!(names("groups/Cluster_1.parquet"), ["A1", "A2"]);

        // Refused: a stale revision, a sample left out, a sample twice.
        let current = reference("groups/Cluster_1.parquet");
        let stale = GroupRevisionRef {
            expected_revision: "rev-1".into(),
            ..current.clone()
        };
        let refuse = |sources: Vec<GroupRevisionRef>, clusters: Vec<Vec<String>>| {
            service
                .apply_clusters(&ApplyClustersRequest {
                    sources,
                    clusters,
                    mode: ApplyClustersMode::Replace,
                    name_prefix: None,
                })
                .expect_err("refused")
                .to_string()
        };
        assert!(matches!(
            service.apply_clusters(&ApplyClustersRequest {
                sources: vec![stale],
                clusters: vec![baca.clone()],
                mode: ApplyClustersMode::Replace,
                name_prefix: None,
            }),
            Err(StoreError::RevisionConflict { .. })
        ));
        assert!(
            refuse(vec![current.clone()], vec![vec![baca[0].clone()]]).contains("does not match")
        );
        assert!(refuse(
            vec![current.clone()],
            vec![baca.clone(), vec![baca[0].clone()]]
        )
        .contains("more than one cluster"));
        assert_eq!(names("groups/Cluster_1.parquet"), ["A1", "A2"]);
    }

    #[test]
    fn cluster_name_prefix_defaults_and_refuses_unsafe_names() {
        assert_eq!(cluster_name_prefix(None).unwrap(), "Cluster ");
        assert_eq!(cluster_name_prefix(Some("Cluster ")).unwrap(), "Cluster ");
        assert_eq!(
            cluster_name_prefix(Some("  Cluster 5-")).unwrap(),
            "Cluster 5-"
        );
        assert_eq!(cluster_name_prefix(Some("COM ")).unwrap(), "COM ");
        for (prefix, why) in [
            ("", "empty"),
            ("   ", "empty"),
            ("a/b ", "cannot contain"),
            ("a\\b ", "cannot contain"),
            ("k: ", "cannot contain"),
            ("why? ", "cannot contain"),
            ("tab\t", "cannot contain"),
            (".hidden ", "dot"),
            ("com", "Windows reserves"),
        ] {
            let error = cluster_name_prefix(Some(prefix)).expect_err(prefix);
            assert!(
                matches!(&error, StoreError::Validation(m) if m.contains(why)),
                "{prefix:?}: {error}"
            );
        }
        assert!(cluster_name_prefix(Some(&"x".repeat(101))).is_err());
    }

    #[test]
    fn apply_clusters_uses_the_name_prefix() {
        use archaeodash_contracts::{ApplyClustersMode, ApplyClustersRequest, GroupRevisionRef};
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, _) = commit_two_groups(dir.path());
        let read = |p: &str| read_group_file(&dir.path().join(p)).expect("read");
        let reference = |p: &str| GroupRevisionRef {
            path: p.to_string(),
            expected_revision: read(p).profile.revision_id,
        };
        let uuids =
            |p: &str| -> Vec<String> { read(p).rows.iter().map(|r| r.uuid.to_string()).collect() };
        let baca = uuids("groups/Baca.parquet");
        let hooper = uuids("groups/Hooper.parquet");
        let request = |sources: Vec<GroupRevisionRef>,
                       clusters: Vec<Vec<String>>,
                       mode: ApplyClustersMode,
                       prefix: &str| ApplyClustersRequest {
            sources,
            clusters,
            mode,
            name_prefix: Some(prefix.to_string()),
        };

        // A custom prefix names the groups and their files.
        let created = service
            .apply_clusters(&request(
                vec![reference("groups/Baca.parquet")],
                vec![vec![baca[0].clone()], vec![baca[1].clone()]],
                ApplyClustersMode::NewFolder,
                "Cluster 5-",
            ))
            .expect("custom prefix");
        let named: Vec<(&str, &str)> = created
            .outputs
            .iter()
            .map(|o| (o.group_name.as_str(), o.path.as_str()))
            .collect();
        assert_eq!(
            named,
            [
                ("Cluster 5-1", "groups clusters/Cluster_5-1.parquet"),
                ("Cluster 5-2", "groups clusters/Cluster_5-2.parquet"),
            ]
        );
        assert_eq!(
            read("groups clusters/Cluster_5-2.parquet")
                .profile
                .group_name,
            "Cluster 5-2"
        );

        // The default prefix, sent explicitly, matches an absent one.
        let default = service
            .apply_clusters(&request(
                vec![reference("groups/Baca.parquet")],
                vec![baca.clone()],
                ApplyClustersMode::NewFolder,
                "Cluster ",
            ))
            .expect("default prefix");
        assert_eq!(default.outputs[0].group_name, "Cluster 1");
        assert_eq!(
            default.outputs[0].path,
            "groups clusters 2/Cluster_1.parquet"
        );

        // Usual clash rule: a name kept by a group that stays gets a suffix.
        service
            .apply_clusters(&request(
                vec![reference("groups/Baca.parquet")],
                vec![baca.clone()],
                ApplyClustersMode::Replace,
                "Site ",
            ))
            .expect("replace Baca");
        let suffixed = service
            .apply_clusters(&request(
                vec![reference("groups/Hooper.parquet")],
                vec![hooper.clone()],
                ApplyClustersMode::Replace,
                "Site ",
            ))
            .expect("replace Hooper");
        assert_eq!(suffixed.outputs[0].group_name, "Site 1 (2)");

        // An invalid prefix is refused before anything is written.
        let refused = service
            .apply_clusters(&request(
                vec![reference("groups/Site_1.parquet")],
                vec![baca.clone()],
                ApplyClustersMode::NewFolder,
                "bad/name ",
            ))
            .expect_err("invalid prefix");
        assert!(matches!(refused, StoreError::Validation(m) if m.contains("cannot contain")));
        assert!(!dir.path().join("groups clusters 3").exists());
        assert_eq!(uuids("groups/Site_1.parquet"), baca);
    }

    #[test]
    fn duplicate_group_preserves_uuids_and_lineage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let source = read_group_file(&dir.path().join(&paths[0])).expect("read source");

        let resp = service
            .duplicate_group(&DuplicateGroupRequest {
                source_path: paths[0].clone(),
                expected_revision: source.profile.revision_id.clone(),
                new_group_name: "Baca Copy".into(),
                destination_path: None,
                preserve_uuids: true,
            })
            .expect_err("same-folder duplicate repeats samples");
        assert!(resp.to_string().contains("same samples twice"), "{resp}");

        let resp = service
            .duplicate_group(&DuplicateGroupRequest {
                source_path: paths[0].clone(),
                expected_revision: source.profile.revision_id.clone(),
                new_group_name: "Baca Copy".into(),
                destination_path: Some("groups/Baca_Copy.parquet".into()),
                preserve_uuids: false,
            })
            .expect("duplicate with fresh identities");
        assert_eq!(resp.outputs[0].path, "groups/Baca_Copy.parquet");
        std::fs::remove_file(dir.path().join("groups/Baca_Copy.parquet")).expect("cleanup");

        let resp = service
            .duplicate_group(&DuplicateGroupRequest {
                source_path: paths[0].clone(),
                expected_revision: source.profile.revision_id.clone(),
                new_group_name: "Baca Copy".into(),
                destination_path: Some("copies/Baca_Copy.parquet".into()),
                preserve_uuids: true,
            })
            .expect("duplicate into another folder");
        assert_eq!(resp.action, "duplicate_group");
        assert_eq!(resp.deleted_paths, Vec::<String>::new());
        assert_eq!(resp.outputs.len(), 1);
        assert_eq!(resp.outputs[0].path, "copies/Baca_Copy.parquet");
        assert_eq!(resp.outputs[0].group_name, "Baca Copy");
        assert_eq!(resp.outputs[0].revision_id, "rev-1");

        let copy =
            read_group_file(&dir.path().join("copies/Baca_Copy.parquet")).expect("read duplicate");
        assert_eq!(copy.rows.len(), source.rows.len());
        let source_uuids: Vec<_> = source.rows.iter().map(|r| r.uuid).collect();
        let copy_uuids: Vec<_> = copy.rows.iter().map(|r| r.uuid).collect();
        assert_eq!(source_uuids, copy_uuids, "UUIDs preserved by default");
        assert_eq!(copy.profile.source_path, source.profile.source_path);
        assert_eq!(copy.profile.source_sha256, source.profile.source_sha256);
        assert_eq!(copy.profile.group_id, "Baca_Copy");

        // The original file is untouched (new revision only on the copy).
        let reread = read_group_file(&dir.path().join(&paths[0])).expect("reread source");
        assert_eq!(reread.profile.revision_id, source.profile.revision_id);

        // Duplicate onto an existing destination is refused.
        let err = service
            .duplicate_group(&DuplicateGroupRequest {
                source_path: paths[0].clone(),
                expected_revision: source.profile.revision_id.clone(),
                new_group_name: "Hooper".into(),
                destination_path: None,
                preserve_uuids: false,
            })
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn rename_group_moves_the_file_and_keeps_identities() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let before = read_group_file(&dir.path().join(&paths[0])).expect("read");
        let response = service
            .rename_group(&RenameGroupRequest {
                path: paths[0].clone(),
                expected_revision: before.profile.revision_id.clone(),
                new_name: "Baca Pueblo".into(),
            })
            .expect("rename");
        assert_eq!(response.action, "rename_group");
        assert_eq!(response.outputs[0].path, "groups/Baca_Pueblo.parquet");
        assert_eq!(response.deleted_paths, vec![paths[0].clone()]);
        assert!(!dir.path().join(&paths[0]).exists());
        let after = read_group_file(&dir.path().join("groups/Baca_Pueblo.parquet")).expect("read");
        assert_eq!(after.profile.group_name, "Baca Pueblo");
        assert_eq!(after.profile.group_id, "Baca_Pueblo");
        assert_eq!(after.profile.revision_id, "rev-2");
        assert_eq!(after.rows, before.rows);
        assert_eq!(after.profile.source_sha256, before.profile.source_sha256);

        // A case-only rename keeps the file.
        let response = service
            .rename_group(&RenameGroupRequest {
                path: "groups/Baca_Pueblo.parquet".into(),
                expected_revision: "rev-2".into(),
                new_name: "baca pueblo".into(),
            })
            .expect("case-only rename");
        assert_eq!(response.outputs[0].path, "groups/Baca_Pueblo.parquet");
        assert!(response.deleted_paths.is_empty());
    }

    #[test]
    fn rename_group_only_checks_its_own_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let hooper = read_group_file(&dir.path().join(&paths[1])).expect("read");
        let copied = service
            .copy_groups(&CopyGroupsRequest {
                sources: vec![archaeodash_contracts::GroupRevisionRef {
                    path: paths[1].clone(),
                    expected_revision: hooper.profile.revision_id.clone(),
                }],
                destination_folder: "Other".into(),
                remove_sources: false,
            })
            .expect("copy");
        let copy = copied.outputs[0].clone();
        // "Baca" is taken in groups/ but not in Other/.
        let response = service
            .rename_group(&RenameGroupRequest {
                path: copy.path.clone(),
                expected_revision: copy.revision_id.clone(),
                new_name: "Baca".into(),
            })
            .expect("rename in another folder");
        assert_eq!(response.outputs[0].path, "Other/Baca.parquet");
        // groups/ still refuses it.
        let clash = service
            .rename_group(&RenameGroupRequest {
                path: paths[1].clone(),
                expected_revision: hooper.profile.revision_id.clone(),
                new_name: "baca".into(),
            })
            .expect_err("name in use in groups/");
        assert!(clash.to_string().contains("already has a group named"));
    }

    #[test]
    fn rename_group_refuses_clashes_blank_names_and_stale_revisions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let request = |name: &str, revision: &str| RenameGroupRequest {
            path: paths[0].clone(),
            expected_revision: revision.into(),
            new_name: name.into(),
        };
        let clash = service
            .rename_group(&request("hooper", "rev-1"))
            .expect_err("name in use");
        assert!(clash.to_string().contains("already has a group named"));
        assert!(service.rename_group(&request("  ", "rev-1")).is_err());
        // Path separators and characters invalid in file names are refused.
        for bad in [
            "Site/Baca",
            "Site\\Baca",
            "Baca:1",
            "Baca?",
            ".hidden",
            "CON",
        ] {
            let err = service.rename_group(&request(bad, "rev-1")).expect_err(bad);
            assert!(
                matches!(err, StoreError::Validation(ref m) if m.starts_with("group name")),
                "{bad}: {err}"
            );
        }
        assert!(matches!(
            service.rename_group(&request("Other", "rev-9")),
            Err(StoreError::RevisionConflict { .. })
        ));
        // A different group whose file name the new name would take.
        std::fs::copy(
            dir.path().join(&paths[1]),
            dir.path().join("groups/Other.parquet"),
        )
        .expect("copy file");
        let taken = service
            .rename_group(&request("Other", "rev-1"))
            .expect_err("file name in use");
        assert!(taken.to_string().contains("already"));
        assert!(dir.path().join(&paths[0]).exists(), "unchanged");
    }

    #[test]
    fn add_descriptive_column_writes_every_group_in_the_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let before = read_group_file(&dir.path().join(&paths[0])).expect("read");
        let add = |column: &str, value: &str, overwrite: bool| {
            service.add_descriptive_column(&AddDescriptiveColumnRequest {
                folder: "groups".into(),
                column: column.into(),
                value: Some(value.into()),
                overwrite,
            })
        };
        let response = add("Phase", "Early", false).expect("add column");
        assert_eq!(response.action, "add_descriptive_column");
        assert_eq!(response.outputs.len(), 2);
        for path in &paths {
            let data = read_group_file(&dir.path().join(path)).expect("read");
            assert_eq!(
                data.profile.roles.descriptive.last().map(String::as_str),
                Some("Phase")
            );
            assert!(data
                .rows
                .iter()
                .all(|r| r.descriptive.last() == Some(&Some("Early".into()))));
            assert_eq!(data.profile.revision_id, "rev-2");
        }
        let after = read_group_file(&dir.path().join(&paths[0])).expect("read");
        assert_eq!(
            after.profile.measured_elemental_checksum,
            before.profile.measured_elemental_checksum
        );

        // An existing column needs the overwrite confirmation.
        let exists = add("Phase", "Late", false).expect_err("exists");
        assert!(exists.to_string().contains("confirm to overwrite"));
        add("Phase", "Late", true).expect("overwrite");
        let data = read_group_file(&dir.path().join(&paths[1])).expect("read");
        let index = data
            .profile
            .roles
            .descriptive
            .iter()
            .position(|c| c == "Phase")
            .expect("column");
        assert_eq!(data.rows[0].descriptive[index].as_deref(), Some("Late"));
        assert_eq!(
            data.profile.roles.descriptive.len(),
            index + 1,
            "not added twice"
        );
    }

    #[test]
    fn add_descriptive_column_refuses_measured_id_and_identity_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (service, paths) = commit_two_groups(dir.path());
        let add = |folder: &str, column: &str| {
            service.add_descriptive_column(&AddDescriptiveColumnRequest {
                folder: folder.into(),
                column: column.into(),
                value: None,
                overwrite: true,
            })
        };
        for column in [
            "fe",
            "anid",
            "analytical_uuid",
            "legacy_rowid",
            "rowid",
            " ",
        ] {
            assert!(add("groups", column).is_err(), "{column} refused");
        }
        assert!(add("nowhere", "Phase").is_err(), "empty folder refused");
        let data = read_group_file(&dir.path().join(&paths[0])).expect("read");
        assert_eq!(data.profile.revision_id, "rev-1", "unchanged");
    }
}
