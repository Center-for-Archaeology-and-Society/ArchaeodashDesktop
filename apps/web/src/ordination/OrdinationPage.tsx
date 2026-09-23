/**
 * Ordination route (Section 8.5–8.7): PCA with explained-variance bars and a
 * score scatter/table, UMAP embedding with the fixed legacy seed default, and
 * LDA group-gated on the first descriptive column. Ordinations are ephemeral
 * (Section 5): nothing is persisted; recompute re-requests.
 */
import { useCallback, useEffect, useMemo, useState, type ReactElement } from 'react';
import type {
  GroupsService,
  GroupRowsResponse,
  LdaResponse,
  OrdinationService,
  PcaResponse,
  UmapResponse,
} from '@archaeodash/client';

export interface OrdinationDeps {
  readonly groups: GroupsService;
  readonly ordination: OrdinationService;
}

type AsyncState<T> =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'loaded'; data: T };

type OrdinationView = 'pca' | 'umap' | 'lda';

const VIEWS: readonly [OrdinationView, string][] = [
  ['pca', 'PCA'],
  ['umap', 'UMAP'],
  ['lda', 'LDA'],
];

function ScoreTable({
  scoreNames,
  scores,
  rowPrefix,
}: {
  scoreNames: string[];
  scores: number[][];
  rowPrefix: string;
}): ReactElement {
  return (
    <table className="data-table">
      <thead>
        <tr>
          <th>Row</th>
          {scoreNames.map((name) => (
            <th key={name}>{name}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {scores.map((row, i) => (
          <tr key={i}>
            <td>{`${rowPrefix} ${i + 1}`}</td>
            {row.map((value, j) => (
              <td key={j}>{Number.isFinite(value) ? value.toFixed(4) : 'NA'}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ScatterPlot({
  x,
  y,
  xLabel,
  yLabel,
}: {
  x: number[];
  y: number[];
  xLabel: string;
  yLabel: string;
}): ReactElement | null {
  if (x.length === 0 || x.length !== y.length) return null;
  const xMin = Math.min(...x);
  const xMax = Math.max(...x);
  const yMin = Math.min(...y);
  const yMax = Math.max(...y);
  const spanX = xMax - xMin || 1;
  const spanY = yMax - yMin || 1;
  const points = x.map((xi, i) => {
    const px = ((xi - xMin) / spanX) * 90 + 5;
    const yi = y[i] ?? yMin;
    const py = 95 - ((yi - yMin) / spanY) * 90;
    return `${px},${py}`;
  });
  return (
    <svg
      className="profile-plot"
      role="img"
      aria-label={`Scatter of ${yLabel} by ${xLabel}`}
      viewBox="0 0 100 100"
      preserveAspectRatio="none"
    >
      {points.map((p, i) => {
        const [cx, cy] = p.split(',');
        return <circle key={i} cx={cx} cy={cy} r="0.8" className="plot-point" />;
      })}
    </svg>
  );
}

export function PcaView({
  deps,
  data,
}: {
  deps: OrdinationDeps;
  data: GroupRowsResponse;
}): ReactElement {
  const [state, setState] = useState<AsyncState<PcaResponse>>({ kind: 'idle' });

  const run = useCallback(async () => {
    setState({ kind: 'loading' });
    try {
      const r = await deps.ordination.pca({
        path: data.path,
        columns: data.elemental_columns,
      });
      setState({ kind: 'loaded', data: r });
    } catch (err: unknown) {
      setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, [deps, data]);

  useEffect(() => {
    void run();
  }, [run]);

  return (
    <>
      {state.kind === 'loading' && <p role="status">Computing PCA…</p>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && <PcaResult result={state.data} onRecompute={() => void run()} />}
      {state.kind === 'idle' && <p>No PCA yet.</p>}
    </>
  );
}

/** Pure PCA result presentation (variance bars, scatter, score table). */
export function PcaResult({ result, onRecompute }: { result: PcaResponse; onRecompute: () => void }): ReactElement {
  return (
    <div>
      <div className="variance-bars" role="img" aria-label="Explained variance per component">
        {result.explained_variance.map((v, i) => (
          <div
            key={i}
            className="histogram-bar"
            style={{ height: `${Math.max(v * 100, 1)}%` }}
            title={`PC${i + 1}: ${(v * 100).toFixed(1)}%`}
          />
        ))}
      </div>
      <p className="muted">
        Cumulative: {result.cumulative_variance.map((v) => `${(v * 100).toFixed(1)}%`).join(', ')}
      </p>
      <ScatterPlot
        x={result.scores.map((r) => r[0] ?? 0)}
        y={result.scores.map((r) => r[1] ?? 0)}
        xLabel="PC1"
        yLabel="PC2"
      />
      <ScoreTable scoreNames={result.score_names} scores={result.scores} rowPrefix="Row" />
      <button type="button" onClick={onRecompute}>
        Recompute
      </button>
    </div>
  );
}

export function UmapView({
  deps,
  data,
}: {
  deps: OrdinationDeps;
  data: GroupRowsResponse;
}): ReactElement {
  const [state, setState] = useState<AsyncState<UmapResponse>>({ kind: 'idle' });

  const run = useCallback(async () => {
    setState({ kind: 'loading' });
    try {
      const r = await deps.ordination.umap({
        path: data.path,
        columns: data.elemental_columns,
        // Fixed default seed (Section 8.7: reproducible UMAP).
        seed: 20260914,
      });
      setState({ kind: 'loaded', data: r });
    } catch (err: unknown) {
      setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, [deps, data]);

  useEffect(() => {
    void run();
  }, [run]);

  if (state.kind === 'loading') return <p role="status">Computing UMAP…</p>;
  if (state.kind === 'error') return <p role="alert">Error: {state.message}</p>;
  if (state.kind !== 'loaded') return <p>No UMAP yet.</p>;
  return (
    <div>
      <p className="muted">
        seed {state.data.seed}, n_neighbors {state.data.n_neighbors}, n_epochs {state.data.n_epochs}
        {state.data.warnings.length > 0 && ` — warnings: ${state.data.warnings.join('; ')}`}
      </p>
      <ScatterPlot
        x={state.data.embedding.map((r) => r[0] ?? 0)}
        y={state.data.embedding.map((r) => r[1] ?? 0)}
        xLabel="V1"
        yLabel="V2"
      />
      <ScoreTable
        scoreNames={state.data.score_names}
        scores={state.data.embedding}
        rowPrefix="Row"
      />
      <button type="button" onClick={() => void run()}>
        Recompute
      </button>
    </div>
  );
}

/** Pure UMAP result presentation (seed echo, scatter, score table). */
export function UmapResult({
  result,
  onRecompute,
}: {
  result: UmapResponse;
  onRecompute: () => void;
}): ReactElement {
  return (
    <div>
      <p className="muted">
        seed {result.seed}, n_neighbors {result.n_neighbors}, n_epochs {result.n_epochs}
        {result.warnings.length > 0 && ` — warnings: ${result.warnings.join('; ')}`}
      </p>
      <ScatterPlot
        x={result.embedding.map((r) => r[0] ?? 0)}
        y={result.embedding.map((r) => r[1] ?? 0)}
        xLabel="V1"
        yLabel="V2"
      />
      <ScoreTable
        scoreNames={result.score_names}
        scores={result.embedding}
        rowPrefix="Row"
      />
      <button type="button" onClick={onRecompute}>
        Recompute
      </button>
    </div>
  );
}

export function LdaView({
  deps,
  data,
}: {
  deps: OrdinationDeps;
  data: GroupRowsResponse;
}): ReactElement {
  const groupColumn = data.descriptive_columns[0] ?? '';
  const [state, setState] = useState<AsyncState<LdaResponse>>({ kind: 'idle' });

  const run = useCallback(async () => {
    if (!groupColumn) return;
    setState({ kind: 'loading' });
    try {
      const r = await deps.ordination.lda({
        path: data.path,
        columns: data.elemental_columns,
        group_column: groupColumn,
      });
      setState({ kind: 'loaded', data: r });
    } catch (err: unknown) {
      setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, [deps, data, groupColumn]);

  useEffect(() => {
    void run();
  }, [run]);

  if (!groupColumn) {
    return <p role="alert">LDA needs a descriptive group column; this dataset has none.</p>;
  }
  if (state.kind === 'loading') return <p role="status">Computing LDA…</p>;
  if (state.kind === 'error') return <p role="alert">Error: {state.message}</p>;
  if (state.kind !== 'loaded') return <p>No LDA yet.</p>;
  return (
    <div>
      <p className="muted">
        Groups: {state.data.levels.join(', ')} — priors{' '}
        {state.data.prior.map((p) => p.toFixed(3)).join(', ')}
      </p>
      {state.data.warnings.length > 0 && (
        <p role="status">Warnings: {state.data.warnings.join('; ')}</p>
      )}
      <ScatterPlot
        x={state.data.scores.map((r) => r[0] ?? 0)}
        y={state.data.scores.map((r) => r[1] ?? 0)}
        xLabel="LD1"
        yLabel="LD2"
      />
      <ScoreTable
        scoreNames={state.data.score_names}
        scores={state.data.scores}
        rowPrefix="Row"
      />
      <button type="button" onClick={() => void run()}>
        Recompute
      </button>
    </div>
  );
}

export function OrdinationPage({ deps }: { deps: OrdinationDeps }): ReactElement {
  const [candidates, setCandidates] = useState<Awaited<ReturnType<GroupsService['scan']>>>([]);
  const [selectedPath, setSelectedPath] = useState('');
  const [data, setData] = useState<GroupRowsResponse | null>(null);
  const [view, setView] = useState<OrdinationView>('pca');

  const ready = useMemo(
    () => candidates.filter((c) => c.ready && c.group !== null && c.group !== undefined),
    [candidates],
  );

  useEffect(() => {
    let cancelled = false;
    deps.groups
      .scan()
      .then((list) => {
        if (cancelled) return;
        setCandidates(list);
        const first = list.find((c) => c.ready && c.group);
        setSelectedPath((current) => current || first?.path || '');
      })
      .catch(() => {
        if (!cancelled) setCandidates([]);
      });
    return () => {
      cancelled = true;
    };
  }, [deps]);

  useEffect(() => {
    if (!selectedPath) return;
    let cancelled = false;
    setData(null);
    deps.groups
      .rows(selectedPath)
      .then((rows) => {
        if (!cancelled) setData(rows);
      })
      .catch(() => {
        if (!cancelled) setData(null);
      });
    return () => {
      cancelled = true;
    };
  }, [deps, selectedPath]);

  return (
    <section aria-labelledby="ordination-heading">
      <h1 id="ordination-heading">Ordination</h1>
      <div className="explore-controls">
        <label>
          Dataset
          <select
            aria-label="Dataset"
            value={selectedPath}
            onChange={(e) => setSelectedPath(e.target.value)}
          >
            <option value="" disabled>
              Choose a group file…
            </option>
            {ready.map((c) => (
              <option key={c.path} value={c.path}>
                {c.group?.group_name} ({c.path})
              </option>
            ))}
          </select>
        </label>
      </div>
      <div role="tablist" aria-label="Ordination views" className="pills">
        {VIEWS.map(([key, label]) => (
          <button
            key={key}
            type="button"
            role="tab"
            aria-selected={view === key}
            className={view === key ? 'pill active' : 'pill'}
            onClick={() => setView(key)}
          >
            {label}
          </button>
        ))}
      </div>
      {!data && <p role="status">Load a dataset to run ordination.</p>}
      {data && view === 'pca' && <PcaView deps={deps} data={data} />}
      {data && view === 'umap' && <UmapView deps={deps} data={data} />}
      {data && view === 'lda' && <LdaView deps={deps} data={data} />}
    </section>
  );
}
