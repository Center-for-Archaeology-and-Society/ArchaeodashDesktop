//! GroupFileStore traits and transaction planning (Section 6.1, 6.7, 6.8).
//!
//! This crate is pure logic: it plans multi-file group operations and
//! verifies their invariants before any bytes move. Filesystem effects live
//! in `archaeodash-file-store-fs`; the hosted object-store backend lands with
//! Phase 7 behind the same trait.
//!
//! Transaction model (Section 6.8): every mutating operation writes an intent
//! naming every input path/revision/checksum and intended output, builds
//! successor files in a staging area, validates them in full, backs up the
//! originals, publishes by atomic rename, then commits and cleans up. A
//! startup recovery pass finishes or rolls back interrupted transactions.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use archaeodash_data_io::{ColumnRoles, GroupFileData, GroupProfile, GroupRow};

/// Typed storage error surfaced to the application layer.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("revision conflict on {path}: expected {expected}, found {found}")]
    RevisionConflict {
        path: String,
        expected: String,
        found: String,
    },
    #[error("schema mismatch between {left} and {right}: {detail}")]
    SchemaMismatch {
        left: String,
        right: String,
        detail: String,
    },
    #[error("transaction invariant violated: {0}")]
    Invariant(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("validation error: {0}")]
    Validation(String),
}

impl From<archaeodash_data_io::ImportError> for StoreError {
    fn from(e: archaeodash_data_io::ImportError) -> Self {
        match e {
            archaeodash_data_io::ImportError::Io(m) => StoreError::Io(m),
            archaeodash_data_io::ImportError::Parse(m) => StoreError::Validation(m),
            // Not-found/limit never arise from storage-internal imports today;
            // surface both as IO failures preserving the message.
            archaeodash_data_io::ImportError::NotFound(m) => StoreError::Io(m),
            archaeodash_data_io::ImportError::Limit(m) => StoreError::Io(m),
        }
    }
}

/// One input file of a transaction, named by project-relative path with the
/// revision and measured checksum expected at plan time (optimistic
/// concurrency; Section 6.8 `expected_revision`/`If-Match`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileInput {
    pub path: String,
    pub revision_id: String,
    pub measured_elemental_checksum: String,
}

/// One intended output file of a transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileOutput {
    pub path: String,
    pub group_id: String,
    pub group_name: String,
    /// Paths that did not exist before this transaction; recovery deletes
    /// them on rollback instead of guessing.
    #[serde(default)]
    pub created: bool,
}

/// The mutating operation a transaction performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionAction {
    /// Move selected analytical units from source(s) into one destination.
    MoveUnits,
    /// Copy selected analytical units, preserving their UUIDs.
    CopyUnits,
    /// Merge whole groups into one target; sources are removed.
    MergeGroups,
    /// Delete one group file after revision/checksum preconditions pass;
    /// the original is archived by the Section 6.8 backup protocol.
    DeleteGroup,
    /// Batch hidden-UUID-addressed descriptive edits on one group file;
    /// measured elemental values are never touched (Section 13
    /// `R/updateCurrent.R`).
    PatchDescriptive,
    /// Duplicate one whole group to a new path, preserving analytical UUIDs
    /// and source lineage by default (Section 10.2
    /// `POST /groups/{id}/duplicate`).
    DuplicateGroup,
    /// Copy whole groups into another folder (same identities and names).
    CopyGroups,
    /// Move whole groups into another folder; the sources are removed.
    MoveGroups,
    /// Rename one group (profile name/id and file name); rows unchanged.
    RenameGroup,
    /// Add (or overwrite) one descriptive column in every group of a folder;
    /// measured values are never touched.
    AddDescriptiveColumn,
    /// Re-import a source over the groups previously imported from it.
    ReplaceImport,
    /// Import rows into existing groups of a folder (new groups for the rest).
    AppendImport,
    /// Turn a clustering of whole groups into cluster groups, replacing the
    /// analyzed groups or in a new folder.
    ApplyClusters,
}

/// Journal record for one multi-file transaction (Section 6.8). Serialized to
/// `.archaeodash/transactions/<txid>/intent.json` before any file is touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransactionIntent {
    pub transaction_id: String,
    pub action: TransactionAction,
    pub inputs: Vec<FileInput>,
    pub outputs: Vec<FileOutput>,
    /// Analytical UUIDs the operation moves or copies.
    pub selected_uuids: Vec<Uuid>,
    /// Journal write time, seconds since the Unix epoch.
    pub started_at_unix_secs: u64,
}

/// A successor group file computed by planning: full row data plus the
/// identity of the file it replaces (or creates).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedOutput {
    pub path: String,
    pub group_id: String,
    pub group_name: String,
    /// Fresh revision stamped by the executor when writing the successor.
    pub revision_id: String,
    pub rows: Vec<archaeodash_data_io::GroupRow>,
    /// Roles carried over from the destination when one exists, plus any
    /// columns the incoming rows add (see [`merged_roles`]), else the
    /// source's.
    pub roles: archaeodash_data_io::ColumnRoles,
    /// Import recipe carried from the source group.
    pub recipe: archaeodash_data_io::ImportRecipe,
    pub source_path: Option<String>,
    pub source_sha256: Option<String>,
}

/// Roles of a group receiving rows from groups with other columns: the
/// target's roles, then the elemental and descriptive columns only the
/// others have (first-seen order). The ID, rowid, and identity columns stay
/// the target's. Refused when a column name would have two roles (measured
/// in one group and descriptive or an ID column in another).
pub fn merged_roles(
    target_path: &str,
    target: &ColumnRoles,
    others: &[(&str, &ColumnRoles)],
) -> Result<ColumnRoles, StoreError> {
    let mut roles = target.clone();
    let clash = |other: &str, detail: String| StoreError::SchemaMismatch {
        left: target_path.to_string(),
        right: other.to_string(),
        detail,
    };
    for (path, other) in others {
        for name in &other.elemental {
            if roles.elemental.contains(name) {
                continue;
            }
            if roles.descriptive.contains(name) || is_id_column(&roles, name) {
                return Err(clash(
                    path,
                    format!("column {name:?} is measured in one group but not in the other"),
                ));
            }
            roles.elemental.push(name.clone());
        }
        for name in &other.descriptive {
            if roles.descriptive.contains(name) {
                continue;
            }
            if roles.elemental.contains(name) {
                return Err(clash(
                    path,
                    format!("column {name:?} is measured in one group but not in the other"),
                ));
            }
            if is_id_column(&roles, name) {
                return Err(clash(
                    path,
                    format!("column {name:?} is the ID column of one group but descriptive in the other"),
                ));
            }
            roles.descriptive.push(name.clone());
        }
    }
    Ok(roles)
}

fn is_id_column(roles: &ColumnRoles, name: &str) -> bool {
    name == roles.visible_id || name == roles.legacy_rowid || name == roles.identity
}

/// One row re-aligned by column name from `from` roles to `to` roles: a
/// measured column `from` lacks is 0, a descriptive column it lacks is
/// missing, except a descriptive column named like `from`'s ID column, which
/// gets the row's ID. Identity, ID, and rowid values are kept.
pub fn align_row(row: &GroupRow, from: &ColumnRoles, to: &ColumnRoles) -> GroupRow {
    if from.elemental == to.elemental && from.descriptive == to.descriptive {
        return row.clone();
    }
    GroupRow {
        uuid: row.uuid,
        visible: row.visible.clone(),
        legacy_rowid: row.legacy_rowid.clone(),
        descriptive: to
            .descriptive
            .iter()
            .map(
                |name| match from.descriptive.iter().position(|c| c == name) {
                    Some(i) => row.descriptive.get(i).cloned().flatten(),
                    None if *name == from.visible_id => row.visible.clone(),
                    None => None,
                },
            )
            .collect(),
        elemental: to
            .elemental
            .iter()
            .map(|name| match from.elemental.iter().position(|c| c == name) {
                Some(i) => row.elemental.get(i).copied().flatten(),
                None => Some(0.0),
            })
            .collect(),
    }
}

/// Verifies measured values are unchanged for every preserved UUID when the
/// successor has the same columns as `before` (Section 6.8 invariant); see
/// [`assert_measured_values_preserved_in`].
pub fn assert_measured_values_preserved(
    selected: &[Uuid],
    before: &GroupFileData,
    after_rows: &[GroupRow],
) -> Result<(), StoreError> {
    assert_measured_values_preserved_in(selected, before, after_rows, &before.profile.roles)
}

/// Verifies measured values are unchanged for every preserved UUID, by
/// column name: each of the row's measured values in `before` appears
/// bit-identically (or missing) under the same column in `after_rows`
/// (columns `after_roles`), and columns `before` lacked are 0 (Section 6.8
/// invariant).
pub fn assert_measured_values_preserved_in(
    selected: &[Uuid],
    before: &GroupFileData,
    after_rows: &[GroupRow],
    after_roles: &ColumnRoles,
) -> Result<(), StoreError> {
    let before_columns = &before.profile.roles.elemental;
    let mut source_of = Vec::with_capacity(after_roles.elemental.len());
    for name in &after_roles.elemental {
        source_of.push(before_columns.iter().position(|c| c == name));
    }
    if let Some(lost) = before_columns
        .iter()
        .find(|name| !after_roles.elemental.contains(name))
    {
        return Err(StoreError::Invariant(format!(
            "measured column {lost:?} missing from successor output"
        )));
    }
    let before_by_uuid: std::collections::HashMap<Uuid, &GroupRow> =
        before.rows.iter().map(|r| (r.uuid, r)).collect();
    let selected_set: HashSet<Uuid> = selected.iter().copied().collect();
    for row in after_rows {
        if !selected_set.contains(&row.uuid) {
            continue;
        }
        let original = before_by_uuid.get(&row.uuid).ok_or_else(|| {
            StoreError::Invariant(format!("unexpected uuid {} in successor output", row.uuid))
        })?;
        let changed =
            || StoreError::Invariant(format!("measured values changed for uuid {}", row.uuid));
        if row.elemental.len() != source_of.len() {
            return Err(changed());
        }
        for (value, source) in row.elemental.iter().zip(&source_of) {
            let expected = match source {
                Some(i) => original.elemental.get(*i).copied().flatten(),
                None => Some(0.0),
            };
            if value.map(f64::to_bits) != expected.map(f64::to_bits) {
                return Err(changed());
            }
        }
    }
    Ok(())
}

/// An existing destination's rows aligned to `roles` (its columns plus the
/// ones incoming rows add); their measured values must be unchanged.
fn existing_rows(
    dest: Option<&GroupFileData>,
    roles: &ColumnRoles,
) -> Result<Vec<GroupRow>, StoreError> {
    let Some(dest) = dest else {
        return Ok(Vec::new());
    };
    let rows: Vec<GroupRow> = dest
        .rows
        .iter()
        .map(|r| align_row(r, &dest.profile.roles, roles))
        .collect();
    let uuids: Vec<Uuid> = dest.rows.iter().map(|r| r.uuid).collect();
    assert_measured_values_preserved_in(&uuids, dest, &rows, roles)?;
    Ok(rows)
}

/// Plans a move: selected units leave `source` and join `destination`.
///
/// Returns the successor outputs; an empty `destination` means the units move
/// into a newly created group file. If every source row moves out, the source
/// successor is `None` and the source file is deleted by the transaction.
/// UUID multisets are preserved exactly: no lost or duplicated identities.
#[allow(clippy::too_many_arguments)]
pub fn plan_move(
    source_path: &str,
    source: &GroupFileData,
    destination_path: Option<(&str, Option<&GroupFileData>)>,
    selected_uuids: &[Uuid],
    new_source_revision: &str,
    destination_group_id: &str,
    destination_group_name: &str,
    new_destination_revision: &str,
) -> Result<(Option<PlannedOutput>, Option<PlannedOutput>), StoreError> {
    let selected: HashSet<Uuid> = selected_uuids.iter().copied().collect();
    let all_uuids: HashSet<Uuid> = source.rows.iter().map(|r| r.uuid).collect();
    let unknown: Vec<_> = selected.difference(&all_uuids).collect();
    if !unknown.is_empty() {
        return Err(StoreError::Invariant(format!(
            "{} selected uuid(s) not present in source {}",
            unknown.len(),
            source_path
        )));
    }

    let (dest_path, dest): (&str, Option<&GroupFileData>) = match destination_path {
        Some((path, existing)) => (path, existing),
        None => {
            return Err(StoreError::Invariant(
                "move requires a destination path".to_string(),
            ))
        }
    };
    // An existing destination keeps its columns plus any the moved rows add.
    let roles = match dest {
        Some(d) => merged_roles(
            dest_path,
            &d.profile.roles,
            &[(source_path, &source.profile.roles)],
        )?,
        None => source.profile.roles.clone(),
    };
    if let Some(dest) = dest {
        let dest_uuids: HashSet<Uuid> = dest.rows.iter().map(|r| r.uuid).collect();
        let collisions: Vec<_> = selected.intersection(&dest_uuids).collect();
        if !collisions.is_empty() {
            return Err(StoreError::Invariant(format!(
                "{} selected uuid(s) already present in destination {dest_path}",
                collisions.len()
            )));
        }
    }
    let moving: Vec<archaeodash_data_io::GroupRow> = source
        .rows
        .iter()
        .filter(|r| selected.contains(&r.uuid))
        .map(|r| align_row(r, &source.profile.roles, &roles))
        .collect();
    if moving.is_empty() {
        return Err(StoreError::Invariant("move selected no rows".to_string()));
    }
    assert_measured_values_preserved_in(selected_uuids, source, &moving, &roles)?;

    let mut dest_rows = existing_rows(dest, &roles)?;
    dest_rows.extend(moving);
    let dest_out = PlannedOutput {
        path: dest_path.to_string(),
        group_id: destination_group_id.to_string(),
        group_name: destination_group_name.to_string(),
        revision_id: new_destination_revision.to_string(),
        rows: dest_rows,
        roles,
        recipe: match dest {
            Some(d) => d.profile.import_recipe.clone(),
            None => source.profile.import_recipe.clone(),
        },
        source_path: match dest {
            Some(d) => d.profile.source_path.clone(),
            None => source.profile.source_path.clone(),
        },
        source_sha256: match dest {
            Some(d) => d.profile.source_sha256.clone(),
            None => source.profile.source_sha256.clone(),
        },
    };

    let remaining: Vec<archaeodash_data_io::GroupRow> = source
        .rows
        .iter()
        .filter(|r| !selected.contains(&r.uuid))
        .cloned()
        .collect();
    if remaining.is_empty() {
        // Every unit moved out; the source group ceases to exist.
        return Ok((None, Some(dest_out)));
    }
    let source_out = PlannedOutput {
        path: source_path.to_string(),
        group_id: source.profile.group_id.clone(),
        group_name: source.profile.group_name.clone(),
        revision_id: new_source_revision.to_string(),
        rows: remaining,
        roles: source.profile.roles.clone(),
        recipe: source.profile.import_recipe.clone(),
        source_path: source.profile.source_path.clone(),
        source_sha256: source.profile.source_sha256.clone(),
    };
    Ok((Some(source_out), Some(dest_out)))
}

/// Plans a copy: selected units keep their UUIDs (same analytical units) and
/// appear in the destination in addition to the unchanged source
/// (Section 6.7: copying preserves the same `analytical_uuid`).
#[allow(clippy::too_many_arguments)]
pub fn plan_copy(
    source_path: &str,
    source: &GroupFileData,
    destination_path: &str,
    destination: Option<&GroupFileData>,
    selected_uuids: &[Uuid],
    destination_group_id: &str,
    destination_group_name: &str,
    destination_revision: &str,
) -> Result<Option<PlannedOutput>, StoreError> {
    let selected: HashSet<Uuid> = selected_uuids.iter().copied().collect();
    let all_uuids: HashSet<Uuid> = source.rows.iter().map(|r| r.uuid).collect();
    if selected.difference(&all_uuids).next().is_some() {
        return Err(StoreError::Invariant(format!(
            "selected uuid(s) not present in source {source_path}"
        )));
    }
    // An existing destination keeps its columns plus any the copies add.
    let roles = match destination {
        Some(d) => merged_roles(
            destination_path,
            &d.profile.roles,
            &[(source_path, &source.profile.roles)],
        )?,
        None => source.profile.roles.clone(),
    };
    let moving: Vec<archaeodash_data_io::GroupRow> = source
        .rows
        .iter()
        .filter(|r| selected.contains(&r.uuid))
        .map(|r| align_row(r, &source.profile.roles, &roles))
        .collect();
    if moving.is_empty() {
        return Err(StoreError::Invariant("copy selected no rows".to_string()));
    }
    assert_measured_values_preserved_in(selected_uuids, source, &moving, &roles)?;

    let dest_rows = match destination {
        Some(dest) => {
            let dest_uuids: HashSet<Uuid> = dest.rows.iter().map(|r| r.uuid).collect();
            let collisions: Vec<_> = selected.intersection(&dest_uuids).collect();
            if !collisions.is_empty() {
                return Err(StoreError::Invariant(format!(
                    "{} selected uuid(s) already present in destination {}",
                    collisions.len(),
                    destination_path
                )));
            }
            let mut rows = existing_rows(Some(dest), &roles)?;
            rows.extend(moving);
            rows
        }
        None => moving,
    };
    let (recipe, source_path_out, source_sha256) = match destination {
        Some(dest) => (
            dest.profile.import_recipe.clone(),
            dest.profile.source_path.clone(),
            dest.profile.source_sha256.clone(),
        ),
        None => (
            source.profile.import_recipe.clone(),
            source.profile.source_path.clone(),
            source.profile.source_sha256.clone(),
        ),
    };
    Ok(Some(PlannedOutput {
        path: destination_path.to_string(),
        group_id: destination_group_id.to_string(),
        group_name: destination_group_name.to_string(),
        revision_id: destination_revision.to_string(),
        rows: dest_rows,
        roles,
        recipe,
        source_path: source_path_out,
        source_sha256,
    }))
}

/// Plans a merge: every row of every source joins the target group and the
/// sources are removed (Section 6.7 merge). Duplicate UUIDs across sources
/// are rejected: analyses must never count an analytical unit twice.
pub fn plan_merge(
    sources: &[(String, GroupFileData)],
    target_index: usize,
    new_group_id: &str,
    new_group_name: &str,
    new_revision: &str,
) -> Result<PlannedOutput, StoreError> {
    if sources.len() < 2 {
        return Err(StoreError::Invariant(
            "merge requires at least two groups".to_string(),
        ));
    }
    if target_index >= sources.len() {
        return Err(StoreError::Invariant(
            "merge target out of range".to_string(),
        ));
    }
    let (target_path, target) = &sources[target_index];
    // The target's columns plus any the other groups add.
    let others: Vec<(&str, &ColumnRoles)> = sources
        .iter()
        .map(|(path, group)| (path.as_str(), &group.profile.roles))
        .collect();
    let roles = merged_roles(target_path, &target.profile.roles, &others)?;
    let mut seen: HashSet<Uuid> = HashSet::new();
    let mut rows = Vec::new();
    for (path, group) in sources {
        let start = rows.len();
        for row in &group.rows {
            if !seen.insert(row.uuid) {
                return Err(StoreError::Invariant(format!(
                    "duplicate analytical uuid {} across merge sources ({path})",
                    row.uuid
                )));
            }
            rows.push(align_row(row, &group.profile.roles, &roles));
        }
        let uuids: Vec<Uuid> = group.rows.iter().map(|r| r.uuid).collect();
        assert_measured_values_preserved_in(&uuids, group, &rows[start..], &roles)?;
    }
    let (recipe, source_path, source_sha256) = {
        let p = &target.profile;
        (
            p.import_recipe.clone(),
            p.source_path.clone(),
            p.source_sha256.clone(),
        )
    };
    Ok(PlannedOutput {
        path: target_path.clone(),
        group_id: new_group_id.to_string(),
        group_name: new_group_name.to_string(),
        revision_id: new_revision.to_string(),
        rows,
        roles,
        recipe,
        source_path,
        source_sha256,
    })
}

/// Application-facing group-file store (Section 6.1). Path arguments are
/// project-relative; implementations must resolve and contain them.
pub trait GroupFileStore {
    /// Full validation-on-add: profile, schema, identities, checksum.
    fn validate_group(&self, path: &str) -> Result<GroupProfile, StoreError>;
    /// Validated read of all row data.
    fn read_group(&self, path: &str) -> Result<GroupFileData, StoreError>;
    /// Executes one planned transaction through the Section 6.8 protocol:
    /// precondition check, journal intent, stage, validate, backup, publish,
    /// commit marker, cleanup. Returns the transaction id.
    fn execute(&self, tx: &Transaction) -> Result<String, StoreError>;
    /// Finishes or rolls back interrupted transactions at startup
    /// (Section 6.8: "startup resolves incomplete journal entries before
    /// scanning candidates"). Returns the number of recovered transactions.
    fn recover_interrupted(&self) -> Result<usize, StoreError>;
}

/// One executable transaction: planned outputs, paths to delete, and the
/// expected preconditions every input file must still satisfy.
#[derive(Debug, Clone, PartialEq)]
pub struct Transaction {
    pub action: TransactionAction,
    pub inputs: Vec<FileInput>,
    pub outputs: Vec<PlannedOutput>,
    /// Group files that cease to exist (move-out sources, merge sources).
    pub delete_paths: Vec<String>,
    /// Analytical UUIDs the operation moves or copies (journaled for audit).
    pub selected_uuids: Vec<Uuid>,
}

/// A backup record written after originals are copied into the transaction
/// directory; rollback restores `path` from `backup_file`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupRecord {
    pub path: String,
    pub backup_file: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use archaeodash_data_io::{GroupProfile, GroupRow};

    fn profile(group_id: &str) -> GroupProfile {
        GroupProfile {
            profile_version: 1,
            file_kind: "group".to_string(),
            group_id: group_id.to_string(),
            group_name: group_id.to_string(),
            revision_id: "r1".to_string(),
            roles: archaeodash_data_io::ColumnRoles {
                identity: "analytical_uuid".to_string(),
                visible_id: "anid".to_string(),
                legacy_rowid: "legacy_rowid".to_string(),
                descriptive: vec!["site".to_string()],
                elemental: vec!["as".to_string(), "fe".to_string()],
            },
            row_count: 0,
            source_path: None,
            source_sha256: None,
            import_recipe: archaeodash_data_io::ImportRecipe::default(),
            measured_elemental_checksum: String::new(),
        }
    }

    fn row(uuid: Uuid, as_val: Option<f64>, fe_val: Option<f64>) -> GroupRow {
        GroupRow {
            uuid,
            visible: Some("anid".to_string()),
            legacy_rowid: None,
            descriptive: vec![Some("site".to_string())],
            elemental: vec![as_val, fe_val],
        }
    }

    fn data(group_id: &str, rows: Vec<GroupRow>) -> GroupFileData {
        GroupFileData {
            profile: profile(group_id),
            rows,
        }
    }

    #[test]
    fn move_plan_preserves_uuid_multisets_and_measured_values() {
        let u = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
        let source = data(
            "g1",
            vec![row(u[0], Some(1.0), None), row(u[1], Some(2.0), Some(3.0))],
        );
        let dest = data("g2", vec![row(u[2], None, Some(9.0))]);
        let (src_out, dest_out) = plan_move(
            "groups/g1.parquet",
            &source,
            Some(("groups/g2.parquet", Some(&dest))),
            &[u[0]],
            "r2",
            "g2",
            "g2",
            "r2",
        )
        .expect("plan");

        let src = src_out.expect("source keeps one row");
        assert_eq!(src.rows.len(), 1);
        assert_eq!(src.rows[0].uuid, u[1]);
        let dst = dest_out.expect("destination gains one row");
        assert_eq!(dst.rows.len(), 2);
        assert_eq!(dst.rows[1].uuid, u[0]);
        assert_measured_values_preserved(&[u[0]], &source, &dst.rows).expect("values preserved");
    }

    #[test]
    fn move_plan_deletes_emptied_source() {
        let u = Uuid::now_v7();
        let source = data("g1", vec![row(u, Some(1.0), None)]);
        let (src_out, dest_out) = plan_move(
            "groups/g1.parquet",
            &source,
            Some(("groups/g2.parquet", None)),
            &[u],
            "r2",
            "g2",
            "g2",
            "r2",
        )
        .expect("plan");
        assert!(src_out.is_none(), "emptied source is deleted");
        assert_eq!(dest_out.expect("dest").rows.len(), 1);
    }

    #[test]
    fn move_plan_rejects_unknown_and_duplicate_uuids() {
        let u = Uuid::now_v7();
        let source = data("g1", vec![row(u, Some(1.0), None)]);
        let err = plan_move(
            "groups/g1.parquet",
            &source,
            Some(("groups/g2.parquet", None)),
            &[Uuid::now_v7()],
            "r2",
            "g2",
            "g2",
            "r2",
        )
        .expect_err("unknown uuid rejected");
        assert!(matches!(err, StoreError::Invariant(_)));

        let dest = data("g2", vec![row(u, Some(5.0), None)]);
        let err = plan_move(
            "groups/g1.parquet",
            &source,
            Some(("groups/g2.parquet", Some(&dest))),
            &[u],
            "r2",
            "g2",
            "g2",
            "r2",
        )
        .expect_err("duplicate uuid rejected");
        assert!(matches!(err, StoreError::Invariant(_)));
    }

    #[test]
    fn copy_plan_keeps_uuids_and_source_unchanged() {
        let u = Uuid::now_v7();
        let source = data("g1", vec![row(u, Some(1.0), None)]);
        let out = plan_copy(
            "groups/g1.parquet",
            &source,
            "groups/g3.parquet",
            None,
            &[u],
            "g3",
            "g3",
            "r2",
        )
        .expect("plan")
        .expect("new destination");
        assert_eq!(out.rows.len(), 1);
        assert_eq!(out.rows[0].uuid, u, "copy preserves the analytical uuid");
    }

    /// A group with columns `elemental` (`zn` extra), descriptive `ware`
    /// instead of `site`, and ID column `fsno`.
    fn other_schema(group_id: &str, rows: Vec<(Uuid, f64, Option<f64>, f64)>) -> GroupFileData {
        let mut out = data(group_id, Vec::new());
        out.profile.roles.elemental = vec!["zn".to_string(), "as".to_string(), "fe".to_string()];
        out.profile.roles.descriptive = vec!["ware".to_string()];
        out.profile.roles.visible_id = "fsno".to_string();
        out.rows = rows
            .into_iter()
            .map(|(uuid, zn, as_val, fe)| GroupRow {
                uuid,
                visible: Some(format!("F-{zn}")),
                legacy_rowid: None,
                descriptive: vec![Some("glaze".to_string())],
                elemental: vec![Some(zn), as_val, Some(fe)],
            })
            .collect();
        out
    }

    #[test]
    fn row_operations_align_differing_columns_by_name() {
        let u = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
        let source = other_schema(
            "g2",
            vec![(u[0], 7.0, None, 3.0), (u[1], 8.0, Some(1.5), 4.0)],
        );
        let dest = data("g1", vec![row(u[2], Some(1.0), None)]);
        // Move one row into a group lacking `zn` and `ware`: the destination
        // keeps its columns and ID column, gains the new ones, and its own
        // rows get 0 for `zn` and a missing `ware`.
        let (_, dest_out) = plan_move(
            "groups/g2.parquet",
            &source,
            Some(("groups/g1.parquet", Some(&dest))),
            &[u[0]],
            "r2",
            "g1",
            "g1",
            "r2",
        )
        .expect("plan");
        let out = dest_out.expect("destination");
        assert_eq!(out.roles.elemental, vec!["as", "fe", "zn"]);
        assert_eq!(out.roles.descriptive, vec!["site", "ware"]);
        assert_eq!(out.roles.visible_id, "anid");
        assert_eq!(out.rows[0].elemental, vec![Some(1.0), None, Some(0.0)]);
        assert_eq!(
            out.rows[0].descriptive,
            vec![Some("site".to_string()), None]
        );
        // The moved row keeps its values by name; its missing `as` stays missing.
        assert_eq!(out.rows[1].elemental, vec![None, Some(3.0), Some(7.0)]);
        assert_eq!(
            out.rows[1].descriptive,
            vec![None, Some("glaze".to_string())]
        );
        assert_eq!(out.rows[1].visible.as_deref(), Some("F-7"));
        assert_measured_values_preserved_in(&[u[0]], &source, &out.rows, &out.roles)
            .expect("preserved by name");
        // Positional comparison would have flagged the reordering.
        assert!(assert_measured_values_preserved(&[u[0]], &source, &out.rows).is_err());

        // Copy the other way: `zn`, `as`, `fe` already there; `as` gets 0 only
        // where the column is absent, `site` becomes a new descriptive column.
        let copy = plan_copy(
            "groups/g1.parquet",
            &dest,
            "groups/g2.parquet",
            Some(&source),
            &[u[2]],
            "g2",
            "g2",
            "r2",
        )
        .expect("plan")
        .expect("destination");
        assert_eq!(copy.roles.elemental, vec!["zn", "as", "fe"]);
        assert_eq!(copy.roles.descriptive, vec!["ware", "site"]);
        assert_eq!(copy.rows[2].elemental, vec![Some(0.0), Some(1.0), None]);
        assert_eq!(copy.rows[0].elemental, vec![Some(7.0), None, Some(3.0)]);

        // Merge: union of columns, each source's values kept by name.
        let merged = plan_merge(
            &[
                ("groups/g1.parquet".to_string(), dest.clone()),
                ("groups/g2.parquet".to_string(), source.clone()),
            ],
            0,
            "gm",
            "merged",
            "r2",
        )
        .expect("merge");
        assert_eq!(merged.roles.elemental, vec!["as", "fe", "zn"]);
        assert_eq!(merged.rows.len(), 3);
        assert_eq!(
            merged.rows[2].elemental,
            vec![Some(1.5), Some(4.0), Some(8.0)]
        );

        // A row whose source has a descriptive column named like the
        // destination's ID column gets its ID there when the cell is absent.
        let mut with_fsno = dest.clone();
        with_fsno.profile.roles.descriptive.push("fsno".to_string());
        with_fsno.rows[0].descriptive.push(Some("F-0".to_string()));
        let (_, out) = plan_move(
            "groups/g2.parquet",
            &source,
            Some(("groups/g1.parquet", Some(&with_fsno))),
            &[u[1]],
            "r2",
            "g1",
            "g1",
            "r2",
        )
        .expect("plan");
        let out = out.expect("destination");
        assert_eq!(out.roles.descriptive, vec!["site", "fsno", "ware"]);
        assert_eq!(
            out.rows[1].descriptive,
            vec![None, Some("F-8".to_string()), Some("glaze".to_string())]
        );
    }

    #[test]
    fn a_column_with_two_roles_is_refused() {
        let u = [Uuid::now_v7(), Uuid::now_v7()];
        let dest = data("g1", vec![row(u[0], Some(1.0), None)]);
        // `site` measured in the source but descriptive in the destination.
        let mut measured_site = data("g2", vec![row(u[1], Some(2.0), Some(3.0))]);
        measured_site.profile.roles.elemental = vec!["as".to_string(), "site".to_string()];
        measured_site.profile.roles.descriptive = vec![];
        measured_site.rows[0].descriptive = vec![];
        // `anid` (the destination's ID column) descriptive in the source.
        let mut anid_descriptive = data("g3", vec![row(u[1], Some(2.0), Some(3.0))]);
        anid_descriptive.profile.roles.visible_id = "fsno".to_string();
        anid_descriptive.profile.roles.descriptive = vec!["anid".to_string()];
        for source in [measured_site, anid_descriptive] {
            let err = plan_move(
                "groups/g2.parquet",
                &source,
                Some(("groups/g1.parquet", Some(&dest))),
                &[u[1]],
                "r2",
                "g1",
                "g1",
                "r2",
            )
            .expect_err("clash");
            assert!(matches!(err, StoreError::SchemaMismatch { .. }), "{err}");
        }
    }

    #[test]
    fn merge_plan_rejects_duplicate_uuids() {
        let u = [Uuid::now_v7(), Uuid::now_v7()];
        let sources = vec![
            (
                "groups/g1.parquet".to_string(),
                data("g1", vec![row(u[0], Some(1.0), None)]),
            ),
            (
                "groups/g2.parquet".to_string(),
                data(
                    "g2",
                    vec![row(u[0], Some(1.0), None), row(u[1], None, Some(2.0))],
                ),
            ),
        ];
        let err = plan_merge(&sources, 0, "gm", "merged", "r2").expect_err("duplicate uuid");
        assert!(matches!(err, StoreError::Invariant(_)));

        let merged = plan_merge(
            &vec![
                (
                    "groups/g1.parquet".to_string(),
                    data("g1", vec![row(u[0], Some(1.0), None)]),
                ),
                (
                    "groups/g2.parquet".to_string(),
                    data("g2", vec![row(u[1], None, Some(2.0))]),
                ),
            ],
            0,
            "gm",
            "merged",
            "r2",
        )
        .expect("plan");
        assert_eq!(merged.rows.len(), 2);
        assert_eq!(merged.group_id, "gm");
    }
}
