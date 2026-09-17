//! Serde DTOs shared by the Axum HTTP API and the Tauri IPC adapter.
//!
//! Contracts are transport-neutral: both adapters serialize the same types so
//! the web and desktop clients cannot drift apart (Section 1).

use serde::{Deserialize, Serialize};

/// Smoke/health payload returned by `GET /healthz` and the Tauri `app_info`
/// command. Proves one use case flows through both adapters (Phase 1 exit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    /// Application name.
    pub app: String,
    /// Workspace version from Cargo.
    pub version: String,
    /// Adapter reporting the info, e.g. `http` or `tauri`.
    pub transport: String,
    /// Whether the hosted control plane is reachable (always `true` for desktop-local).
    pub ready: bool,
}

/// Transport-neutral error envelope using problem-details-style fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    /// Stable machine-readable error code.
    pub code: String,
    /// Safe user-facing message; diagnostics stay server-side.
    pub message: String,
}

/// Request to parse a source spreadsheet without modifying it (Section 10.2
/// `imports/preview`). Paths are project-relative; the adapter owns the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPreviewRequest {
    /// Project-relative path of the source CSV/TSV.
    pub source: String,
    /// Optional group column; when present the response includes the group
    /// partition summary that a commit with the same column would produce.
    pub group_column: Option<String>,
}

/// One group partition from the preview summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionPreview {
    /// Raw group value (the future `group_name`).
    pub group_name: String,
    /// Rows sharing the value.
    pub row_count: u64,
    /// Deterministic project-relative Parquet path a commit would publish.
    pub suggested_path: String,
}

/// Response describing a loaded source and the default import decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPreviewResponse {
    /// Echo of the project-relative source path.
    pub source: String,
    /// Data rows after `dataLoader` (rowid prepended, incoming rowid dropped).
    pub row_count: u64,
    /// Cleaned column names in frame order.
    pub columns: Vec<String>,
    /// `default_id_column` suggestion (first case-insensitive `anid` match).
    pub id_column: Option<String>,
    /// `default_chem_columns` suggestion (INAA-list matches or all non-ID columns).
    pub elemental_columns: Vec<String>,
    /// Partition summary; empty when the request omitted `group_column`.
    pub partitions: Vec<PartitionPreview>,
}

/// Import value-class policies echoed from `archaeodash_data_io::ImportRecipe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRecipeDto {
    /// Treat zero elemental values as NA.
    pub zero_as_na: bool,
    /// Treat negative elemental values as NA.
    pub negative_as_na: bool,
    /// Replace NA elemental values with zero (applied last).
    pub na_as_zero: bool,
    /// Optional `[blank]`-style label for empty non-elemental cells.
    pub blank_non_element_label: Option<String>,
}

/// Request to publish one validated group Parquet file per group value
/// (Section 10.2 `imports/commit`, local Phase-2 form: paths are
/// project-relative and the caller already chose the file location).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportCommitRequest {
    /// Project-relative path of the source CSV/TSV.
    pub source: String,
    /// Group column; blank values are rejected.
    pub group_column: String,
    /// Visible-ID column; defaults to the `anid` suggestion.
    pub visible_id_column: Option<String>,
    /// Elemental columns; defaults to `default_chem_columns`.
    pub elemental_columns: Option<Vec<String>>,
    /// Value-class policies; defaults to all-off.
    pub recipe: Option<ImportRecipeDto>,
    /// Project-relative destination directory for group files; default `groups`.
    pub destination_dir: Option<String>,
}

/// One published group file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedGroup {
    /// Stable group identifier (sanitized group name for first imports).
    pub group_id: String,
    /// Raw group value from the source.
    pub group_name: String,
    /// Project-relative Parquet path that was written.
    pub path: String,
    /// Analytical units in the file.
    pub row_count: u64,
    /// Initial revision identifier (`rev-1`).
    pub revision_id: String,
    /// Measured-elemental checksum stored in the profile.
    pub measured_elemental_checksum: String,
}

/// Response listing every group file a commit published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportCommitResponse {
    /// Echo of the project-relative source path.
    pub source: String,
    /// One entry per published group Parquet file, in partition order.
    pub groups: Vec<CommittedGroup>,
}

/// Summary of one group Parquet file (Section 10.2 `GET /groups/{id}` shape,
/// local Phase-2 form: paths are project-relative).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSummary {
    /// Project-relative path of the group file.
    pub path: String,
    pub group_id: String,
    pub group_name: String,
    pub revision_id: String,
    pub row_count: u64,
    pub source_path: Option<String>,
    pub source_sha256: Option<String>,
    pub elemental_columns: Vec<String>,
    pub descriptive_columns: Vec<String>,
}

/// One discovered Parquet candidate with its readiness state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupCandidate {
    /// Project-relative path.
    pub path: String,
    /// `true` when the profile is present and the file is ready to add.
    pub ready: bool,
    /// Summary when ready.
    pub group: Option<GroupSummary>,
}

/// The kind of analytical-unit transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferAction {
    /// Units leave the source and join the destination.
    Move,
    /// Units keep their UUIDs and appear in both groups.
    Copy,
}

/// Request to move/copy selected analytical units between groups by hidden
/// UUID (Section 10.2 `POST /groups/transfer-units`). Optimistic concurrency:
/// `expected_source_revision` must match the live source revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferUnitsRequest {
    pub action: TransferAction,
    /// Project-relative source group path.
    pub source_path: String,
    /// Project-relative destination path; may name an existing group or a
    /// not-yet-existing file (created from `destination_group_name`).
    pub destination_path: String,
    /// Required when the destination file does not exist yet.
    pub destination_group_name: Option<String>,
    /// Hidden analytical UUIDs to transfer.
    pub selected_uuids: Vec<String>,
    /// Source revision the caller last read.
    pub expected_source_revision: String,
}

/// Request to merge whole groups into one target; sources after the target
/// are removed (Section 10.2 `POST /groups/merge`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeGroupsRequest {
    /// Project-relative paths, two or more; the first is the merge target.
    pub sources: Vec<String>,
    /// New group name/id for the merged target.
    pub new_group_name: String,
}

/// Request to delete one group file from the project (Section 10.2
/// `DELETE /groups/{id}`, local Phase-2 form: the local store keeps no
/// manifest/active selection, so delete is the whole operation; "unload"
/// arrives with the workspace/manifest layer). Destructive: requires
/// exact-path confirmation and the revision the caller last read
/// (Section 10.4: destructive file commands are never implied).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteGroupRequest {
    /// Project-relative path of the group file to delete.
    pub path: String,
    /// Revision the caller last read; a mismatch is a conflict.
    pub expected_revision: String,
    /// Must equal `path` exactly; the client confirms the exact file.
    pub confirm_path: String,
}

/// Result of one journaled group transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionResponse {
    /// Transaction id from the Section 6.8 journal.
    pub transaction_id: String,
    /// Snake-case action, e.g. `move_units`.
    pub action: String,
    /// Published successor files with their new revisions.
    pub outputs: Vec<GroupSummary>,
    /// Group files that ceased to exist.
    pub deleted_paths: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_round_trips() {
        let info = AppInfo {
            app: "archaeodash".into(),
            version: "0.1.0".into(),
            transport: "http".into(),
            ready: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(serde_json::from_str::<AppInfo>(&json).unwrap(), info);
    }

    #[test]
    fn import_dtos_round_trip() {
        let preview = ImportPreviewResponse {
            source: "sources/INAA_test.csv".into(),
            row_count: 307,
            columns: vec!["rowid".into(), "anid".into(), "as".into()],
            id_column: Some("anid".into()),
            elemental_columns: vec!["as".into()],
            partitions: vec![PartitionPreview {
                group_name: "Baca".into(),
                row_count: 12,
                suggested_path: "groups/Baca.parquet".into(),
            }],
        };
        let json = serde_json::to_string(&preview).unwrap();
        assert_eq!(
            serde_json::from_str::<ImportPreviewResponse>(&json).unwrap(),
            preview
        );

        let commit = ImportCommitRequest {
            source: "sources/INAA_test.csv".into(),
            group_column: "Site".into(),
            visible_id_column: None,
            elemental_columns: None,
            recipe: Some(ImportRecipeDto {
                zero_as_na: true,
                negative_as_na: false,
                na_as_zero: false,
                blank_non_element_label: Some("[blank]".into()),
            }),
            destination_dir: None,
        };
        let json = serde_json::to_string(&commit).unwrap();
        assert_eq!(
            serde_json::from_str::<ImportCommitRequest>(&json).unwrap(),
            commit
        );
    }

    #[test]
    fn group_operation_dtos_round_trip() {
        let transfer = TransferUnitsRequest {
            action: TransferAction::Move,
            source_path: "groups/a.parquet".into(),
            destination_path: "groups/b.parquet".into(),
            destination_group_name: Some("b".into()),
            selected_uuids: vec!["01900000-0000-7000-8000-000000000001".into()],
            expected_source_revision: "rev-1".into(),
        };
        let json = serde_json::to_string(&transfer).unwrap();
        assert!(json.contains("\"move\""));
        assert_eq!(
            serde_json::from_str::<TransferUnitsRequest>(&json).unwrap(),
            transfer
        );

        let merge = MergeGroupsRequest {
            sources: vec!["groups/a.parquet".into(), "groups/b.parquet".into()],
            new_group_name: "merged".into(),
        };
        let json = serde_json::to_string(&merge).unwrap();
        assert_eq!(
            serde_json::from_str::<MergeGroupsRequest>(&json).unwrap(),
            merge
        );

        let delete = DeleteGroupRequest {
            path: "groups/a.parquet".into(),
            expected_revision: "rev-1".into(),
            confirm_path: "groups/a.parquet".into(),
        };
        let json = serde_json::to_string(&delete).unwrap();
        assert_eq!(
            serde_json::from_str::<DeleteGroupRequest>(&json).unwrap(),
            delete
        );
    }
}
