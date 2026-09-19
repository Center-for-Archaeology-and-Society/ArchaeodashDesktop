//! Group use cases shared by the HTTP and Tauri adapters (Section 10.2,
//! Phase 2 local form): candidate discovery, full validation-on-add, and the
//! journaled transfer/merge operations over the transactional group store.

use std::path::PathBuf;

use archaeodash_contracts::{
    DeleteGroupRequest, DuplicateGroupRequest, GroupCandidate, GroupSummary, MergeGroupsRequest,
    PatchDescriptiveValuesRequest, TransactionResponse, TransferAction, TransferUnitsRequest,
};
use archaeodash_data_io::{
    sanitize_group_name, scan_project, GroupFileData, GroupProfile, GroupRow,
};
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
        let transaction_id = self.store.execute(&tx)?;
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
        let transaction_id = self.store.execute(&tx)?;
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
            None => format!("groups/{group_id}.parquet"),
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
        let transaction_id = self.store.execute(&tx)?;
        Ok(TransactionResponse {
            transaction_id,
            action: "duplicate_group".to_string(),
            outputs: vec![Self::summary_of_output(&out.path, &out)],
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
        DescriptiveEdit, DuplicateGroupRequest, ImportCommitRequest, PatchDescriptiveValuesRequest,
    };
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

        // Elemental columns are locked.
        let elemental_edit = PatchDescriptiveValuesRequest {
            path: paths[0].clone(),
            expected_revision: "rev-2".into(),
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
            expected_revision: "rev-2".into(),
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
            .expect("duplicate");
        assert_eq!(resp.action, "duplicate_group");
        assert_eq!(resp.deleted_paths, Vec::<String>::new());
        assert_eq!(resp.outputs.len(), 1);
        assert_eq!(resp.outputs[0].path, "groups/Baca_Copy.parquet");
        assert_eq!(resp.outputs[0].group_name, "Baca Copy");
        assert_eq!(resp.outputs[0].revision_id, "rev-1");

        let copy =
            read_group_file(&dir.path().join("groups/Baca_Copy.parquet")).expect("read duplicate");
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
                preserve_uuids: true,
            })
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }
}
