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

use archaeodash_data_io::{GroupFileData, GroupProfile};

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
    /// Roles carried over from the destination when one exists (schema
    /// compatibility is enforced by plan functions), else from the source.
    pub roles: archaeodash_data_io::ColumnRoles,
    /// Import recipe carried from the source group.
    pub recipe: archaeodash_data_io::ImportRecipe,
    pub source_path: Option<String>,
    pub source_sha256: Option<String>,
}

/// Confirms two group files can exchange rows: identical elemental and
/// descriptive role columns (Section 6.8 "destination schema/units
/// compatibility").
pub fn assert_schema_compatible(
    left_path: &str,
    left: &GroupFileData,
    right_path: &str,
    right: &GroupFileData,
) -> Result<(), StoreError> {
    if left.profile.roles.elemental != right.profile.roles.elemental {
        return Err(StoreError::SchemaMismatch {
            left: left_path.to_string(),
            right: right_path.to_string(),
            detail: "elemental columns differ".to_string(),
        });
    }
    if left.profile.roles.descriptive != right.profile.roles.descriptive {
        return Err(StoreError::SchemaMismatch {
            left: left_path.to_string(),
            right: right_path.to_string(),
            detail: "descriptive columns differ".to_string(),
        });
    }
    if left.profile.roles.visible_id != right.profile.roles.visible_id {
        return Err(StoreError::SchemaMismatch {
            left: left_path.to_string(),
            right: right_path.to_string(),
            detail: "visible id column differs".to_string(),
        });
    }
    Ok(())
}

/// Verifies measured values are unchanged for every preserved UUID: the
/// `(uuid, col, value|null)` tuples of the selected rows must appear
/// identically in the successor output (Section 6.8 invariant).
pub fn assert_measured_values_preserved(
    selected: &[Uuid],
    before: &GroupFileData,
    after_rows: &[archaeodash_data_io::GroupRow],
) -> Result<(), StoreError> {
    let tuple = |row: &archaeodash_data_io::GroupRow| -> Vec<(Uuid, usize, Option<u64>)> {
        row.elemental
            .iter()
            .enumerate()
            .map(|(col, v)| (row.uuid, col, v.map(f64::to_bits)))
            .collect()
    };
    let before_by_uuid: std::collections::HashMap<Uuid, &archaeodash_data_io::GroupRow> =
        before.rows.iter().map(|r| (r.uuid, r)).collect();
    let selected_set: HashSet<Uuid> = selected.iter().copied().collect();
    for row in after_rows {
        if !selected_set.contains(&row.uuid) {
            continue;
        }
        let original = before_by_uuid.get(&row.uuid).ok_or_else(|| {
            StoreError::Invariant(format!("unexpected uuid {} in successor output", row.uuid))
        })?;
        if tuple(original) != tuple(row) {
            return Err(StoreError::Invariant(format!(
                "measured values changed for uuid {}",
                row.uuid
            )));
        }
    }
    Ok(())
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
    if let Some(dest) = dest {
        assert_schema_compatible(source_path, source, dest_path, dest)?;
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
        .cloned()
        .collect();
    if moving.is_empty() {
        return Err(StoreError::Invariant("move selected no rows".to_string()));
    }
    assert_measured_values_preserved(selected_uuids, source, &moving)?;

    let mut dest_rows = dest.map(|d| d.rows.clone()).unwrap_or_default();
    dest_rows.extend(moving);
    let dest_out = PlannedOutput {
        path: dest_path.to_string(),
        group_id: destination_group_id.to_string(),
        group_name: destination_group_name.to_string(),
        revision_id: new_destination_revision.to_string(),
        rows: dest_rows,
        roles: match dest {
            Some(d) => d.profile.roles.clone(),
            None => source.profile.roles.clone(),
        },
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
    let moving: Vec<archaeodash_data_io::GroupRow> = source
        .rows
        .iter()
        .filter(|r| selected.contains(&r.uuid))
        .cloned()
        .collect();
    if moving.is_empty() {
        return Err(StoreError::Invariant("copy selected no rows".to_string()));
    }
    assert_measured_values_preserved(selected_uuids, source, &moving)?;

    let dest_rows = match destination {
        Some(dest) => {
            assert_schema_compatible(source_path, source, destination_path, dest)?;
            let dest_uuids: HashSet<Uuid> = dest.rows.iter().map(|r| r.uuid).collect();
            let collisions: Vec<_> = selected.intersection(&dest_uuids).collect();
            if !collisions.is_empty() {
                return Err(StoreError::Invariant(format!(
                    "{} selected uuid(s) already present in destination {}",
                    collisions.len(),
                    destination_path
                )));
            }
            let mut rows = dest.rows.clone();
            rows.extend(moving);
            rows
        }
        None => moving,
    };
    let (roles, recipe, source_path_out, source_sha256) = match destination {
        Some(dest) => (
            dest.profile.roles.clone(),
            dest.profile.import_recipe.clone(),
            dest.profile.source_path.clone(),
            dest.profile.source_sha256.clone(),
        ),
        None => (
            source.profile.roles.clone(),
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
    for (path, group) in sources {
        assert_schema_compatible(target_path, target, path, group)?;
    }
    let mut seen: HashSet<Uuid> = HashSet::new();
    let mut rows = Vec::new();
    for (path, group) in sources {
        for row in &group.rows {
            if !seen.insert(row.uuid) {
                return Err(StoreError::Invariant(format!(
                    "duplicate analytical uuid {} across merge sources ({path})",
                    row.uuid
                )));
            }
            rows.push(row.clone());
        }
    }
    let (roles, recipe, source_path, source_sha256) = {
        let p = &target.profile;
        (
            p.roles.clone(),
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

    #[test]
    fn merge_plan_rejects_duplicate_uuids_and_schema_mismatch() {
        let u = [Uuid::now_v7(), Uuid::now_v7()];
        let mut dup = data("g2", vec![row(u[0], Some(4.0), None)]);
        dup.profile.roles.elemental.push("zn".to_string());
        let sources = vec![
            (
                "groups/g1.parquet".to_string(),
                data("g1", vec![row(u[0], Some(1.0), None)]),
            ),
            ("groups/g2.parquet".to_string(), dup),
        ];
        let err = plan_merge(&sources, 0, "gm", "merged", "r2").expect_err("schema mismatch");
        assert!(matches!(err, StoreError::SchemaMismatch { .. }));

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
