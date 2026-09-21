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

/// One hidden-UUID-addressed descriptive edit (Section 13 `R/updateCurrent.R`):
/// set one descriptive cell of the row with the given analytical UUID; `None`
/// clears the cell to empty/NA. Elemental and identity columns are rejected
/// by the use case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptiveEdit {
    /// Hidden analytical UUID of the edited row (never the visible id).
    pub analytical_uuid: String,
    /// Descriptive column name, in `roles.descriptive` order.
    pub column: String,
    /// New cell value; `None` clears it (legacy NA).
    pub value: Option<String>,
}

/// `PATCH /groups/descriptive-values` request: batch descriptive edits on
/// one group file guarded by the revision the caller last read. Elemental
/// columns are locked (Section 4 Phase 4: "descriptive-edit table with
/// elemental columns locked").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchDescriptiveValuesRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Revision the caller last read; a mismatch is a conflict.
    pub expected_revision: String,
    /// Edits applied in order; one transaction for the whole batch.
    pub edits: Vec<DescriptiveEdit>,
}

/// `POST /groups/duplicate` request: duplicate one whole group to a new
/// path. UUIDs and source lineage are preserved by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateGroupRequest {
    /// Project-relative path of the group file to duplicate.
    pub source_path: String,
    /// Revision the caller last read; a mismatch is a conflict.
    pub expected_revision: String,
    /// Name of the new group; the sanitized id derives the destination path
    /// when `destination_path` is omitted.
    pub new_group_name: String,
    /// Optional explicit project-relative destination path; must not exist.
    #[serde(default)]
    pub destination_path: Option<String>,
    /// Preserve analytical UUIDs and source lineage (Section 10.2 default).
    #[serde(default = "default_true")]
    pub preserve_uuids: bool,
}

fn default_true() -> bool {
    true
}

/// Metadata for one uploaded source file (Section 10.2 `GET /files/{id}`,
/// local Phase-2 form: the quarantine record under `.archaeodash/quarantine`
/// is the file catalog until the hosted control plane lands in Phase 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedFile {
    /// Opaque file ID (UUIDv7); the only handle metadata/download/delete take.
    pub file_id: String,
    /// Project-relative logical path the file was uploaded to.
    pub path: String,
    /// Byte size of the stored content.
    pub size_bytes: u64,
    /// SHA-256 hex digest of the stored content.
    pub sha256: String,
    /// Format from the extension allowlist: `csv`, `tsv`, or `xlsx`.
    pub format: String,
    /// `parsed` (CSV parse check passed), `parse_failed`, or `deferred`
    /// (no tested parser yet for the format in the Rust port).
    pub parse_state: String,
    /// Parse failure message when `parse_state` is `parse_failed`.
    pub parse_error: Option<String>,
    /// Soft-delete tombstone: bytes moved to quarantine trash, record kept.
    pub deleted: bool,
}

/// Desktop upload payload: target logical path plus raw bytes (Section 10.2
/// `POST /projects/{id}/files`; the HTTP adapter takes the bytes as the raw
/// request body instead).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileUploadRequest {
    /// Project-relative logical path for the uploaded source.
    pub path: String,
    /// Raw file bytes.
    pub content: Vec<u8>,
}

/// Desktop download payload: metadata plus raw bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDownload {
    pub metadata: StagedFile,
    pub content: Vec<u8>,
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

/// Base transform applied to the measured elemental matrix before ratios
/// (Section 8.4). Wire names match the legacy R values exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransformMethod {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "log")]
    Log,
    #[serde(rename = "log10")]
    Log10,
    #[serde(rename = "zScore")]
    ZScore,
}

/// Imputation method (Section 8.3). `pmm`, `midastouch`, and `rf` stay
/// behind the experimental-parity gate until their oracle fixtures pass;
/// the application layer rejects applying them for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImputationMethod {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "pmm")]
    Pmm,
    #[serde(rename = "midastouch")]
    MidasTouch,
    #[serde(rename = "rf")]
    Rf,
}

/// One ratio definition (Section 8.2). `output_name` defaults to the
/// deterministic `numerator_denominator` form when omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatioSpecDto {
    pub output_name: Option<String>,
    pub numerator: String,
    pub denominator: String,
}

/// Whether generated ratio columns are appended to the elemental matrix or
/// replace it (legacy `ratioMode` `append`/`only`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RatioMode {
    Append,
    Only,
}

/// A named, persisted transformation definition (Section 8.2): column
/// selections plus method configuration only — never calculated values
/// (Section 5: derived values are recomputed on demand).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformationDefinition {
    /// User-facing name; unique per project, sanitized for storage.
    pub name: String,
    pub transform_method: TransformMethod,
    /// Requires a visible/replayable seed when not `none` (Section 8.3).
    pub imputation_method: ImputationMethod,
    pub imputation_seed: Option<u64>,
    /// Measured elemental columns the transform reads.
    pub elemental_columns: Vec<String>,
    /// Descriptive columns carried through as metadata.
    pub descriptive_columns: Vec<String>,
    /// Group column for selection controls.
    pub group_column: Option<String>,
    /// Ratio specs applied in order after the base transform.
    pub ratios: Vec<RatioSpecDto>,
    pub ratio_mode: RatioMode,
}

/// Listing entry for one saved transformation definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformationSummary {
    pub name: String,
    /// Unix seconds of the last save.
    pub created_at_unix_secs: u64,
    pub transform_method: TransformMethod,
    pub imputation_method: ImputationMethod,
    pub ratio_count: usize,
}

/// `POST /transformations` request: save (upsert by name) one definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveTransformationRequest {
    pub definition: TransformationDefinition,
}

/// Save result: the stored definition plus whether it replaced an existing one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveTransformationResponse {
    pub definition: TransformationDefinition,
    pub replaced: bool,
}

/// `GET /transformations` response: summaries sorted by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformationListResponse {
    pub transformations: Vec<TransformationSummary>,
}

/// Request for one-to-one or Cartesian batch ratio generation (Section 8.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRatioRequest {
    pub numerators: Vec<String>,
    pub denominators: Vec<String>,
    /// `one_to_one` pairs by index (lengths must match); `cartesian` pairs
    /// every combination.
    pub mode: BatchRatioMode,
}

/// Batch generation pairing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchRatioMode {
    OneToOne,
    Cartesian,
}

/// Ephemeral apply result: transformed values computed on demand from one
/// group file, never persisted (Section 5 storage invariant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppliedTransformation {
    /// Project-relative group path the transform ran against.
    pub path: String,
    /// Revision the group file was at when applied.
    pub revision_id: String,
    /// Output column names: elemental selection (or ratios only), then
    /// generated ratio columns in spec order.
    pub columns: Vec<String>,
    /// Row values in `columns` order; `null` marks NA.
    pub rows: Vec<Vec<Option<f64>>>,
    /// Cells made non-finite by a log transform, then zeroed (Section 8.4
    /// warning count); always 0 for `none`/`zScore`.
    pub non_finite_to_zero: u64,
}

/// `POST /transformations/apply` request: run one definition (inline or
/// previously saved by name) against a group file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyTransformationRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Inline definition to apply.
    pub definition: TransformationDefinition,
}

/// `POST /ordination/pca` request: prcomp-parity PCA over one group file
/// (Section 8.5). Ordination results are ephemeral and never persisted
/// (Section 5 storage invariant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PcaRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Columns to ordinate: measured elemental names, or post-transform
    /// output names when `transformation` is present.
    pub columns: Vec<String>,
    /// `prcomp` `scale.` flag: divide centered columns by their sample sd.
    #[serde(default)]
    pub scale: bool,
    /// Optional transformation applied to the group matrix first (Section 8.2
    /// definition, applied on demand from measured values).
    pub transformation: Option<TransformationDefinition>,
}

/// PCA result: `prcomp` parity outputs with deterministic component signs
/// (each component's largest-|value| loading is positive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PcaResponse {
    pub path: String,
    /// Revision the group file was at when computed.
    pub revision_id: String,
    /// Input column names, in request order.
    pub column_names: Vec<String>,
    /// Component names `PC1..PCk`.
    pub score_names: Vec<String>,
    /// Component standard deviations `d / sqrt(n - 1)`.
    pub sdev: Vec<f64>,
    /// `sdev^2` shares of total variance.
    pub explained_variance: Vec<f64>,
    /// Running sum of `explained_variance`.
    pub cumulative_variance: Vec<f64>,
    /// Column means subtracted before decomposition.
    pub center: Vec<f64>,
    /// Column sample sds when `scale` was requested, else `None`.
    pub scale: Option<Vec<f64>>,
    /// Loadings, variable-major rows (`rotation[v][k]` like R's matrix).
    pub rotation: Vec<Vec<f64>>,
    /// Scores, row-major (`scores[i][k]` like R's `pca$x`).
    pub scores: Vec<Vec<f64>>,
}

/// `POST /ordination/lda` request: `MASS::lda` moment-method parity over one
/// group file, with the legacy three-group minimum gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LdaRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Columns to ordinate: measured elemental names, or post-transform
    /// output names when `transformation` is present.
    pub columns: Vec<String>,
    /// Descriptive column holding the grouping factor.
    pub group_column: String,
    /// Optional transformation applied to the group matrix first.
    pub transformation: Option<TransformationDefinition>,
}

/// LDA result: priors, group means, discriminant scaling, singular values,
/// and scores with deterministic signs (largest-|value| scaling entry per
/// discriminant is positive).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LdaResponse {
    pub path: String,
    pub revision_id: String,
    /// Input column names, in request order.
    pub column_names: Vec<String>,
    /// Factor levels in sorted order; `prior`, `counts`, and `means` rows
    /// follow this order.
    pub levels: Vec<String>,
    /// Level proportions `counts / n`.
    pub prior: Vec<f64>,
    /// Row counts per level.
    pub counts: Vec<u64>,
    /// Group means, level-major rows (`means[g][j]`).
    pub means: Vec<Vec<f64>>,
    /// Discriminant loadings, variable-major rows (`scaling[v][k]`).
    pub scaling: Vec<Vec<f64>>,
    /// Stage-2 singular values kept (`svd[1:rank]`).
    pub svd: Vec<f64>,
    /// Discriminant names `LD1..LDk`.
    pub score_names: Vec<String>,
    /// Scores, row-major, each column re-centered to mean zero (the legacy
    /// capture convention).
    pub scores: Vec<Vec<f64>>,
    /// Non-fatal legacy warnings (collinearity downgrades the rank).
    pub warnings: Vec<String>,
}

/// `POST /ordination/umap` request: legacy `umap::umap(method = "naive")`
/// parity over one group file (Section 8.6, class D). Results are ephemeral
/// and never persisted (Section 5 storage invariant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UmapRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Columns to ordinate: measured elemental names, or post-transform
    /// output names when `transformation` is present.
    pub columns: Vec<String>,
    /// Optional transformation applied to the group matrix first.
    pub transformation: Option<TransformationDefinition>,
    /// Deterministic RNG seed replacing the legacy unseeded global stream
    /// (class-D fixed-seed golden). Defaults to the golden-fixture seed.
    #[serde(default)]
    pub seed: Option<u64>,
}

/// UMAP result: seeded, deterministic `V1`/`V2` embedding with the resolved
/// legacy configuration echoed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UmapResponse {
    pub path: String,
    /// Revision the group file was at when computed.
    pub revision_id: String,
    /// Input column names, in request order.
    pub column_names: Vec<String>,
    /// Embedding dimension names `V1..Vd` (legacy `umap$layout` names).
    pub score_names: Vec<String>,
    /// Embedding, row-major (`embedding[i][k]` like R's `umap$layout`).
    pub embedding: Vec<Vec<f64>>,
    /// Seed used for the deterministic stream.
    pub seed: u64,
    /// Legacy `n_neighbors` (brute-force neighbor count per row).
    pub n_neighbors: usize,
    /// Legacy `n_epochs` (SGD epochs).
    pub n_epochs: usize,
    /// Fitted `a` curve parameter (`find.ab.params(spread, min_dist)`).
    pub a: f64,
    /// Fitted `b` curve parameter.
    pub b: f64,
    /// Non-fatal legacy warnings (spectral-init fallback).
    pub warnings: Vec<String>,
}

/// `POST /explore/missing-profile` request: `profile_missing` band summaries
/// over one group file's columns (Section 8 procedure 12, class E). Results
/// are ephemeral and never persisted (Section 5 storage invariant).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreMissingProfileRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Columns to profile: measured elemental names, or post-transform
    /// output names when `transformation` is present.
    pub columns: Vec<String>,
    /// Optional transformation applied to the group matrix first.
    pub transformation: Option<TransformationDefinition>,
}

/// One `profile_missing` row: legacy `cut` band label ("Good", "OK", "Bad",
/// "Remove"); rows are ordered by descending missing count with ties in
/// column order (`order(-rank(num_missing))`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissingProfileRow {
    pub feature: String,
    pub num_missing: u64,
    pub pct_missing: f64,
    pub band: String,
}

/// `POST /explore/missing-profile` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreMissingProfileResponse {
    pub path: String,
    /// Revision the group file was at when computed.
    pub revision_id: String,
    pub rows: Vec<MissingProfileRow>,
}

/// `POST /explore/histogram` request: `hist.default` breakpoints and counts
/// for one column (`breaks = bins`, the legacy default of 30).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreHistogramRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Single elemental (or post-transform) column to bin.
    pub column: String,
    /// `hist(..., breaks = bins)` suggestion; defaults to the legacy 30.
    #[serde(default = "default_histogram_bins")]
    pub bins: u32,
    /// Optional transformation applied to the group matrix first.
    pub transformation: Option<TransformationDefinition>,
}

fn default_histogram_bins() -> u32 {
    30
}

/// `POST /explore/histogram` response: `pretty` breakpoints (length
/// `counts.len() + 1`) and right-closed `include.lowest` bin counts over the
/// finite values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreHistogramResponse {
    pub path: String,
    pub revision_id: String,
    pub column: String,
    pub breaks: Vec<f64>,
    pub counts: Vec<u64>,
}

/// `POST /explore/crosstab` request: legacy `compute_crosstab_summary`.
/// `count` groups by both columns as text; `mean`/`median`/`sd` coerce the
/// value column numerically (`as.numeric(as.character(...))`, `na.rm = TRUE`)
/// and group by the group column only, rounding to two decimals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreCrosstabRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Descriptive (text) grouping column.
    pub group_column: String,
    /// Second column: raw text for `count`, numeric for the summaries.
    pub value_column: String,
    /// One of `count`, `mean`, `median`, `sd` (legacy `summary_method`).
    pub summary_method: String,
}

/// One `count` crosstab row: the distinct (group, value) text pair and its
/// count, keys ascending with `NA` groups last (dplyr group order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrosstabCountRow {
    pub group: Option<String>,
    pub value: Option<String>,
    pub count: u64,
}

/// One `mean`/`median`/`sd` row; `result` is `null` when the group has no
/// numeric values (R rounds, then jsonlite writes NA as null).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrosstabSummaryRow {
    pub group: Option<String>,
    pub result: Option<f64>,
}

/// Crosstab rows, tagged by summary method: `count` rows group by both
/// columns; summary rows carry the legacy `result-<value_column>` name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CrosstabRows {
    Count {
        rows: Vec<CrosstabCountRow>,
    },
    Summary {
        result_column: String,
        rows: Vec<CrosstabSummaryRow>,
    },
}

/// `POST /explore/crosstab` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreCrosstabResponse {
    pub path: String,
    pub revision_id: String,
    /// Echo of the requested `summary_method`.
    pub summary_method: String,
    #[serde(flatten)]
    pub rows: CrosstabRows,
}

/// `POST /explore/compositional-profile` request: the `comp.profile`
/// `pivot_longer` long table over one group file, optionally colored by a
/// descriptive group column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreCompositionalProfileRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Elemental (or post-transform) columns, in plot order.
    pub columns: Vec<String>,
    /// Optional descriptive column for line coloring; `rep(groups, each =
    /// ncol)` aligns one label per long row.
    pub group_column: Option<String>,
    /// Optional transformation applied to the group matrix first.
    pub transformation: Option<TransformationDefinition>,
}

/// One long-table row: 1-based positional `rowid`, element name, value
/// (`null` for NA), and the optional group label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionalProfileRow {
    pub rowid: u64,
    pub element: String,
    pub value: Option<f64>,
    pub group_label: Option<String>,
}

/// `POST /explore/compositional-profile` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExploreCompositionalProfileResponse {
    pub path: String,
    pub revision_id: String,
    pub rows: Vec<CompositionalProfileRow>,
}

/// `POST /exports/measured-data` request: the measured chemical frame of one
/// group file as CSV (Section 7.3; legacy `rvals$selectedData`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportMeasuredDataRequest {
    /// Project-relative group file path.
    pub path: String,
    /// `true` disables the Section 7.3 formula-injection guard and reproduces
    /// the byte-exact legacy `fwrite` output for the measured-data case.
    #[serde(default)]
    pub raw_text: bool,
}

/// `POST /exports/transformed` request: the explicitly computed transformed
/// result of one definition over one group file as CSV (Section 7.3).
/// Calculated values are ephemeral and never persisted (Section 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportTransformedRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Inline definition applied on demand before export.
    pub definition: TransformationDefinition,
    /// `true` disables the Section 7.3 formula-injection guard.
    #[serde(default)]
    pub raw_text: bool,
}

/// `POST /exports/pca-scores` request: the computed PCA score frame as CSV.
/// The legacy export read the nonexistent `rvals$pcaData`; Section 3.2
/// corrects it to export the computed `pcadf` equivalent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportPcaScoresRequest {
    /// Project-relative group file path.
    pub path: String,
    /// Columns to ordinate: measured elemental names, or post-transform
    /// output names when `transformation` is present.
    pub columns: Vec<String>,
    /// `prcomp` `scale.` flag, mirroring `PcaRequest`.
    #[serde(default)]
    pub scale: bool,
    /// Optional transformation applied to the group matrix first.
    pub transformation: Option<TransformationDefinition>,
}

/// One ephemeral CSV export result returned to the caller (Section 7.3).
/// The desktop client saves `content` through a native save dialog using
/// `file_name` as the default; the hosted streaming-download route arrives
/// with the Phase 7 job surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportResult {
    /// Suggested download filename (legacy `saveexportTab.R` name rule).
    pub file_name: String,
    /// MIME type, always `text/csv` for this slice (XLSX/TSV are deferred
    /// per the Section 7.1 capability matrix).
    pub media_type: String,
    /// Full CSV text, header row first, `\n` line endings.
    pub content: String,
}

/// Allowlisted preference keys (Section 10.1: typed allowlisted preference
/// keys only; Section 6.5: typed JSON values, never free-form rows). The
/// legacy app persisted exactly two fields — `themePreference`
/// (`R/userPreferences.R`, values `simple`/`light`/`dark`) and
/// `lastOpenedDataset` (`DataLoader.R`) — and Section 9.4 adds the Explore
/// table UI prefs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreferenceKey {
    /// UI theme; legacy `simple`/`light`/`dark` values (Section 9.3).
    Theme,
    /// Last-opened dataset name, re-validated against live groups on read
    /// (legacy `lastOpenedDataset` selector-default semantics).
    LastOpenedDataset,
    /// Explore table column-visibility map, column name to visible flag
    /// (Section 9.4).
    ColumnVisibility,
    /// Explore table compact-mode flag (Section 9.4).
    CompactMode,
}

impl PreferenceKey {
    /// Stable storage/wire name for the key.
    pub fn as_str(self) -> &'static str {
        match self {
            PreferenceKey::Theme => "theme",
            PreferenceKey::LastOpenedDataset => "lastOpenedDataset",
            PreferenceKey::ColumnVisibility => "columnVisibility",
            PreferenceKey::CompactMode => "compactMode",
        }
    }
}

impl TryFrom<&str> for PreferenceKey {
    type Error = ();

    /// Parses a storage/wire key name; unknown names are rejected (the
    /// typed allowlist, Section 10.1).
    fn try_from(name: &str) -> Result<Self, Self::Error> {
        match name {
            "theme" => Ok(PreferenceKey::Theme),
            "lastOpenedDataset" => Ok(PreferenceKey::LastOpenedDataset),
            "columnVisibility" => Ok(PreferenceKey::ColumnVisibility),
            "compactMode" => Ok(PreferenceKey::CompactMode),
            _ => Err(()),
        }
    }
}

/// One persisted preference: an allowlisted key plus its typed JSON value.
/// Value-shape validation per key happens in the application service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreferenceEntry {
    /// Allowlisted preference key.
    pub key: PreferenceKey,
    /// JSON value whose shape depends on the key.
    pub value: serde_json::Value,
}

/// `GET /preferences` / desktop `preferences_get` response: every stored
/// preference (absent keys simply do not appear; readers apply defaults).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GetPreferencesResponse {
    /// All stored preference entries.
    pub preferences: Vec<PreferenceEntry>,
}

/// `PUT /preferences` / desktop `preferences_set` request: upsert one
/// allowlisted preference (legacy `write_user_preference_safe` upsert
/// semantics, typed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PutPreferenceRequest {
    /// Allowlisted preference key.
    pub key: PreferenceKey,
    /// JSON value; must match the key's documented shape.
    pub value: serde_json::Value,
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

        let staged = StagedFile {
            file_id: "01900000-0000-7000-8000-00000000000f".into(),
            path: "sources/INAA_test.csv".into(),
            size_bytes: 128,
            sha256: "abc123".into(),
            format: "csv".into(),
            parse_state: "parsed".into(),
            parse_error: None,
            deleted: false,
        };
        let json = serde_json::to_string(&staged).unwrap();
        assert_eq!(serde_json::from_str::<StagedFile>(&json).unwrap(), staged);

        let upload = FileUploadRequest {
            path: "sources/INAA_test.csv".into(),
            content: b"anid,Site\nA1,Baca\n".to_vec(),
        };
        let json = serde_json::to_string(&upload).unwrap();
        assert_eq!(
            serde_json::from_str::<FileUploadRequest>(&json).unwrap(),
            upload
        );

        let download = FileDownload {
            metadata: staged.clone(),
            content: upload.content,
        };
        let json = serde_json::to_string(&download).unwrap();
        assert_eq!(
            serde_json::from_str::<FileDownload>(&json).unwrap(),
            download
        );
    }

    #[test]
    fn transformation_dtos_round_trip() {
        let definition = TransformationDefinition {
            name: "log_ratio_set".into(),
            transform_method: TransformMethod::Log10,
            imputation_method: ImputationMethod::None,
            imputation_seed: None,
            elemental_columns: vec!["as".into(), "fe".into()],
            descriptive_columns: vec!["Site".into()],
            group_column: Some("Site".into()),
            ratios: vec![RatioSpecDto {
                output_name: None,
                numerator: "as".into(),
                denominator: "fe".into(),
            }],
            ratio_mode: RatioMode::Append,
        };
        let json = serde_json::to_string(&definition).unwrap();
        assert_eq!(
            serde_json::from_str::<TransformationDefinition>(&json).unwrap(),
            definition
        );
        // Legacy-compatible wire names for the method enums.
        assert!(json.contains("\"log10\""));
        assert!(
            serde_json::from_str::<TransformMethod>("\"zScore\"").unwrap()
                == TransformMethod::ZScore
        );
        assert_eq!(
            serde_json::to_string(&TransformMethod::ZScore).unwrap(),
            "\"zScore\""
        );
        assert!(
            serde_json::from_str::<ImputationMethod>("\"midastouch\"").unwrap()
                == ImputationMethod::MidasTouch
        );

        let apply = ApplyTransformationRequest {
            path: "groups/Baca.parquet".into(),
            definition: definition.clone(),
        };
        let json = serde_json::to_string(&apply).unwrap();
        assert_eq!(
            serde_json::from_str::<ApplyTransformationRequest>(&json).unwrap(),
            apply
        );

        let applied = AppliedTransformation {
            path: "groups/Baca.parquet".into(),
            revision_id: "rev-1".into(),
            columns: vec!["as".into(), "as_fe".into()],
            rows: vec![vec![Some(1.5), Some(0.75)], vec![None, None]],
            non_finite_to_zero: 0,
        };
        let json = serde_json::to_string(&applied).unwrap();
        assert_eq!(
            serde_json::from_str::<AppliedTransformation>(&json).unwrap(),
            applied
        );

        let batch = BatchRatioRequest {
            numerators: vec!["as".into()],
            denominators: vec!["fe".into()],
            mode: BatchRatioMode::Cartesian,
        };
        let json = serde_json::to_string(&batch).unwrap();
        assert_eq!(
            serde_json::from_str::<BatchRatioRequest>(&json).unwrap(),
            batch
        );
    }

    #[test]
    fn ordination_dtos_round_trip() {
        let pca_request = PcaRequest {
            path: "groups/Baca.parquet".into(),
            columns: vec!["as".into(), "fe".into()],
            scale: false,
            transformation: None,
        };
        let json = serde_json::to_string(&pca_request).unwrap();
        assert_eq!(
            serde_json::from_str::<PcaRequest>(&json).unwrap(),
            pca_request
        );
        // `scale` defaults to false on the wire.
        assert_eq!(
            serde_json::from_str::<PcaRequest>(
                "{\"path\":\"groups/Baca.parquet\",\"columns\":[\"as\"]}"
            )
            .unwrap()
            .scale,
            false
        );

        let pca_response = PcaResponse {
            path: "groups/Baca.parquet".into(),
            revision_id: "rev-1".into(),
            column_names: vec!["as".into()],
            score_names: vec!["PC1".into()],
            sdev: vec![1.5],
            explained_variance: vec![1.0],
            cumulative_variance: vec![1.0],
            center: vec![2.0],
            scale: None,
            rotation: vec![vec![1.0]],
            scores: vec![vec![0.5], vec![-0.5]],
        };
        let json = serde_json::to_string(&pca_response).unwrap();
        assert_eq!(
            serde_json::from_str::<PcaResponse>(&json).unwrap(),
            pca_response
        );

        let lda_request = LdaRequest {
            path: "groups/Baca.parquet".into(),
            columns: vec!["as".into(), "fe".into()],
            group_column: "Site".into(),
            transformation: None,
        };
        let json = serde_json::to_string(&lda_request).unwrap();
        assert_eq!(
            serde_json::from_str::<LdaRequest>(&json).unwrap(),
            lda_request
        );

        let lda_response = LdaResponse {
            path: "groups/Baca.parquet".into(),
            revision_id: "rev-1".into(),
            column_names: vec!["as".into()],
            levels: vec!["Baca".into(), "Hooper".into()],
            prior: vec![0.5, 0.5],
            counts: vec![1, 2],
            means: vec![vec![1.5], vec![4.0]],
            scaling: vec![vec![-0.707]],
            svd: vec![2.5],
            score_names: vec!["LD1".into()],
            scores: vec![vec![-0.6], vec![0.3], vec![0.3]],
            warnings: vec![],
        };
        let json = serde_json::to_string(&lda_response).unwrap();
        assert_eq!(
            serde_json::from_str::<LdaResponse>(&json).unwrap(),
            lda_response
        );

        let umap_request = UmapRequest {
            path: "groups/Baca.parquet".into(),
            columns: vec!["as".into(), "fe".into()],
            transformation: None,
            seed: None,
        };
        let json = serde_json::to_string(&umap_request).unwrap();
        assert_eq!(
            serde_json::from_str::<UmapRequest>(&json).unwrap(),
            umap_request
        );
        // `seed` defaults on deserialization when omitted.
        let omitted = serde_json::json!({
            "path": "groups/Baca.parquet",
            "columns": ["as", "fe"],
            "transformation": null,
        });
        let request: UmapRequest = serde_json::from_value(omitted).unwrap();
        assert_eq!(request.seed, None);

        let umap_response = UmapResponse {
            path: "groups/Baca.parquet".into(),
            revision_id: "rev-1".into(),
            column_names: vec!["as".into(), "fe".into()],
            score_names: vec!["V1".into(), "V2".into()],
            embedding: vec![vec![-4.46, 2.06], vec![1.61, -6.25]],
            seed: 20260914,
            n_neighbors: 15,
            n_epochs: 200,
            a: 1.5769436126945664,
            b: 0.8950607181519281,
            warnings: vec![],
        };
        let json = serde_json::to_string(&umap_response).unwrap();
        assert_eq!(
            serde_json::from_str::<UmapResponse>(&json).unwrap(),
            umap_response
        );
    }
}

#[cfg(test)]
mod explore_dto_tests {
    use super::*;

    #[test]
    fn explore_dtos_round_trip() {
        let missing = ExploreMissingProfileRequest {
            path: "groups/Baca.parquet".into(),
            columns: vec!["as".into(), "fe".into()],
            transformation: None,
        };
        let json = serde_json::to_string(&missing).unwrap();
        assert_eq!(
            serde_json::from_str::<ExploreMissingProfileRequest>(&json).unwrap(),
            missing
        );

        // Histogram bins default to the legacy 30.
        let hist: ExploreHistogramRequest = serde_json::from_str(
            r#"{"path":"groups/Baca.parquet","column":"as","transformation":null}"#,
        )
        .unwrap();
        assert_eq!(hist.bins, 30);

        let count = ExploreCrosstabResponse {
            path: "groups/Baca.parquet".into(),
            revision_id: "rev".into(),
            summary_method: "count".into(),
            rows: CrosstabRows::Count {
                rows: vec![CrosstabCountRow {
                    group: Some("A".into()),
                    value: Some("1.5".into()),
                    count: 2,
                }],
            },
        };
        let json = serde_json::to_string(&count).unwrap();
        assert!(json.contains(r#""kind":"count""#), "tagged rows: {json}");
        assert_eq!(
            serde_json::from_str::<ExploreCrosstabResponse>(&json).unwrap(),
            count
        );

        let summary = ExploreCrosstabResponse {
            path: "groups/Baca.parquet".into(),
            revision_id: "rev".into(),
            summary_method: "sd".into(),
            rows: CrosstabRows::Summary {
                result_column: "result-as".into(),
                rows: vec![CrosstabSummaryRow {
                    group: None,
                    result: None,
                }],
            },
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains(r#""kind":"summary""#), "tagged rows: {json}");
        assert!(json.contains("null"), "NA serializes as null");
        assert_eq!(
            serde_json::from_str::<ExploreCrosstabResponse>(&json).unwrap(),
            summary
        );

        let row = CompositionalProfileRow {
            rowid: 1,
            element: "as".into(),
            value: None,
            group_label: Some("A".into()),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("null"), "NA value serializes as null: {json}");
        assert_eq!(
            serde_json::from_str::<CompositionalProfileRow>(&json).unwrap(),
            row
        );
    }
}

#[cfg(test)]
mod export_dto_tests {
    use super::*;

    #[test]
    fn export_dtos_round_trip() {
        let measured = ExportMeasuredDataRequest {
            path: "groups/Baca.parquet".into(),
            raw_text: false,
        };
        let json = serde_json::to_string(&measured).unwrap();
        assert_eq!(
            serde_json::from_str::<ExportMeasuredDataRequest>(&json).unwrap(),
            measured
        );
        // `raw_text` defaults to false on the wire.
        assert_eq!(
            serde_json::from_str::<ExportMeasuredDataRequest>("{\"path\":\"groups/Baca.parquet\"}")
                .unwrap()
                .raw_text,
            false
        );

        let definition = TransformationDefinition {
            name: "log_ratio_set".into(),
            transform_method: TransformMethod::Log10,
            imputation_method: ImputationMethod::None,
            imputation_seed: None,
            elemental_columns: vec!["as".into(), "fe".into()],
            descriptive_columns: vec![],
            group_column: None,
            ratios: vec![RatioSpecDto {
                output_name: None,
                numerator: "as".into(),
                denominator: "fe".into(),
            }],
            ratio_mode: RatioMode::Append,
        };
        let transformed = ExportTransformedRequest {
            path: "groups/Baca.parquet".into(),
            definition: definition.clone(),
            raw_text: true,
        };
        let json = serde_json::to_string(&transformed).unwrap();
        assert!(json.contains("\"raw_text\":true"), "raw_text: {json}");
        assert_eq!(
            serde_json::from_str::<ExportTransformedRequest>(&json).unwrap(),
            transformed
        );

        let pca = ExportPcaScoresRequest {
            path: "groups/Baca.parquet".into(),
            columns: vec!["as".into(), "fe".into()],
            scale: false,
            transformation: Some(definition),
        };
        let json = serde_json::to_string(&pca).unwrap();
        assert_eq!(
            serde_json::from_str::<ExportPcaScoresRequest>(&json).unwrap(),
            pca
        );
        // `scale` defaults to false on the wire, mirroring `PcaRequest`.
        let omitted: ExportPcaScoresRequest = serde_json::from_str(
            r#"{"path":"groups/Baca.parquet","columns":["as"],"transformation":null}"#,
        )
        .unwrap();
        assert_eq!(omitted.scale, false);

        let result = ExportResult {
            file_name: "Baca.csv".into(),
            media_type: "text/csv".into(),
            content: "anid,Site,as\nA1,Baca,'=x\n".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(serde_json::from_str::<ExportResult>(&json).unwrap(), result);
    }
}

#[cfg(test)]
mod preference_dto_tests {
    use super::*;

    #[test]
    fn preference_dtos_round_trip() {
        // Keys serialize under their stable camelCase wire names.
        assert_eq!(
            serde_json::to_string(&PreferenceKey::LastOpenedDataset).unwrap(),
            "\"lastOpenedDataset\""
        );
        assert_eq!(
            serde_json::to_string(&PreferenceKey::ColumnVisibility).unwrap(),
            "\"columnVisibility\""
        );

        let entry = PreferenceEntry {
            key: PreferenceKey::Theme,
            value: serde_json::json!("dark"),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(
            serde_json::from_str::<PreferenceEntry>(&json).unwrap(),
            entry
        );

        let put = PutPreferenceRequest {
            key: PreferenceKey::CompactMode,
            value: serde_json::json!(true),
        };
        let json = serde_json::to_string(&put).unwrap();
        assert_eq!(
            serde_json::from_str::<PutPreferenceRequest>(&json).unwrap(),
            put
        );

        let response = GetPreferencesResponse {
            preferences: vec![entry],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(
            serde_json::from_str::<GetPreferencesResponse>(&json).unwrap(),
            response
        );
        assert_eq!(
            serde_json::from_str::<GetPreferencesResponse>("{\"preferences\":[]}")
                .unwrap()
                .preferences,
            Vec::<PreferenceEntry>::new()
        );
    }

    #[test]
    fn preference_key_str_matches_wire_names() {
        assert_eq!(PreferenceKey::Theme.as_str(), "theme");
        assert_eq!(
            PreferenceKey::LastOpenedDataset.as_str(),
            "lastOpenedDataset"
        );
        assert_eq!(PreferenceKey::ColumnVisibility.as_str(), "columnVisibility");
        assert_eq!(PreferenceKey::CompactMode.as_str(), "compactMode");
    }
}
