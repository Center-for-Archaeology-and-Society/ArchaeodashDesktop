//! Group use cases shared by the HTTP and Tauri adapters (Section 10.2,
//! Phase 2 local form): candidate discovery, full validation-on-add, and the
//! journaled transfer/merge operations over the transactional group store.

use std::path::PathBuf;

use archaeodash_contracts::{
    BatchTransferUnitsRequest, DeleteGroupRequest, DuplicateGroupRequest, GroupCandidate,
    GroupRowDto, GroupRowsResponse, GroupSummary, MergeGroupsRequest,
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
        let original_source = self.store.read_group(&req.source_path)?;
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
            let exists = abs.exists();
            let data = if exists {
                Some(self.store.read_group(&target.destination_path)?)
            } else {
                None
            };
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
        let transaction_id = self.store.execute(&tx)?;
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
        BatchTransferTarget, BatchTransferUnitsRequest, DescriptiveEdit, DuplicateGroupRequest,
        ImportCommitRequest, PatchDescriptiveValuesRequest,
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
