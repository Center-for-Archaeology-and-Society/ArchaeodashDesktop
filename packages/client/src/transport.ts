/**
 * Transport abstraction (Section 9.2): the client never knows whether it talks
 * to the Axum HTTP API or Tauri IPC. One adapter per delivery mode, one
 * `Transport` port with the service groups that exist so far (jobs, projects,
 * and workspaces land with their phases).
 */
import type {
  AnalysisJobSnapshot,
  AnalysisJobEvent,
  SubmitAnalysisJobRequest,
  AppliedTransformation,
  AppInfo,
  ApplyTransformationRequest,
  BatchRatioRequest,
  BatchTransferUnitsRequest,
  ClusterDiagnosticsRequest,
  ClusterDiagnosticsResponse,
  ClusterFitRequest,
  ClusterFitResponse,
  DeleteGroupRequest,
  DuplicateGroupRequest,
  ErrorEnvelope,
  EuclideanMatchesRequest,
  EuclideanMatchesResponse,
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
  GetPreferencesResponse,
  GroupCandidate,
  GroupRowsResponse,
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
  PreferenceEntry,
  PreferenceKey,
  ProjectInfo,
  PatchDescriptiveValuesRequest,
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

export type { AppInfo, ErrorEnvelope };

export class TransportError extends Error {
  readonly envelope: ErrorEnvelope;

  constructor(envelope: ErrorEnvelope) {
    super(envelope.message);
    this.name = 'TransportError';
    this.envelope = envelope;
  }
}

export interface ImportsService {
  preview(request: ImportPreviewRequest): Promise<ImportPreviewResponse>;
  commit(request: ImportCommitRequest): Promise<ImportCommitResponse>;
}

export interface FilesService {
  /** Opens the desktop source-file picker and stages the selected file. */
  pickImportSource?: () => Promise<StagedFile | null>;
  /** HTTP sends raw bytes; Tauri wraps them in a FileUploadRequest. */
  upload(path: string, content: Uint8Array): Promise<StagedFile>;
  metadata(fileId: string): Promise<StagedFile>;
  download(fileId: string): Promise<FileDownload>;
  remove(fileId: string): Promise<StagedFile>;
}

export interface GroupsService {
  scan(): Promise<GroupCandidate[]>;
  validate(path: string): Promise<GroupSummary>;
  rows(path: string): Promise<GroupRowsResponse>;
  transferUnits(request: TransferUnitsRequest): Promise<TransactionResponse>;
  batchTransferUnits(request: BatchTransferUnitsRequest): Promise<TransactionResponse>;
  mergeGroups(request: MergeGroupsRequest): Promise<TransactionResponse>;
  patchDescriptiveValues(request: PatchDescriptiveValuesRequest): Promise<TransactionResponse>;
  duplicateGroup(request: DuplicateGroupRequest): Promise<TransactionResponse>;
  deleteGroup(request: DeleteGroupRequest): Promise<TransactionResponse>;
}

export interface TransformationsService {
  save(request: SaveTransformationRequest): Promise<SaveTransformationResponse>;
  list(): Promise<TransformationListResponse>;
  load(name: string): Promise<TransformationDefinition>;
  remove(name: string): Promise<TransformationDefinition>;
  batchRatios(request: BatchRatioRequest): Promise<RatioSpecDto[]>;
  apply(request: ApplyTransformationRequest): Promise<AppliedTransformation>;
}

export interface OrdinationService {
  pca(request: PcaRequest): Promise<PcaResponse>;
  lda(request: LdaRequest): Promise<LdaResponse>;
  umap(request: UmapRequest): Promise<UmapResponse>;
}

export interface AnalysisJobsService {
  submit(request: SubmitAnalysisJobRequest): Promise<AnalysisJobSnapshot>;
  get(id: string): Promise<AnalysisJobSnapshot>;
  cancel(id: string): Promise<AnalysisJobSnapshot>;
  /** Optional progress stream; polling remains the source for full job results. */
  subscribe?(id: string, onEvent: (event: AnalysisJobEvent) => void): () => void;
}

export interface ProjectsService {
  /** Opens the platform-native picker; null means the user cancelled. */
  open(): Promise<ProjectInfo | null>;
  current(): Promise<ProjectInfo | null>;
}

export interface ClusteringService {
  diagnostics(request: ClusterDiagnosticsRequest): Promise<ClusterDiagnosticsResponse>;
  fit(request: ClusterFitRequest): Promise<ClusterFitResponse>;
  membershipProbabilities(request: MembershipProbabilitiesRequest): Promise<MembershipProbabilitiesResponse>;
  euclideanMatches(request: EuclideanMatchesRequest): Promise<EuclideanMatchesResponse>;
}

export interface ExploreService {
  missingProfile(request: ExploreMissingProfileRequest): Promise<ExploreMissingProfileResponse>;
  histogram(request: ExploreHistogramRequest): Promise<ExploreHistogramResponse>;
  crosstab(request: ExploreCrosstabRequest): Promise<ExploreCrosstabResponse>;
  compositionalProfile(
    request: ExploreCompositionalProfileRequest,
  ): Promise<ExploreCompositionalProfileResponse>;
}

export interface ExportsService {
  measuredData(request: ExportMeasuredDataRequest): Promise<ExportResult>;
  transformed(request: ExportTransformedRequest): Promise<ExportResult>;
  pcaScores(request: ExportPcaScoresRequest): Promise<ExportResult>;
}

export interface PreferencesService {
  get(): Promise<GetPreferencesResponse>;
  put(key: PreferenceKey, value: unknown): Promise<void>;
}

/** Current terms/privacy notice version the registration dialog must accept. */
export interface ConsentInfo {
  consentVersion: string;
  termsPath: string;
  privacyPath: string;
}

/** Minimal session principal returned by `GET /auth/session`. */
export interface SessionInfo {
  authenticated: boolean;
  username?: string;
  email?: string;
  emailVerified?: boolean;
  csrfToken?: string;
}

/**
 * Hosted-account service (Section 10.1). Password reset and registration
 * issue emails through the server; the client never handles raw tokens
 * beyond carrying the link's token for the verify/reset calls.
 */
export interface AuthService {
  consent(): Promise<ConsentInfo>;
  register(request: {
    username: string;
    email: string;
    password: string;
    consentVersion: string;
  }): Promise<void>;
  verify(token: string): Promise<void>;
  login(request: {
    identifier: string;
    password: string;
    rememberDays?: number;
  }): Promise<SessionInfo>;
  session(): Promise<SessionInfo>;
  logout(): Promise<void>;
  logoutAll(): Promise<void>;
  requestPasswordReset(email: string): Promise<void>;
  confirmPasswordReset(request: { token: string; newPassword: string }): Promise<void>;
}

export interface Transport {
  readonly kind: 'http' | 'tauri';
  appInfo(): Promise<AppInfo>;
  readonly imports: ImportsService;
  readonly files: FilesService;
  readonly groups: GroupsService;
  readonly transformations: TransformationsService;
  readonly ordination: OrdinationService;
  readonly clustering: ClusteringService;
  readonly jobs: AnalysisJobsService;
  readonly projects?: ProjectsService;
  readonly explore: ExploreService;
  readonly exports: ExportsService;
  readonly preferences: PreferencesService;
  /** Hosted-account operations; absent in desktop (Tauri) mode. */
  readonly auth?: AuthService;
}

/** Normalizes any backend rejection into a TransportError. */
export function toTransportError(err: unknown, fallbackCode: string): TransportError {
  if (err instanceof TransportError) return err;
  if (typeof err === 'object' && err !== null) {
    const candidate = err as Partial<ErrorEnvelope>;
    if (typeof candidate.code === 'string' && typeof candidate.message === 'string') {
      return new TransportError({ code: candidate.code, message: candidate.message });
    }
  }
  const message = typeof err === 'string' ? err : String(err);
  return new TransportError({ code: fallbackCode, message });
}
