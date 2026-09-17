//! Group use cases shared by the HTTP and Tauri adapters (Section 10.2,
//! Phase 2 local form): candidate discovery, full validation-on-add, and the
//! journaled transfer/merge operations over the transactional group store.

use std::path::PathBuf;

use archaeodash_contracts::{
    GroupCandidate, GroupSummary, MergeGroupsRequest, TransactionResponse, TransferAction,
    TransferUnitsRequest,
};
use archaeodash_data_io::{sanitize_group_name, scan_project, GroupFileData, GroupProfile};
use archaeodash_file_store_fs::FsGroupFileStore;
use archaeodash_storage::{
    plan_copy, plan_merge, plan_move, FileInput, GroupFileStore, PlannedOutput, StoreError,
    Transaction, TransactionAction,
};
use uuid::Uuid;

/// Group operations rooted at one local project directory. Construction runs
/// the Section 6.8 startup recovery before any other use case.
pub struct GroupService {
    store: FsGroupFileStore,
    root: PathBuf,
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
        let candidates = scan_project(&self.root)?;
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
        Ok(out)
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
                Uuid::parse_str(s).map_err(|e| {
                    StoreError::Invariant(format!("invalid analytical uuid {s:?}: {e}"))
                })
            })
            .collect()
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

        let source = self.store.read_group(&req.source_path)?;
        if source.profile.revision_id != req.expected_source_revision {
            return Err(StoreError::RevisionConflict {
                path: req.source_path.clone(),
                expected: req.expected_source_revision.clone(),
                found: source.profile.revision_id.clone(),
            });
        }

        let dest_path = req.destination_path.as_str();
        if req.action == TransferAction::Copy && dest_path == req.source_path {
            return Err(StoreError::Invariant(
                "copy destination must differ from the source".to_string(),
            ));
        }
        let dest_abs = self.store.resolve(dest_path)?;
        let dest_data = if dest_abs.exists() {
            Some(self.store.read_group(dest_path)?)
        } else {
            None
        };

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
        let transaction_id = self.store.execute(&tx)?;
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
        for path in &req.sources {
            let data = self.store.read_group(path)?;
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
        let transaction_id = self.store.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "merge_groups".to_string(),
            outputs: vec![Self::summary_of_output(&out.path, &out)],
            deleted_paths: req.sources[1..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use crate::import::ImportService;
    use archaeodash_contracts::ImportCommitRequest;
    use archaeodash_data_io::read_group_file;

    fn commit_two_groups(root: &std::path::Path) -> (GroupService, Vec<String>) {
        std::fs::write(
            root.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,5,6\n",
        )
        .expect("write source");
        let import = ImportService::new(root).expect("import service");
        let resp = import
            .commit(&ImportCommitRequest {
                source: "mini.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
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
            .expect("copy");
        let dst =
            read_group_file(&dir.path().join("groups/copy-target.parquet")).expect("new group");
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
}
