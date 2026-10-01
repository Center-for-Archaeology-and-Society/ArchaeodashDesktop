//! Import use cases shared by the HTTP and Tauri adapters (Section 10.2,
//! Phase 2 local form): preview a source spreadsheet and commit one validated
//! group Parquet file per group value, all paths contained in the project.

use std::path::{Component, Path, PathBuf};

use archaeodash_contracts::{
    CommittedGroup, ImportCommitRequest, ImportCommitResponse, ImportPreviewRequest,
    ImportPreviewResponse, PartitionPreview,
};
use archaeodash_data_io::{
    data_loader, default_chem_columns, default_id_column, partition_by_group, sanitize_group_name,
    write_group_file, ImportError, ImportRecipe, Partition, ScanCandidate,
};
use archaeodash_file_store_fs::FsGroupFileStore;
use sha2::{Digest, Sha256};

/// Default project-relative destination for first-import group files.
const DEFAULT_DESTINATION_DIR: &str = "groups";

/// Initial revision for a first import (later revisions go through the
/// transactional group store, not this use case).
const INITIAL_REVISION: &str = "rev-1";

fn io_err(e: std::io::Error) -> ImportError {
    ImportError::Io(e.to_string())
}

/// Resolves the two mutually exclusive import grouping modes. No grouping is
/// valid for a metadata-only preview; commits must always select a mode.
fn import_partitions(
    frame: &archaeodash_data_io::TextFrame,
    group_column: Option<&str>,
    group_name: Option<&str>,
    required: bool,
) -> Result<Vec<Partition>, ImportError> {
    let column = group_column.filter(|column| !column.trim().is_empty());
    let name = group_name.filter(|name| !name.trim().is_empty());
    if group_name.is_some() && name.is_none() {
        return Err(ImportError::Parse("group name cannot be blank".into()));
    }
    match (column, name) {
        (Some(_), Some(_)) => Err(ImportError::Parse(
            "choose either a group column or one group name, not both".into(),
        )),
        (Some(column), None) => partition_by_group(frame, column),
        (None, Some(name)) => Ok(vec![Partition {
            group_name: name.trim().to_string(),
            row_indices: (0..frame.n_rows()).collect(),
        }]),
        (None, None) if required => Err(ImportError::Parse(
            "choose a group column or provide one group name".into(),
        )),
        (None, None) => Ok(Vec::new()),
    }
}

/// Shared import use cases rooted at one local project directory.
pub struct ImportService {
    root: PathBuf,
    store: FsGroupFileStore,
}

impl ImportService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ImportError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        let store = FsGroupFileStore::new(&root).map_err(|e| ImportError::Io(e.to_string()))?;
        Ok(Self { root, store })
    }

    /// Lexically contains the project root: rejects absolute paths, `..`,
    /// and root/prefix components (Section 6.3 path-containment rule).
    fn resolve(&self, rel: &str) -> Result<PathBuf, ImportError> {
        let mut out = self.root.clone();
        for component in Path::new(rel).components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                other => {
                    return Err(ImportError::Parse(format!(
                        "path {rel:?} escapes the project boundary: {other:?}"
                    )))
                }
            }
        }
        Ok(out)
    }

    /// Parses the source with the `dataLoader` semantics and returns the
    /// default role suggestions plus the optional group partition summary.
    /// Never modifies the source (Section 10.2 preview contract).
    pub fn preview(
        &self,
        req: &ImportPreviewRequest,
    ) -> Result<ImportPreviewResponse, ImportError> {
        let frame = data_loader(&self.resolve(&req.source)?)?;
        let id_column = default_id_column(&frame.columns);
        let elemental_columns = default_chem_columns(&frame.columns);
        let partitions: Vec<PartitionPreview> = import_partitions(
            &frame,
            req.group_column.as_deref(),
            req.group_name.as_deref(),
            false,
        )?
        .into_iter()
        .map(|partition| PartitionPreview {
            suggested_path: format!(
                "{DEFAULT_DESTINATION_DIR}/{}.parquet",
                sanitize_group_name(&partition.group_name)
            ),
            row_count: partition.row_indices.len() as u64,
            group_name: partition.group_name,
        })
        .collect();
        Ok(ImportPreviewResponse {
            source: req.source.clone(),
            row_count: frame.n_rows() as u64,
            columns: frame.columns,
            id_column,
            elemental_columns,
            partitions,
        })
    }

    /// Partitions the source by the group column and publishes one
    /// profile-valid Parquet file per group under the destination directory.
    /// Duplicate sanitized names get deterministic `_2`, `_3`, ... suffixes in
    /// partition order. Every published file passes full validation.
    pub fn commit(&self, req: &ImportCommitRequest) -> Result<ImportCommitResponse, ImportError> {
        let source_path = self.resolve(&req.source)?;
        let frame = data_loader(&source_path)?;

        let visible_id_column = req
            .visible_id_column
            .clone()
            .or_else(|| default_id_column(&frame.columns))
            .unwrap_or_else(|| "anid".to_string());
        let elemental_columns = req
            .elemental_columns
            .clone()
            .unwrap_or_else(|| default_chem_columns(&frame.columns));
        let recipe = req
            .recipe
            .clone()
            .map_or(ImportRecipe::default(), |dto| ImportRecipe {
                zero_as_na: dto.zero_as_na,
                negative_as_na: dto.negative_as_na,
                na_as_zero: dto.na_as_zero,
                blank_non_element_label: dto.blank_non_element_label,
            });
        let destination_dir = req
            .destination_dir
            .as_deref()
            .unwrap_or(DEFAULT_DESTINATION_DIR);
        // Path-containment check without touching the filesystem.
        self.resolve(destination_dir)?;
        // Validate partitioning before any filesystem side effects: a commit
        // rejected for blank group values must not create directories.
        let partitions = import_partitions(
            &frame,
            Some(&req.group_column),
            req.group_name.as_deref(),
            true,
        )?;

        let source_sha256 = {
            let bytes = std::fs::read(&source_path).map_err(io_err)?;
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            format!("{:x}", hasher.finalize())
        };
        let source_path_string = Some(req.source.clone());

        // Coordinate destination probing and every publication with store
        // transactions and readers for this project.
        let _project_lock = self
            .store
            .lock_exclusive()
            .map_err(|e| ImportError::Io(e.to_string()))?;

        let mut groups = Vec::new();
        for partition in partitions {
            let group_id = sanitize_group_name(&partition.group_name);
            let mut rel = format!("{destination_dir}/{group_id}.parquet");
            let mut suffix = 2usize;
            while self.resolve(&rel)?.exists() {
                rel = format!("{destination_dir}/{group_id}_{suffix}.parquet");
                suffix += 1;
            }
            let dest_abs = self.resolve(&rel)?;
            if let Some(parent) = dest_abs.parent() {
                std::fs::create_dir_all(parent).map_err(io_err)?;
            }
            let profile = write_group_file(
                &self.resolve(&rel)?,
                &group_id,
                INITIAL_REVISION,
                &visible_id_column,
                &elemental_columns,
                &recipe,
                &frame,
                &partition,
                source_path_string.clone(),
                Some(source_sha256.clone()),
                None,
            )?;
            archaeodash_data_io::validate_group_file(&self.resolve(&rel)?)?;
            groups.push(CommittedGroup {
                group_id,
                group_name: partition.group_name,
                row_count: profile.row_count as u64,
                path: rel,
                revision_id: profile.revision_id,
                measured_elemental_checksum: profile.measured_elemental_checksum,
            });
        }
        Ok(ImportCommitResponse {
            source: req.source.clone(),
            groups,
        })
    }

    /// Manifest-free discovery of published group files with their readiness
    /// states (Section 10.2 `GET /projects/{id}/candidates`, local form).
    /// Results are sorted by path; full validation happens on add/load.
    pub fn scan(&self) -> Result<Vec<ScanCandidate>, ImportError> {
        self.store
            .scan_candidates_snapshot()
            .map_err(|e| ImportError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_contracts::ImportRecipeDto;
    use archaeodash_data_io::read_group_file;
    use archaeodash_data_io::CandidateStatus;

    const MINI_CSV: &str = "anid,Site,Sub-region,as,fe\n\
                            A1,Baca,Central,1.5,3\n\
                            A2,Baca,Central,2,4\n\
                            A3,Hooper,,,6\n";

    fn service_with_source() -> (ImportService, tempdir::TempDir, String) {
        let dir = tempdir::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("mini.csv"), MINI_CSV).expect("write source");
        let service = ImportService::new(dir.path()).expect("service");
        (service, dir, "mini.csv".to_string())
    }

    // Minimal tempdir shim so the dev-dependency list stays as-is.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        pub fn tempdir() -> Result<TempDir, std::io::Error> {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "archaeodash-import-test-{}-{}",
                std::process::id(),
                n
            ));
            std::fs::create_dir_all(&dir)?;
            Ok(TempDir(dir))
        }
    }

    #[test]
    fn preview_returns_defaults_and_partitions_without_touching_source() {
        let (service, _dir, source) = service_with_source();
        let req = ImportPreviewRequest {
            source: source.clone(),
            group_column: Some("Site".into()),
            group_name: None,
        };
        let resp = service.preview(&req).expect("preview");
        assert_eq!(resp.row_count, 3);
        assert_eq!(
            resp.columns,
            vec!["rowid", "anid", "Site", "Sub_region", "as", "fe"]
        );
        assert_eq!(resp.id_column.as_deref(), Some("anid"));
        // INAA-list matches in frame order.
        assert_eq!(
            resp.elemental_columns,
            vec!["as".to_string(), "fe".to_string()]
        );
        assert_eq!(resp.partitions.len(), 2);
        assert_eq!(resp.partitions[0].group_name, "Baca");
        assert_eq!(resp.partitions[0].row_count, 2);
        assert_eq!(resp.partitions[0].suggested_path, "groups/Baca.parquet");
        // Source untouched: no group files exist after a preview.
        assert!(!service.resolve("groups").expect("resolve").exists());
    }

    #[test]
    fn preview_rejects_path_escape() {
        let (service, _dir, _source) = service_with_source();
        let req = ImportPreviewRequest {
            source: "../outside.csv".into(),
            group_column: None,
            group_name: None,
        };
        let err = service.preview(&req).expect_err("escape rejected");
        assert!(matches!(err, ImportError::Parse(msg) if msg.contains("escapes")));
    }

    #[test]
    fn commit_publishes_one_valid_file_per_group() {
        let (service, dir, source) = service_with_source();
        let req = ImportCommitRequest {
            source,
            group_column: "Site".into(),
            group_name: None,
            visible_id_column: None,
            elemental_columns: None,
            recipe: Some(ImportRecipeDto {
                zero_as_na: false,
                negative_as_na: false,
                na_as_zero: false,
                blank_non_element_label: Some("[blank]".into()),
            }),
            destination_dir: None,
        };
        let resp = service.commit(&req).expect("commit");
        assert_eq!(resp.groups.len(), 2);
        assert_eq!(resp.groups[0].group_name, "Baca");
        assert_eq!(resp.groups[0].path, "groups/Baca.parquet");
        assert_eq!(resp.groups[0].row_count, 2);
        assert_eq!(resp.groups[1].group_name, "Hooper");
        assert_eq!(resp.groups[1].path, "groups/Hooper.parquet");

        let root = dir.path();
        for group in &resp.groups {
            let data = read_group_file(&root.join(&group.path)).expect("group validates");
            assert_eq!(data.profile.group_id, group.group_id);
            assert_eq!(data.profile.revision_id, "rev-1");
            assert_eq!(data.profile.source_path.as_deref(), Some("mini.csv"));
            assert!(!data.profile.measured_elemental_checksum.is_empty());
        }
        // The blank Sub_region cell in Hooper received the recipe label;
        // descriptive roles exclude rowid/anid/elemental, so index 1.
        let hooper = read_group_file(&root.join("groups/Hooper.parquet")).expect("hooper");
        assert_eq!(hooper.rows[0].descriptive[0].as_deref(), Some("Hooper"));
        assert_eq!(hooper.rows[0].descriptive[1].as_deref(), Some("[blank]"));

        // Scan discovers both as ready-to-add, sorted by path. `scan_project`
        // returns absolute canonical paths under the project root; canonicalize
        // the expectation because macOS /tmp is a symlink (/var -> /private/var).
        let root = std::fs::canonicalize(dir.path()).expect("canonical project root");
        let scanned = service.scan().expect("scan");
        assert_eq!(scanned.len(), 2);
        assert_eq!(scanned[0].path, root.join("groups/Baca.parquet"));
        assert_eq!(scanned[1].path, root.join("groups/Hooper.parquet"));
        for candidate in &scanned {
            assert!(matches!(candidate.status, CandidateStatus::ReadyToAdd(_)));
        }
    }

    #[test]
    fn commit_waits_for_exclusive_store_project_lock() {
        let (service, dir, source) = service_with_source();
        let locker = FsGroupFileStore::new(dir.path()).expect("store lock handle");
        let lock = locker.lock_exclusive().expect("exclusive project lock");
        let request = ImportCommitRequest {
            source,
            group_column: "Site".into(),
            group_name: None,
            visible_id_column: None,
            elemental_columns: None,
            recipe: None,
            destination_dir: None,
        };
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            done_tx
                .send(service.commit(&request))
                .expect("send commit result");
        });

        assert!(done_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err());
        assert!(
            !dir.path().join("groups").exists(),
            "commit does not publish before acquiring the lock"
        );
        drop(lock);
        let response = done_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("commit completes after lock release")
            .expect("commit succeeds");
        assert_eq!(response.groups.len(), 2);
        worker.join().expect("commit worker joins");
    }

    #[test]
    fn preview_and_commit_can_assign_every_row_to_one_named_group() {
        let (service, dir, source) = service_with_source();
        let preview = service
            .preview(&ImportPreviewRequest {
                source: source.clone(),
                group_column: None,
                group_name: Some("Reference Set".into()),
            })
            .expect("single-group preview");
        assert_eq!(preview.partitions.len(), 1);
        assert_eq!(preview.partitions[0].group_name, "Reference Set");
        assert_eq!(preview.partitions[0].row_count, 3);
        assert_eq!(
            preview.partitions[0].suggested_path,
            "groups/Reference_Set.parquet"
        );

        let committed = service
            .commit(&ImportCommitRequest {
                source,
                group_column: String::new(),
                group_name: Some("Reference Set".into()),
                visible_id_column: Some("anid".into()),
                elemental_columns: Some(vec!["as".into(), "fe".into()]),
                recipe: None,
                destination_dir: None,
            })
            .expect("single-group commit");
        assert_eq!(committed.groups.len(), 1);
        assert_eq!(committed.groups[0].group_name, "Reference Set");
        assert_eq!(committed.groups[0].path, "groups/Reference_Set.parquet");
        assert_eq!(committed.groups[0].row_count, 3);
        let data = read_group_file(&dir.path().join(&committed.groups[0].path))
            .expect("committed group validates");
        assert_eq!(data.profile.group_id, "Reference_Set");
        assert_eq!(data.rows.len(), 3);
    }

    #[test]
    fn commit_requires_exactly_one_nonblank_grouping_mode() {
        let (service, _dir, source) = service_with_source();
        for (group_column, group_name) in [
            ("".to_string(), None),
            ("Site".to_string(), Some("  ".to_string())),
            ("Site".to_string(), Some("Named".to_string())),
        ] {
            let error = service
                .commit(&ImportCommitRequest {
                    source: source.clone(),
                    group_column,
                    group_name,
                    visible_id_column: None,
                    elemental_columns: None,
                    recipe: None,
                    destination_dir: None,
                })
                .expect_err("invalid grouping mode rejected");
            assert!(matches!(error, ImportError::Parse(_)));
        }
    }

    #[test]
    fn commit_rejects_blank_group_values() {
        let dir = tempdir::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("blank.csv"),
            "anid,Site,as\nA1,Baca,1\nA2,,2\n",
        )
        .expect("write");
        let service = ImportService::new(dir.path()).expect("service");
        let err = service
            .commit(&ImportCommitRequest {
                source: "blank.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect_err("blank group rejected");
        assert!(matches!(err, ImportError::Parse(msg) if msg.contains("blank")));
        // Nothing was published for the rejected import.
        assert!(!dir.path().join("groups").exists());
    }

    #[test]
    fn commit_dedupes_sanitized_group_names() {
        let dir = tempdir::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("collide.csv"),
            "anid,Site,as\nA1,A/B,1\nA2,A_B,2\n",
        )
        .expect("write");
        let service = ImportService::new(dir.path()).expect("service");
        let resp = service
            .commit(&ImportCommitRequest {
                source: "collide.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: Some(vec!["as".into()]),
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        assert_eq!(resp.groups[0].path, "groups/A_B.parquet");
        assert_eq!(resp.groups[1].path, "groups/A_B_2.parquet");
    }
}
