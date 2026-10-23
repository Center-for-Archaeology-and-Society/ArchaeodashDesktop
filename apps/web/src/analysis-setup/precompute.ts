/**
 * Background precompute of the Analysis setup's ordinations. The Data
 * Manager sends exactly the requests the Ordination and Visualize pages make
 * for "all groups" (the Dataset default), so the backend's ordination cache
 * answers those pages instantly instead of computing on the fly.
 */
import type {
  GroupCandidate,
  LdaRequest,
  OrdinationKind,
  OrdinationService,
  PcaRequest,
  TransformationDefinition,
  UmapRequest,
} from '@archaeodash/client';
import { syntheticGroupColumn } from '../visualize/combine.ts';
import { UMAP_SEED, analysisInput } from './setup.ts';

export const ORDINATION_LABELS: Readonly<Record<OrdinationKind, string>> = { pca: 'PCA', umap: 'UMAP', lda: 'LDA' };

/** Slowest last, so quick results are ready first. */
const RUN_ORDER: readonly OrdinationKind[] = ['pca', 'lda', 'umap'];

export interface PrecomputeJob {
  readonly kind: OrdinationKind;
  readonly run: (ordination: OrdinationService) => Promise<void>;
}

export interface PrecomputePlan {
  readonly jobs: PrecomputeJob[];
  /** Requested ordinations that cannot run, with the reason. */
  readonly skipped: { kind: OrdinationKind; reason: string }[];
  /** The requests, for tests and diagnostics. */
  readonly requests: { pca?: PcaRequest; umap?: UmapRequest; lda: LdaRequest[] };
}

/** Plans the precompute for every ready group file, in scan order. */
export function planPrecompute(setup: TransformationDefinition | null, candidates: readonly GroupCandidate[]): PrecomputePlan {
  const wanted = RUN_ORDER.filter((k) => setup?.precompute?.includes(k));
  const ready = candidates.filter((c) => c.ready && c.group);
  const skipAll = (reason: string): PrecomputePlan => ({ jobs: [], skipped: wanted.map((kind) => ({ kind, reason })), requests: { lda: [] } });
  if (!setup || wanted.length === 0) return { jobs: [], skipped: [], requests: { lda: [] } };
  if (ready.length === 0) return skipAll('no group files yet');
  // Same column rule as the pages: measured columns every group shares.
  const firstColumns = ready[0]!.group!.elemental_columns;
  const shared = firstColumns.filter((c) => ready.every((r) => r.group!.elemental_columns.includes(c)));
  const input = analysisInput(setup, shared);
  if (input.columns.length < 2) return skipAll('needs two selected elements shared by every group');

  const [path, ...extra] = ready.map((c) => c.path);
  const base = { path: path!, extra_paths: extra, columns: input.columns, transformation: input.transformation ?? null };
  const jobs: PrecomputeJob[] = [];
  const skipped: PrecomputePlan['skipped'] = [];
  const requests: PrecomputePlan['requests'] = { lda: [] };
  for (const kind of wanted) {
    if (kind === 'pca') {
      const request: PcaRequest = { ...base };
      requests.pca = request;
      jobs.push({ kind, run: async (o) => { await o.pca(request); } });
    } else if (kind === 'umap') {
      const request: UmapRequest = { ...base, seed: UMAP_SEED };
      requests.umap = request;
      jobs.push({ kind, run: async (o) => { await o.umap(request); } });
    } else if (ready.length < 3) {
      skipped.push({ kind, reason: 'needs three or more groups' });
    } else {
      const descriptive = ready.flatMap((r) => r.group!.descriptive_columns);
      // Visualize groups by file; Ordination uses the synthetic group column.
      const byFile: LdaRequest = { ...base, group_column: '', group_by_file: true };
      const byColumn: LdaRequest = { ...base, group_column: syntheticGroupColumn(descriptive) };
      requests.lda.push(byFile, byColumn);
      jobs.push({ kind, run: async (o) => { await o.lda(byFile); await o.lda(byColumn); } });
    }
  }
  return { jobs, skipped, requests };
}

export type PrecomputeState = 'waiting' | 'computing' | 'ready' | { failed: string } | { skipped: string };

export function precomputeStatusText(kind: OrdinationKind, state: PrecomputeState): string {
  const label = ORDINATION_LABELS[kind];
  if (state === 'waiting') return `${label} waiting`;
  if (state === 'computing') return `${label} computing…`;
  if (state === 'ready') return `${label} ready`;
  if ('failed' in state) return `${label} failed: ${state.failed}`;
  return `${label} skipped (${state.skipped})`;
}
