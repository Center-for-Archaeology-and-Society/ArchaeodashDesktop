/**
 * Tauri IPC adapter for the desktop client (Section 9.2). The webview `invoke`
 * function is injected so this package does not depend on `@tauri-apps/api`;
 * `apps/desktop` passes `invoke` from `@tauri-apps/api/core`.
 *
 * Tauri 2 maps JS argument keys (camelCase) onto the Rust command parameters
 * (snake_case). Request DTO fields inside a `request` object keep the Rust
 * serde names (snake_case) because the contract structs carry no rename.
 * Desktop commands fail as `Result<T, String>`, so rejections are plain
 * strings; `toTransportError` normalizes them into TransportError envelopes.
 */
import type {
  AppInfo,
  AppliedTransformation,
  ApplyTransformationRequest,
  BatchRatioRequest,
  DeleteGroupRequest,
  DuplicateGroupRequest,
  ExploreCrosstabRequest,
  ExploreCrosstabResponse,
  ExploreCompositionalProfileRequest,
  ExploreCompositionalProfileResponse,
  ExploreHistogramRequest,
  ExploreHistogramResponse,
  ExploreMissingProfileRequest,
  ExploreMissingProfileResponse,
  ExportMeasuredDataRequest,
  ExportPcaScoresRequest,
  ExportResult,
  ExportTransformedRequest,
  FileDownload,
  FileUploadRequest,
  GetPreferencesResponse,
  GroupCandidate,
  GroupSummary,
  ImportCommitRequest,
  ImportCommitResponse,
  ImportPreviewRequest,
  ImportPreviewResponse,
  LdaRequest,
  LdaResponse,
  MergeGroupsRequest,
  PcaRequest,
  PcaResponse,
  PatchDescriptiveValuesRequest,
  PreferenceKey,
  RatioSpecDto,
  SaveTransformationRequest,
  SaveTransformationResponse,
  StagedFile,
  TransformationDefinition,
  TransformationListResponse,
  TransactionResponse,
  TransferUnitsRequest,
  UmapRequest,
  UmapResponse,
} from '@archaeodash/contracts';
import {
  toTransportError,
  type ExportsService,
  type ExploreService,
  type FilesService,
  type GroupsService,
  type ImportsService,
  type OrdinationService,
  type PreferencesService,
  type TransformationsService,
  type Transport,
} from './transport.ts';

export type InvokeLike = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

export class TauriTransport implements Transport {
  readonly kind = 'tauri' as const;

  private readonly invoke: InvokeLike;

  readonly imports: ImportsService;
  readonly files: FilesService;
  readonly groups: GroupsService;
  readonly transformations: TransformationsService;
  readonly ordination: OrdinationService;
  readonly explore: ExploreService;
  readonly exports: ExportsService;
  readonly preferences: PreferencesService;

  constructor(invoke: InvokeLike) {
    this.invoke = invoke;
    this.imports = this.makeImports();
    this.files = this.makeFiles();
    this.groups = this.makeGroups();
    this.transformations = this.makeTransformations();
    this.ordination = this.makeOrdination();
    this.explore = this.makeExplore();
    this.exports = this.makeExports();
    this.preferences = this.makePreferences();
  }

  private call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    return this.invoke<T>(cmd, args).catch((err: unknown) => {
      throw toTransportError(err, 'tauri_error');
    });
  }

  appInfo(): Promise<AppInfo> {
    return this.call<AppInfo>('app_info');
  }

  private makeImports(): ImportsService {
    return {
      preview: (request: ImportPreviewRequest) => this.call('open_import_preview', { request }),
      commit: (request: ImportCommitRequest) => this.call('commit_group_import', { request }),
    };
  }

  private makeFiles(): FilesService {
    return {
      upload: (path: string, content: Uint8Array) => {
        const request: FileUploadRequest = { path, content: Array.from(content) };
        return this.call<StagedFile>('upload_source_file', { request });
      },
      metadata: (fileId: string) => this.call<StagedFile>('source_file_metadata', { fileId }),
      download: (fileId: string) => this.call<FileDownload>('download_source_file', { fileId }),
      remove: (fileId: string) => this.call<StagedFile>('delete_source_file', { fileId }),
    };
  }

  private makeGroups(): GroupsService {
    return {
      scan: () => this.call<GroupCandidate[]>('scan_group_candidates'),
      validate: (path: string) => this.call<GroupSummary>('validate_group_file', { path }),
      rows: (path: string) => this.call('group_rows', { path }),
      transferUnits: (request: TransferUnitsRequest) => this.call('transfer_units', { request }),
      mergeGroups: (request: MergeGroupsRequest) => this.call('merge_groups', { request }),
      patchDescriptiveValues: (request: PatchDescriptiveValuesRequest) =>
        this.call('patch_descriptive_values', { request }),
      duplicateGroup: (request: DuplicateGroupRequest) => this.call('duplicate_group', { request }),
      deleteGroup: (request: DeleteGroupRequest) => this.call('delete_group', { request }),
    };
  }

  private makeTransformations(): TransformationsService {
    return {
      // The desktop command takes the bare definition, not the save wrapper.
      save: (request: SaveTransformationRequest) =>
        this.call<SaveTransformationResponse>('save_transformation', {
          definition: request.definition,
        }),
      list: () => this.call<TransformationListResponse>('list_transformations'),
      load: (name: string) => this.call<TransformationDefinition>('load_transformation', { name }),
      remove: (name: string) =>
        this.call<TransformationDefinition>('delete_transformation', { name }),
      batchRatios: (request: BatchRatioRequest) => this.call('batch_ratio_specs', { request }),
      apply: (request: ApplyTransformationRequest) => this.call('apply_transformation', { request }),
    };
  }

  private makeOrdination(): OrdinationService {
    return {
      pca: (request: PcaRequest) => this.call<PcaResponse>('ordination_pca', { request }),
      lda: (request: LdaRequest) => this.call<LdaResponse>('ordination_lda', { request }),
      umap: (request: UmapRequest) => this.call<UmapResponse>('ordination_umap', { request }),
    };
  }

  private makeExplore(): ExploreService {
    return {
      missingProfile: (request: ExploreMissingProfileRequest) =>
        this.call('explore_missing_profile', { request }),
      histogram: (request: ExploreHistogramRequest) =>
        this.call('explore_histogram', { request }),
      crosstab: (request: ExploreCrosstabRequest) => this.call('explore_crosstab', { request }),
      compositionalProfile: (request: ExploreCompositionalProfileRequest) =>
        this.call('explore_compositional_profile', { request }),
    };
  }

  private makeExports(): ExportsService {
    return {
      measuredData: (request: ExportMeasuredDataRequest) =>
        this.call('export_measured_data', { request }),
      transformed: (request: ExportTransformedRequest) =>
        this.call('export_transformed', { request }),
      pcaScores: (request: ExportPcaScoresRequest) =>
        this.call('export_pca_scores', { request }),
    };
  }

  private makePreferences(): PreferencesService {
    return {
      get: () => this.call<GetPreferencesResponse>('preferences_get'),
      put: (key: PreferenceKey, value: unknown) =>
        this.call<void>('preferences_set', { request: { key, value } }),
    };
  }
}
