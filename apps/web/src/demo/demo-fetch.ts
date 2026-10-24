/**
 * Demo data mode for UI review in a plain browser (`?demo` in the URL): a
 * fake `fetch` for the HTTP transport that answers from a small synthetic
 * project held in memory. Nothing leaves the page. Only loaded when the URL
 * asks for it, never inside the desktop app.
 */
import type { EventSourceLike } from '@archaeodash/client';

type FetchLike = (input: string, init?: RequestInit) => Promise<Response>;

/** Deterministic pseudo-random numbers (mulberry32). */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const ELEMENTS = ['Na', 'K', 'Fe', 'Sc', 'Cr', 'Co', 'La', 'Th'];
const GROUPS = ['Baca', 'Chaco', 'Hooper', 'Pueblo Alto'];
const ROWS_PER_GROUP = 60;
const pathOf = (group: string) => `groups/${group.replace(/\s+/g, '_')}.parquet`;
const uuid = (g: number, i: number) => `0197${String(g).padStart(4, '0')}-0000-7000-8000-${String(i).padStart(12, '0')}`;

interface Row {
  analytical_uuid: string;
  legacy_rowid: string;
  visible_id: string;
  descriptive: (string | null)[];
  elemental: (number | null)[];
}

const random = rng(7);
const rowsByGroup = new Map<string, Row[]>(
  GROUPS.map((group, g) => [
    pathOf(group),
    Array.from({ length: ROWS_PER_GROUP }, (_, i): Row => ({
      analytical_uuid: uuid(g, i),
      legacy_rowid: String(g * ROWS_PER_GROUP + i + 1),
      visible_id: `${group.slice(0, 3).toUpperCase()}${String(i + 1).padStart(3, '0')}`,
      descriptive: [group, ['Bowl', 'Jar', 'Sherd'][i % 3]!],
      elemental: ELEMENTS.map((_, e) =>
        e === 5 && i % 9 === 0 ? null : Math.round((10 + e * 40 + g * (e + 3) * 6 + random() * 25) * 100) / 100,
      ),
    })),
  ]),
);

const summary = (group: string) => ({
  path: pathOf(group),
  group_id: `id-${group}`,
  group_name: group,
  revision_id: 'demo-1',
  row_count: ROWS_PER_GROUP,
  source_path: 'sources/demo.csv',
  source_sha256: null,
  elemental_columns: ELEMENTS,
  descriptive_columns: ['Site', 'Form'],
});

function combinedRows(body: { path?: string; extra_paths?: string[] }): Row[] {
  const paths = [body.path ?? '', ...(body.extra_paths ?? [])];
  return paths.flatMap((p) => rowsByGroup.get(p) ?? []);
}

function groupOfRow(row: Row): string {
  return row.descriptive[0] ?? '';
}

/** Two "score" dimensions that separate the groups, plus noise. */
function scores(rows: Row[], dims: number, spread: number): number[][] {
  const r = rng(rows.length);
  return rows.map((row) => {
    const g = GROUPS.indexOf(groupOfRow(row));
    return Array.from({ length: dims }, (_, d) =>
      Math.round(((d === 0 ? Math.cos(g * 1.7) : Math.sin(g * 1.7)) * spread + (r() - 0.5) * spread * 0.6) * 1e4) / 1e4,
    );
  });
}

function json(value: unknown, ms = 120, status = 200): Promise<Response> {
  return new Promise((resolve) =>
    setTimeout(() => resolve(new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } })), ms),
  );
}

let setup: unknown = null;
let jobCounter = 0;

function clusterResult(kind: string, request: Record<string, unknown>): unknown {
  const rows = combinedRows(request as { path?: string; extra_paths?: string[] });
  const base = { path: request.path, revision_id: 'demo-1', column_names: request.columns, source: 'elements' };
  const k = Number(request.k ?? request.max_k ?? 3) || 3;
  const coords = scores(rows, 2, 3);
  if (kind === 'cluster_diagnostics') {
    const wss = Array.from({ length: k }, (_, i) => Math.round(9000 / (i + 1) ** 1.3));
    return { ...base, n_rows: rows.length, wss, silhouette: wss.slice(1).map((_, i) => 0.62 - i * 0.07), diagnostic_method: 'kmeans', metric: 'euclidean' };
  }
  if (kind === 'cluster_fit') {
    const cluster = rows.map((row) => (GROUPS.indexOf(groupOfRow(row)) % k) + 1);
    return {
      ...base,
      analytical_uuids: rows.map((r) => r.analytical_uuid),
      method: request.method ?? 'kmeans',
      n_rows: rows.length,
      cluster,
      size: Array.from({ length: k }, (_, c) => cluster.filter((x) => x === c + 1).length),
      silhouette: rows.map((_, i) => Math.round((0.35 + ((i * 37) % 50) / 100) * 1000) / 1000),
      plot_coordinates: coords,
      cluster_plot_coordinates: coords,
      plot_column_names: ['PC1', 'PC2'],
      cluster_plot_column_names: ['PC1', 'PC2'],
      plot_groups: rows.map(groupOfRow),
      metric: 'euclidean',
    };
  }
  if (kind === 'membership_probabilities') {
    const r = rng(11);
    const probabilities = rows.map(() => GROUPS.map(() => Math.round(r() * 1000) / 10));
    return {
      ...base,
      analytical_uuids: rows.map((x) => x.analytical_uuid),
      effective_method: 'hotellings',
      eligible_groups: GROUPS,
      ids: rows.map((x) => x.visible_id),
      groups: rows.map(groupOfRow),
      probabilities,
      best_group: probabilities.map((p) => GROUPS[p.indexOf(Math.max(...p))] ?? null),
      best_value: probabilities.map((p) => Math.max(...p)),
      in_group: rows.map((_, i) => i % 4 !== 0),
      projection_included: rows.map(() => true),
    };
  }
  return {
    ...base,
    rows: rows.slice(0, 80).map((row, i) => {
      const match = rows[(i + 7) % rows.length]!;
      return {
        analytical_uuid: row.analytical_uuid,
        match_analytical_uuid: match.analytical_uuid,
        rowid: row.legacy_rowid,
        id: row.visible_id,
        match_id: match.visible_id,
        distance: Math.round((1 + (i % 13) / 3) * 1000) / 1000,
        group: groupOfRow(row),
        match_group: groupOfRow(match),
      };
    }),
  };
}

export const demoFetch: FetchLike = async (url, init) => {
  const parsed = new URL(url, 'http://demo.local');
  const p = parsed.pathname;
  const method = (init?.method ?? 'GET').toUpperCase();
  const body = (() => {
    try {
      return init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : {};
    } catch {
      return {};
    }
  })();

  if (p === '/healthz') return json({ app: 'ArchaeoDash', version: 'demo', ready: true }, 10);
  if (p === '/api/v1/auth/session') return json({ authenticated: false }, 10);
  if (p === '/api/v1/preferences') return json(method === 'GET' ? { preferences: [] } : {}, 10);
  if (p === '/api/v1/groups' && method === 'GET') {
    return json(GROUPS.map((g) => ({ path: pathOf(g), ready: true, group: summary(g) })), 200);
  }
  if (p === '/api/v1/groups/validate') {
    const path = String(JSON.parse(String(init?.body ?? '""')));
    const group = GROUPS.find((g) => pathOf(g) === path) ?? GROUPS[0]!;
    return json(summary(group), 80);
  }
  if (p === '/api/v1/groups/rows') {
    const path = parsed.searchParams.get('path') ?? '';
    return json({
      path,
      revision_id: 'demo-1',
      visible_id_column: 'ANID',
      legacy_rowid_column: 'rowid',
      descriptive_columns: ['Site', 'Form'],
      elemental_columns: ELEMENTS,
      rows: rowsByGroup.get(path) ?? [],
    }, 500);
  }
  if (p === '/api/v1/transformations' && method === 'GET') {
    return json({ transformations: setup ? [{ name: 'analysis_setup' }] : [] }, 30);
  }
  if (p === '/api/v1/transformations' && method === 'POST') {
    setup = (body as { definition?: unknown }).definition ?? null;
    return json({ definition: setup }, 200);
  }
  if (p.startsWith('/api/v1/transformations/')) {
    if (method === 'DELETE') setup = null;
    return json(setup ?? {}, 30);
  }
  if (p === '/api/v1/explore/histogram') {
    const rows = combinedRows(body);
    const idx = ELEMENTS.indexOf(String(body.column));
    const values = rows.map((r) => r.elemental[idx]).filter((v): v is number => v !== null);
    const lo = Math.floor(Math.min(...values));
    const hi = Math.ceil(Math.max(...values));
    const bins = 12;
    const step = (hi - lo) / bins || 1;
    const breaks = Array.from({ length: bins + 1 }, (_, i) => Math.round((lo + i * step) * 100) / 100);
    const counts = new Array<number>(bins).fill(0);
    for (const v of values) counts[Math.min(bins - 1, Math.floor((v - lo) / step))]! += 1;
    return json({ path: body.path, revision_id: 'demo-1', column: body.column, breaks, counts }, 600);
  }
  if (p === '/api/v1/explore/crosstab') {
    const rows = combinedRows(body);
    const counts = new Map<string, number>();
    for (const r of rows) {
      const key = `${groupOfRow(r)}\u0000${r.descriptive[1]}`;
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return json({
      path: body.path,
      revision_id: 'demo-1',
      summary_method: 'count',
      kind: 'count',
      rows: [...counts].map(([key, count]) => {
        const [group, value] = key.split('\u0000');
        return { group, value, count };
      }),
    }, 400);
  }
  if (p === '/api/v1/explore/compositional-profile') {
    const rows = combinedRows(body).slice(0, 80);
    const columns = (body.columns as string[] | undefined) ?? ELEMENTS;
    return json({
      path: body.path,
      revision_id: 'demo-1',
      column_names: columns,
      rows: rows.flatMap((r, i) =>
        columns.map((c) => ({ rowid: i + 1, element: c, value: r.elemental[ELEMENTS.indexOf(c)] ?? null, group_label: body.group_column ? groupOfRow(r) : null })),
      ),
    }, 700);
  }
  if (p === '/api/v1/ordination/pca') {
    const rows = combinedRows(body);
    const columns = (body.columns as string[] | undefined) ?? ELEMENTS;
    const n = columns.length;
    const variance = Array.from({ length: n }, (_, i) => 0.55 / (i + 1) ** 1.6);
    const total = variance.reduce((a, b) => a + b, 0);
    const explained = variance.map((v) => v / total);
    let cumulative = 0;
    return json({
      path: body.path,
      revision_id: 'demo-1',
      column_names: columns,
      score_names: columns.map((_, i) => `PC${i + 1}`),
      sdev: explained.map((v) => Math.sqrt(v * 10)),
      explained_variance: explained,
      cumulative_variance: explained.map((v) => (cumulative += v)),
      center: columns.map(() => 0),
      scale: null,
      rotation: columns.map((_, i) => columns.map((__, j) => Math.round(Math.cos(i + j) * 1000) / 1000)),
      scores: scores(rows, n, 4),
    }, 700);
  }
  if (p === '/api/v1/ordination/umap') {
    const rows = combinedRows(body);
    return json({ path: body.path, revision_id: 'demo-1', score_names: ['V1', 'V2'], embedding: scores(rows, 2, 6), seed: 42, n_neighbors: 15, n_epochs: 200, a: 1.58, b: 0.9, warnings: [] }, 1500);
  }
  if (p === '/api/v1/ordination/lda') {
    const rows = combinedRows(body);
    return json({
      path: body.path,
      revision_id: 'demo-1',
      levels: GROUPS,
      prior: GROUPS.map(() => 0.25),
      counts: GROUPS.map(() => ROWS_PER_GROUP),
      means: [],
      scaling: [],
      svd: [3, 2, 1],
      score_names: ['LD1', 'LD2', 'LD3'],
      scores: scores(rows, 3, 5),
      warnings: [],
    }, 900);
  }
  if (p === '/api/v1/jobs' && method === 'POST') {
    const analysis = (body as { analysis: { kind: string; request: Record<string, unknown> } }).analysis;
    const now = Date.now();
    return json({
      id: `demo-job-${++jobCounter}`,
      state: 'succeeded',
      stage: null,
      progress: 100,
      submitted_at_ms: now,
      started_at_ms: now,
      completed_at_ms: now,
      result: { kind: analysis.kind, result: clusterResult(analysis.kind, analysis.request) },
      error: null,
    }, 1200);
  }
  if (p.startsWith('/api/v1/exports/')) {
    return json({ file_name: 'demo.csv', media_type: 'text/csv', content: 'demo\n' }, 300);
  }
  return json({ code: 'demo_unsupported', message: `Not available in demo mode: ${method} ${p}` }, 50, 404);
};

/** Event source stub: demo jobs finish immediately, so no events are sent. */
export const demoEventSource = (): EventSourceLike => ({ addEventListener: () => {}, removeEventListener: () => {}, close: () => {}, onerror: null });
