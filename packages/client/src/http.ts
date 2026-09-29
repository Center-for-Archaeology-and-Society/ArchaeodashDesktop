/**
 * HTTP adapter for the hosted web client (Section 9.2): relative `/api/v1`
 * routes, JSON envelopes, raw-byte uploads/downloads, and ErrorEnvelope
 * error normalization.
 */
import type {
  AppInfo,
  AppliedTransformation,
  ApplyTransformationRequest,
  BatchRatioRequest,
  BatchTransferUnitsRequest,
  ClusterDiagnosticsRequest,
  ClusterDiagnosticsResponse,
  ClusterFitRequest,
  ClusterFitResponse,
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
  EuclideanMatchesRequest,
  EuclideanMatchesResponse,
  FileDownload,
  GetPreferencesResponse,
  ErrorEnvelope,
  GroupCandidate,
  GroupSummary,
  ImportCommitRequest,
  ImportCommitResponse,
  ImportPreviewRequest,
  ImportPreviewResponse,
  LdaRequest,
  LdaResponse,
  MembershipProbabilitiesRequest,
  MembershipProbabilitiesResponse,
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
  TransportError,
  type ExportsService,
  type ExploreService,
  type FilesService,
  type GroupsService,
  type ImportsService,
  type OrdinationService,
  type ClusteringService,
  type PreferencesService,
  type TransformationsService,
  type Transport,
} from './transport.ts';

export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

export class HttpTransport implements Transport {
  readonly kind = 'http' as const;

  private readonly baseUrl: string;
  private readonly fetchImpl: FetchLike;

  readonly imports: ImportsService;
  readonly files: FilesService;
  readonly groups: GroupsService;
  readonly transformations: TransformationsService;
  readonly ordination: OrdinationService;
  readonly clustering: ClusteringService;
  readonly explore: ExploreService;
  readonly exports: ExportsService;
  readonly preferences: PreferencesService;

  constructor(baseUrl: string = '', fetchImpl: FetchLike = (...args) => fetch(...args)) {
    this.baseUrl = baseUrl.replace(/\/$/, '');
    this.fetchImpl = fetchImpl;
    this.imports = this.makeImports();
    this.files = this.makeFiles();
    this.groups = this.makeGroups();
    this.transformations = this.makeTransformations();
    this.ordination = this.makeOrdination();
    this.clustering = this.makeClustering();
    this.explore = this.makeExplore();
    this.exports = this.makeExports();
    this.preferences = this.makePreferences();
  }

  private url(path: string, query?: Record<string, string>): string {
    const base = `${this.baseUrl}${path}`;
    if (!query) return base;
    const params = new URLSearchParams(query);
    const qs = params.toString();
    return qs ? `${base}?${qs}` : base;
  }

  /** Encodes a project-relative path for wildcard route segments, keeping `/`. */
  private static pathSegment(path: string): string {
    return path.split('/').map(encodeURIComponent).join('/');
  }

  private async request<T>(
    method: string,
    path: string,
    options?: { query?: Record<string, string>; body?: unknown; raw?: Uint8Array },
  ): Promise<T> {
    const init: RequestInit = { method };
    if (options?.body !== undefined) {
      init.headers = { 'content-type': 'application/json' };
      init.body = JSON.stringify(options.body);
    } else if (options?.raw !== undefined) {
      init.headers = { 'content-type': 'application/octet-stream' };
      init.body = options.raw as unknown as BodyInit;
    }
    const res = await this.fetchImpl(this.url(path, options?.query), init);
    return this.unwrap<T>(res);
  }

  private async unwrap<T>(res: Response): Promise<T> {
    if (res.status === 204) return undefined as T;
    if (!res.ok) {
      let envelope: ErrorEnvelope | null = null;
      try {
        envelope = (await res.json()) as ErrorEnvelope;
      } catch {
        envelope = null;
      }
      if (!envelope || typeof envelope.code !== 'string') {
        envelope = { code: `http_${res.status}`, message: res.statusText };
      }
      throw new TransportError(envelope);
    }
    return (await res.json()) as T;
  }

  appInfo(): Promise<AppInfo> {
    return this.request<AppInfo>('GET', '/healthz');
  }
  private makeImports(): ImportsService {
    return {
      preview: (request) => this.request('POST', '/api/v1/imports/preview', { body: request }),
      commit: (request) => this.request('POST', '/api/v1/imports/commit', { body: request }),
    };
  }

  private makeFiles(): FilesService {
    const metadata = (fileId: string) =>
      this.request<StagedFile>('GET', `/api/v1/files/${encodeURIComponent(fileId)}`);
    return {
      upload: (path, content) =>
        this.request<StagedFile>('POST', '/api/v1/files', { query: { path }, raw: content }),
      metadata,
      download: async (fileId) => {
        const meta = await metadata(fileId);
        const res = await this.fetchImpl(this.url(`/api/v1/files/${encodeURIComponent(fileId)}/download`));
        if (!res.ok) return this.unwrap<FileDownload>(res);
        const bytes = new Uint8Array(await res.arrayBuffer());
        return { metadata: meta, content: Array.from(bytes) };
      },
      remove: (fileId) =>
        this.request<StagedFile>('DELETE', `/api/v1/files/${encodeURIComponent(fileId)}`),
    };
  }

  private makeGroups(): GroupsService {
    return {
      scan: () => this.request<GroupCandidate[]>('GET', '/api/v1/groups'),
      // The validate route takes a bare JSON string body, not an object.
      validate: (path) => this.request('POST', '/api/v1/groups/validate', { body: path }),
      rows: (path) => this.request('GET', '/api/v1/groups/rows', { query: { path } }),
      transferUnits: (request) =>
        this.request('POST', '/api/v1/groups/transfer-units', { body: request }),
      batchTransferUnits: (request: BatchTransferUnitsRequest) =>
        this.request('POST', '/api/v1/groups/batch-transfer-units', { body: request }),
      mergeGroups: (request) => this.request('POST', '/api/v1/groups/merge', { body: request }),
      patchDescriptiveValues: (request: PatchDescriptiveValuesRequest) =>
        this.request('PATCH', '/api/v1/groups/descriptive-values', { body: request }),
      duplicateGroup: (request) => this.request('POST', '/api/v1/groups/duplicate', { body: request }),
      deleteGroup: (request: DeleteGroupRequest) =>
        this.request('DELETE', `/api/v1/groups/${HttpTransport.pathSegment(request.path)}`, {
          query: { expected_revision: request.expected_revision, confirm_path: request.confirm_path },
        }),
    };
  }

  private makeTransformations(): TransformationsService {
    return {
      save: (request) => this.request('POST', '/api/v1/transformations', { body: request }),
      list: () => this.request<TransformationListResponse>('GET', '/api/v1/transformations'),
      load: (name) =>
        this.request<TransformationDefinition>(
          'GET',
          `/api/v1/transformations/${encodeURIComponent(name)}`,
        ),
      remove: (name) =>
        this.request<TransformationDefinition>(
          'DELETE',
          `/api/v1/transformations/${encodeURIComponent(name)}`,
        ),
      batchRatios: (request) =>
        this.request('POST', '/api/v1/transformations/ratios/batch', { body: request }),
      apply: (request: ApplyTransformationRequest) =>
        this.request('POST', '/api/v1/transformations/apply', { body: request }),
    };
  }

  private makeOrdination(): OrdinationService {
    return {
      pca: (request: PcaRequest) => this.request('POST', '/api/v1/ordination/pca', { body: request }),
      lda: (request: LdaRequest) => this.request('POST', '/api/v1/ordination/lda', { body: request }),
      umap: (request: UmapRequest) => this.request('POST', '/api/v1/ordination/umap', { body: request }),
    };
  }

  private makeClustering(): ClusteringService {
    return {
      diagnostics: (request: ClusterDiagnosticsRequest) =>
        this.request<ClusterDiagnosticsResponse>('POST', '/api/v1/cluster/diagnostics', { body: request }),
      fit: (request: ClusterFitRequest) =>
        this.request<ClusterFitResponse>('POST', '/api/v1/cluster/fit', { body: request }),
      membershipProbabilities: (request: MembershipProbabilitiesRequest) =>
        this.request<MembershipProbabilitiesResponse>('POST', '/api/v1/membership/probabilities', { body: request }),
      euclideanMatches: (request: EuclideanMatchesRequest) =>
        this.request<EuclideanMatchesResponse>('POST', '/api/v1/euclidean/matches', { body: request }),
    };
  }

  private makeExplore(): ExploreService {
    return {
      missingProfile: (request: ExploreMissingProfileRequest) =>
        this.request('POST', '/api/v1/explore/missing-profile', { body: request }),
      histogram: (request: ExploreHistogramRequest) =>
        this.request('POST', '/api/v1/explore/histogram', { body: request }),
      crosstab: (request: ExploreCrosstabRequest) =>
        this.request('POST', '/api/v1/explore/crosstab', { body: request }),
      compositionalProfile: (request: ExploreCompositionalProfileRequest) =>
        this.request('POST', '/api/v1/explore/compositional-profile', { body: request }),
    };
  }

  private makeExports(): ExportsService {
    return {
      measuredData: (request: ExportMeasuredDataRequest) =>
        this.request('POST', '/api/v1/exports/measured-data', { body: request }),
      transformed: (request: ExportTransformedRequest) =>
        this.request('POST', '/api/v1/exports/transformed', { body: request }),
      pcaScores: (request: ExportPcaScoresRequest) =>
        this.request('POST', '/api/v1/exports/pca-scores', { body: request }),
    };
  }

  private makePreferences(): PreferencesService {
    return {
      get: () => this.request<GetPreferencesResponse>('GET', '/api/v1/preferences'),
      put: async (key: PreferenceKey, value: unknown) => {
        await this.request('PUT', '/api/v1/preferences', { body: { key, value } });
      },
    };
  }
}
