/**
 * Generated-from-Rust API/IPC contract types (Section 4).
 *
 * These mirror crates/contracts DTOs. Keep both sides in sync until codegen
 * (schemars -> TS) is introduced; this file is the single client source of truth.
 *
 * Wire naming: the Rust structs use serde's default field naming (snake_case).
 * The only camelCase wire names are the PreferenceKey values and the
 * TransformMethod `zScore` variant, both explicitly renamed on the Rust side.
 */

export interface AppInfo {
  app: string;
  version: string;
  transport: 'http' | 'tauri';
  ready: boolean;
}

export interface ErrorEnvelope {
  code: string;
  message: string;
}

export type JobState =
  | 'idle'
  | 'validating'
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'timed_out';

// --- Imports (Sections 7 / 10.4) ---

export interface ImportPreviewRequest {
  source: string;
  group_column?: string | null;
}

export interface PartitionPreview {
  group_name: string;
  row_count: number;
  suggested_path: string;
}

export interface ImportPreviewResponse {
  source: string;
  row_count: number;
  columns: string[];
  id_column?: string | null;
  elemental_columns: string[];
  partitions: PartitionPreview[];
}

export interface ImportRecipeDto {
  zero_as_na: boolean;
  negative_as_na: boolean;
  na_as_zero: boolean;
  blank_non_element_label?: string | null;
}

export interface ImportCommitRequest {
  source: string;
  group_column: string;
  visible_id_column?: string | null;
  elemental_columns?: string[] | null;
  recipe?: ImportRecipeDto | null;
  destination_dir?: string | null;
}

export interface CommittedGroup {
  group_id: string;
  group_name: string;
  path: string;
  row_count: number;
  revision_id: string;
  measured_elemental_checksum: string;
}

export interface ImportCommitResponse {
  source: string;
  groups: CommittedGroup[];
}

// --- Source files (Section 10.4) ---

export type SourceFileFormat = 'csv' | 'tsv' | 'xlsx';
export type SourceParseState = 'parsed' | 'parse_failed' | 'deferred';

export interface StagedFile {
  file_id: string;
  path: string;
  size_bytes: number;
  sha256: string;
  format: SourceFileFormat;
  parse_state: SourceParseState;
  parse_error?: string | null;
  deleted: boolean;
}

/** Desktop (Tauri) only: the HTTP upload route takes raw bytes instead. */
export interface FileUploadRequest {
  path: string;
  content: number[];
}

/** Desktop (Tauri) only: the HTTP download route streams raw bytes. */
export interface FileDownload {
  metadata: StagedFile;
  content: number[];
}

// --- Groups (Sections 6 / 10.4) ---

export interface GroupSummary {
  path: string;
  group_id: string;
  group_name: string;
  revision_id: string;
  row_count: number;
  source_path?: string | null;
  source_sha256?: string | null;
  elemental_columns: string[];
  descriptive_columns: string[];
}

export interface GroupCandidate {
  path: string;
  ready: boolean;
  group?: GroupSummary | null;
}

export type TransferAction = 'move' | 'copy';

export interface TransferUnitsRequest {
  action: TransferAction;
  source_path: string;
  destination_path: string;
  destination_group_name?: string | null;
  selected_uuids: string[];
  expected_source_revision: string;
}

export interface BatchTransferTarget {
  destination_path: string;
  destination_group_name?: string | null;
  expected_destination_revision?: string | null;
  selected_uuids: string[];
}

export interface BatchTransferUnitsRequest {
  source_path: string;
  expected_source_revision: string;
  targets: BatchTransferTarget[];
}

export interface MergeGroupsRequest {
  sources: string[];
  new_group_name: string;
}

export interface DeleteGroupRequest {
  path: string;
  expected_revision: string;
  confirm_path: string;
}

export interface DescriptiveEdit {
  analytical_uuid: string;
  column: string;
  value?: string | null;
}

export interface PatchDescriptiveValuesRequest {
  path: string;
  expected_revision: string;
  edits: DescriptiveEdit[];
}

export interface DuplicateGroupRequest {
  source_path: string;
  expected_revision: string;
  new_group_name: string;
  destination_path?: string | null;
  preserve_uuids?: boolean;
}

/** Full row data of one group file for the client dataset table (Section 9.4).
 *  The hidden `analytical_uuid` travels for edit addressing; the UI never
 *  displays it and ordinary exports never contain it (Section 3.2). */
export interface GroupRowsRequest {
  path: string;
}

export interface GroupRowDto {
  analytical_uuid: string;
  legacy_rowid?: string | null;
  visible_id?: string | null;
  descriptive: (string | null)[];
  elemental: (number | null)[];
}

export interface GroupRowsResponse {
  path: string;
  revision_id: string;
  visible_id_column: string;
  legacy_rowid_column: string;
  descriptive_columns: string[];
  elemental_columns: string[];
  rows: GroupRowDto[];
}

export interface TransactionResponse {
  transaction_id: string;
  action: string;
  outputs: GroupSummary[];
  deleted_paths: string[];
}

// --- Transformations (Section 8) ---

export type TransformMethod = 'none' | 'log' | 'log10' | 'zScore';
export type ImputationMethod = 'none' | 'pmm' | 'midastouch' | 'rf';
export type RatioMode = 'append' | 'only';
export type BatchRatioMode = 'one_to_one' | 'cartesian';

export interface RatioSpecDto {
  output_name?: string | null;
  numerator: string;
  denominator: string;
}

export interface TransformationDefinition {
  name: string;
  transform_method: TransformMethod;
  imputation_method: ImputationMethod;
  imputation_seed?: number | null;
  elemental_columns: string[];
  descriptive_columns: string[];
  group_column?: string | null;
  ratios: RatioSpecDto[];
  ratio_mode: RatioMode;
}

export interface TransformationSummary {
  name: string;
  created_at_unix_secs: number;
  transform_method: TransformMethod;
  imputation_method: ImputationMethod;
  ratio_count: number;
}

export interface SaveTransformationRequest {
  definition: TransformationDefinition;
}

export interface SaveTransformationResponse {
  definition: TransformationDefinition;
  replaced: boolean;
}

export interface TransformationListResponse {
  transformations: TransformationSummary[];
}

export interface BatchRatioRequest {
  numerators: string[];
  denominators: string[];
  mode: BatchRatioMode;
}

export interface ApplyTransformationRequest {
  path: string;
  definition: TransformationDefinition;
}

export interface AppliedTransformation {
  path: string;
  revision_id: string;
  columns: string[];
  rows: (number | null)[][];
  non_finite_to_zero: number;
}

// --- Ordination (Sections 8.5-8.7) ---

export interface OrdinationRequestBase {
  path: string;
  columns: string[];
  transformation?: TransformationDefinition | null;
}

export type OrdinationResponseBase = {
  path: string;
  revision_id: string;
  column_names: string[];
};

export interface PcaRequest extends OrdinationRequestBase {
  scale?: boolean;
}

export interface PcaResponse extends OrdinationResponseBase {
  score_names: string[];
  sdev: number[];
  explained_variance: number[];
  cumulative_variance: number[];
  center: number[];
  scale?: number[] | null;
  rotation: number[][];
  scores: number[][];
}

export interface LdaRequest extends OrdinationRequestBase {
  group_column: string;
}

export interface LdaResponse extends OrdinationResponseBase {
  levels: string[];
  prior: number[];
  counts: number[];
  means: number[][];
  scaling: number[][];
  svd: number[];
  score_names: string[];
  scores: number[][];
  warnings: string[];
}

export interface UmapRequest extends OrdinationRequestBase {
  seed?: number | null;
}

export interface UmapResponse extends OrdinationResponseBase {
  score_names: string[];
  embedding: number[][];
  seed: number;
  n_neighbors: number;
  n_epochs: number;
  a: number;
  b: number;
  warnings: string[];
}

// --- Clustering, membership and nearest matches (Phase 6) ---

export type ClusterMethod = 'kmeans' | 'pam' | 'hclust_ward_d2' | 'diana';
export interface ClusterDiagnosticsRequest extends OrdinationRequestBase {
  max_k: number;
  seed: number;
}
export interface ClusterDiagnosticsResponse {
  path: string;
  revision_id: string;
  column_names: string[];
  n_rows: number;
  wss: number[];
  /** Silhouette values correspond to k=2..=max_k; null represents NaN. */
  silhouette: (number | null)[];
}
export interface ClusterFitRequest extends OrdinationRequestBase {
  method: ClusterMethod;
  k?: number | null;
  iter_max?: number;
  nstart?: number;
  seed?: number | null;
}
export interface ClusterFitResponse {
  /** Hidden immutable identities in input row order. */
  analytical_uuids: string[];
  path: string;
  revision_id: string;
  method: ClusterMethod;
  n_rows: number;
  cluster?: number[] | null;
  size?: number[] | null;
  tot_withinss?: number | null;
  centers?: number[][] | null;
  medoids?: number[] | null;
  merge?: [number, number][] | null;
  height?: number[] | null;
  order?: number[] | null;
  silhouette?: (number | null)[] | null;
}
export type MembershipMethod = 'hotellings' | 'mahalanobis';
export interface MembershipProbabilitiesRequest {
  path: string;
  columns: string[];
  group_column: string;
  id_column: string;
  method: MembershipMethod;
}
export interface MembershipProbabilitiesResponse {
  /** Hidden immutable identities aligned with ids and probability rows. */
  analytical_uuids: string[];
  path: string;
  revision_id: string;
  effective_method: MembershipMethod;
  eligible_groups: string[];
  ids: string[];
  groups: string[];
  probabilities: (number | null)[][];
  best_group: (string | null)[];
  best_value: (number | null)[];
  in_group: boolean[];
}
export interface EuclideanMatchesRequest {
  path: string;
  columns: string[];
  group_column: string;
  id_column: string;
  limit: number;
  within_group: boolean;
}
export interface EuclideanMatchDto {
  /** Hidden identities; display IDs are not unique selection keys. */
  analytical_uuid: string;
  match_analytical_uuid: string;
  rowid: string;
  id: string;
  match_id: string;
  distance: number | null;
  group: string;
  match_group: string;
}
export interface EuclideanMatchesResponse {
  path: string;
  revision_id: string;
  rows: EuclideanMatchDto[];
}

// --- Explore (Section 8.12) ---

export interface ExploreMissingProfileRequest extends OrdinationRequestBase {}

export interface MissingProfileRow {
  feature: string;
  num_missing: number;
  pct_missing: number;
  band: string;
}

export interface ExploreMissingProfileResponse extends OrdinationResponseBase {
  rows: MissingProfileRow[];
}

export interface ExploreHistogramRequest {
  path: string;
  column: string;
  bins?: number;
  transformation?: TransformationDefinition | null;
}

export interface ExploreHistogramResponse {
  path: string;
  revision_id: string;
  column: string;
  breaks: number[];
  counts: number[];
}

export interface ExploreCrosstabRequest {
  path: string;
  group_column: string;
  value_column: string;
  summary_method: 'count' | 'mean' | 'median' | 'sd' | (string & {});
}

export interface CrosstabCountRow {
  kind: 'count';
  rows: { group: string | null; value: string | null; count: number }[];
}

export interface CrosstabSummaryRow {
  kind: 'summary';
  result_column: string;
  rows: { group: string | null; result: number | null }[];
}

export type CrosstabRows = CrosstabCountRow | CrosstabSummaryRow;

export interface ExploreCrosstabResponse extends OrdinationResponseBase {
  summary_method: string;
  rows: CrosstabRows;
}

export interface ExploreCompositionalProfileRequest extends OrdinationRequestBase {
  group_column?: string | null;
}

export interface CompositionalProfileRow {
  rowid: number;
  element: string;
  value: number | null;
  group_label: string | null;
}

export interface ExploreCompositionalProfileResponse extends OrdinationResponseBase {
  rows: CompositionalProfileRow[];
}

// --- Exports (Section 7.3) ---

export interface ExportMeasuredDataRequest {
  path: string;
  raw_text?: boolean;
}

export interface ExportTransformedRequest {
  path: string;
  definition: TransformationDefinition;
  raw_text?: boolean;
}

export interface ExportPcaScoresRequest extends OrdinationRequestBase {
  scale?: boolean;
}

export interface ExportResult {
  file_name: string;
  media_type: string;
  content: string;
}

// --- Preferences (Section 10.1) ---

export type PreferenceKey = 'theme' | 'lastOpenedDataset' | 'columnVisibility' | 'compactMode';

export interface PreferenceEntry {
  key: PreferenceKey;
  value: unknown;
}

export interface GetPreferencesResponse {
  preferences: PreferenceEntry[];
}

export interface PutPreferenceRequest {
  key: PreferenceKey;
  value: unknown;
}
