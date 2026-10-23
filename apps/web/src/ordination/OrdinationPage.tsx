/**
 * Ordination route (Section 8.5–8.7): PCA with explained-variance bars and a
 * score scatter/table, UMAP embedding with the fixed legacy seed default, and
 * LDA group-gated on the first descriptive column. Ordinations are ephemeral
 * (Section 5): nothing is persisted; recompute re-requests. PCA scores export
 * as ephemeral CSV via the exports service (Section 7.3).
 */
import { useCallback, useEffect, useMemo, useState, type ReactElement } from 'react';
import type {
  ExportsService,
  GroupsService,
  GroupRowsResponse,
  LdaResponse,
  OrdinationService,
  PcaResponse,
  TransformationDefinition,
  UmapResponse,
} from '@archaeodash/client';
import type { Data } from 'plotly.js-dist-min';
import { downloadExportResult } from '../exports.ts';
import { PlotlyChart } from '../charts/PlotlyChart.tsx';
import { GroupMultiSelect } from '../datasets/GroupMultiSelect.tsx';
import { useGroupFiles } from '../datasets/useGroupFiles.ts';
import { asRowsResponse, defaultGroupColumn, inputPaths } from '../visualize/combine.ts';
import { groupColors } from '../visualize/visualize-model.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { UMAP_SEED, analysisInput, setupSummary } from '../analysis-setup/setup.ts';

export interface OrdinationDeps {
  readonly groups: GroupsService;
  readonly ordination: OrdinationService;
  readonly exports: ExportsService;
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

/** Score scatter with labelled axes (Plotly, SVG renderer), colored by group. */
function ScatterPlot({
  x,
  y,
  xLabel,
  yLabel,
  groups,
}: {
  x: number[];
  y: number[];
  xLabel: string;
  yLabel: string;
  /** Group name per point; omitted or mismatched draws one series. */
  groups?: readonly string[];
}): ReactElement | null {
  const data = useMemo(() => {
    const labels = groups && groups.length === x.length ? groups : x.map(() => 'All rows');
    const names = [...new Set(labels)];
    const colors = names.length === 1 ? ['#1e8f66'] : groupColors(names.length);
    return names.map((name, gi) => {
      const idx = labels.flatMap((g, i) => (g === name ? [i] : []));
      return {
        type: 'scatter',
        mode: 'markers',
        name,
        x: idx.map((i) => x[i]),
        y: idx.map((i) => y[i]),
        text: idx.map((i) => `${name} · row ${i + 1}`),
        hovertemplate: `%{text}<br>${xLabel}: %{x:.3f}<br>${yLabel}: %{y:.3f}<extra></extra>`,
        marker: { color: colors[gi], size: 7, opacity: 0.85 },
      } as Data;
    });
  }, [x, y, xLabel, yLabel, groups]);
  const layout = useMemo(
    () => ({
      xaxis: { title: { text: xLabel }, zeroline: true },
      yaxis: { title: { text: yLabel }, zeroline: true },
      showlegend: data.length > 1,
    }),
    [xLabel, yLabel, data.length],
  );
  if (x.length === 0 || x.length !== y.length) return null;
  return (
    <PlotlyChart
      data={data}
      layout={layout}
      ariaLabel={`Scatter of ${yLabel} by ${xLabel}`}
      className="plotly-chart plot-point-scatter"
    />
  );
}

/** Components shown in the scree chart and variance table. */
const SCREE_COMPONENTS = 10;

/** Pure PCA result presentation (scree chart, variance table, scores). */
export function PcaResult({
  result,
  onRecompute,
  onExportScores,
  groups,
}: {
  result: PcaResponse;
  onRecompute: () => void;
  onExportScores?: () => void;
  groups?: readonly string[];
}): ReactElement {
  const k = Math.min(SCREE_COMPONENTS, result.explained_variance.length);
  const names = result.score_names.slice(0, k);
  const pct = (v: number | undefined) => `${((v ?? 0) * 100).toFixed(1)}%`;
  const scree = useMemo(
    () => [
      {
        type: 'bar',
        name: 'Explained',
        x: names,
        y: result.explained_variance.slice(0, k).map((v) => v * 100),
        marker: { color: '#48c895' },
        hovertemplate: '%{x}: %{y:.1f}%<extra></extra>',
      } as Data,
      {
        type: 'scatter',
        mode: 'lines+markers',
        name: 'Cumulative',
        x: names,
        y: result.cumulative_variance.slice(0, k).map((v) => v * 100),
        line: { color: '#163126' },
        hovertemplate: '%{x}: %{y:.1f}% cumulative<extra></extra>',
      } as Data,
    ],
    [result, k, names],
  );
  const screeLayout = useMemo(
    () => ({
      yaxis: { title: { text: '% of variance' }, range: [0, 105] as [number, number] },
      legend: { orientation: 'h' as const, y: 1.12 },
      bargap: 0.25,
    }),
    [],
  );
  const pc1 = `${result.score_names[0] ?? 'PC1'} (${pct(result.explained_variance[0])})`;
  const pc2 = `${result.score_names[1] ?? 'PC2'} (${pct(result.explained_variance[1])})`;
  return (
    <div>
      <h2>Explained variance</h2>
      <div className="ordination-scree">
        <PlotlyChart data={scree} layout={screeLayout} ariaLabel="Explained variance per component" height={300} />
        <table className="data-table variance-table">
          <thead>
            <tr>
              <th>Component</th>
              <th>Variance</th>
              <th>Cumulative</th>
            </tr>
          </thead>
          <tbody>
            {names.map((name, i) => (
              <tr key={name}>
                <td>{name}</td>
                <td>{pct(result.explained_variance[i])}</td>
                <td>{pct(result.cumulative_variance[i])}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {result.scale === null || result.scale === undefined ? (
        <p className="muted">
          Unscaled PCA: on untransformed values, elements with large concentrations (e.g. Fe, Na,
          K) dominate the first components. For compositional work choose Log10 in the Data
          Manager&apos;s Analysis setup.
        </p>
      ) : null}
      <h2>Scores</h2>
      <ScatterPlot
        x={result.scores.map((r) => r[0] ?? 0)}
        y={result.scores.map((r) => r[1] ?? 0)}
        xLabel={pc1}
        yLabel={pc2}
        groups={groups}
      />
      <details>
        <summary>Score table</summary>
        <ScoreTable scoreNames={result.score_names} scores={result.scores} rowPrefix="Row" />
      </details>
      <div className="table-actions">
        <button type="button" onClick={onRecompute}>
          Recompute
        </button>
        {onExportScores && (
          <button type="button" onClick={onExportScores}>
            Export PCA scores (CSV)
          </button>
        )}
      </div>
    </div>
  );
}

function useOrdination<TResp>(
  request: () => Promise<TResp>,
  deps: readonly unknown[],
): { state: AsyncState<TResp>; reload: () => void } {
  const [state, setState] = useState<AsyncState<TResp>>({ kind: 'idle' });
  const run = useCallback(async () => {
    setState({ kind: 'loading' });
    try {
      const r = await request();
      setState({ kind: 'loaded', data: r });
    } catch (err: unknown) {
      setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  useEffect(() => {
    void run();
  }, [run]);
  return { state, reload: () => void run() };
}

/** The rows being ordinated: combined selected groups, request paths, and
 *  the group name of each row (for coloring). */
interface OrdinationInput {
  readonly data: GroupRowsResponse;
  readonly extraPaths: readonly string[];
  readonly groups?: readonly string[];
  /** Columns to ordinate (Analysis setup); defaults to every measured column. */
  readonly columns?: readonly string[];
  /** Analysis setup transform applied on demand before ordinating. */
  readonly transformation?: TransformationDefinition;
}

function PcaView({ deps, data, extraPaths, groups, columns, transformation }: { deps: OrdinationDeps } & OrdinationInput): ReactElement {
  const request = useMemo(
    () => ({
      path: data.path,
      extra_paths: [...extraPaths],
      columns: [...(columns ?? data.elemental_columns)],
      transformation: transformation ?? null,
    }),
    [data, extraPaths, columns, transformation],
  );
  const { state, reload } = useOrdination<PcaResponse>(() => deps.ordination.pca(request), [deps, request]);
  return (
    <>
      {state.kind === 'loading' && <p role="status">Computing PCA…</p>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && (
        <PcaResult
          result={state.data}
          groups={groups}
          onRecompute={reload}
          onExportScores={() =>
            void deps.exports
              .pcaScores(request)
              .then(downloadExportResult)
              .catch((err: unknown) => console.error(err))
          }
        />
      )}
      {state.kind === 'idle' && <p>No PCA yet.</p>}
    </>
  );
}

function UmapView({ deps, data, extraPaths, groups, columns, transformation }: { deps: OrdinationDeps } & OrdinationInput): ReactElement {
  const { state } = useOrdination<UmapResponse>(
    () =>
      deps.ordination.umap({
        path: data.path,
        extra_paths: [...extraPaths],
        columns: [...(columns ?? data.elemental_columns)],
        transformation: transformation ?? null,
        seed: UMAP_SEED,
      }),
    [deps, data, extraPaths, columns, transformation],
  );
  if (state.kind === 'loading') return <p role="status">Computing UMAP…</p>;
  if (state.kind === 'error') return <p role="alert">Error: {state.message}</p>;
  if (state.kind !== 'loaded') return <p>No UMAP yet.</p>;
  return <UmapResult result={state.data} groups={groups} />;
}

/** Pure UMAP result presentation (seed echo, embedding scatter, score table). */
export function UmapResult({ result, groups }: { result: UmapResponse; groups?: readonly string[] }): ReactElement {
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
        groups={groups}
      />
      <details>
        <summary>Score table</summary>
        <ScoreTable scoreNames={result.score_names} scores={result.embedding} rowPrefix="Row" />
      </details>
      <p className="muted">Re-run uses the same seed: the embedding is reproducible.</p>
    </div>
  );
}

/** Stable default: a fresh `[]` per render would refetch LDA every render. */
const NO_EXTRA_PATHS: readonly string[] = [];

export function LdaView({
  deps,
  data,
  extraPaths = NO_EXTRA_PATHS,
  defaultGroupColumn,
  columns,
  transformation,
}: { deps: OrdinationDeps; defaultGroupColumn?: string } & Omit<OrdinationInput, 'extraPaths'> & {
  extraPaths?: readonly string[];
}): ReactElement {
  const [groupColumn, setGroupColumn] = useState(
    defaultGroupColumn ?? data.descriptive_columns[0] ?? '',
  );
  const { state } = useOrdination<LdaResponse>(
    () =>
      deps.ordination.lda({
        path: data.path,
        extra_paths: [...extraPaths],
        columns: [...(columns ?? data.elemental_columns)],
        transformation: transformation ?? null,
        group_column: groupColumn,
      }),
    [deps, data, extraPaths, groupColumn, columns, transformation],
  );

  if (!groupColumn) {
    return <p role="alert">LDA needs a descriptive group column; this dataset has none.</p>;
  }
  const colorBy = data.rows.map(
    (r) => r.descriptive[data.descriptive_columns.indexOf(groupColumn)] ?? '(Missing)',
  );
  return (
    <div>
      <label>
        Group column
        <select value={groupColumn} onChange={(e) => setGroupColumn(e.target.value)}>
          {data.descriptive_columns.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      {state.kind === 'loading' && <p role="status">Computing LDA…</p>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'idle' && <p>No LDA yet.</p>}
      {state.kind === 'loaded' && (
        <>
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
            groups={colorBy}
          />
          <details>
            <summary>Score table</summary>
            <ScoreTable scoreNames={state.data.score_names} scores={state.data.scores} rowPrefix="Row" />
          </details>
        </>
      )}
    </div>
  );
}

export function OrdinationPage({ deps }: { deps: OrdinationDeps }): ReactElement {
  const dataset = useGroupFiles(deps.groups);
  const { files, combined, load } = dataset;
  const [view, setView] = useState<OrdinationView>('pca');
  const data = useMemo(() => asRowsResponse(files, combined), [files, combined]);
  const extraPaths = useMemo(() => inputPaths(files).extra_paths, [files]);
  const viewKey = files.map((f) => `${f.path}@${f.data.revision_id}`).join('|');
  const analysisSetup = useAnalysisSetup();
  const input = useMemo(
    () => analysisInput(analysisSetup.setup, data.elemental_columns),
    [analysisSetup.setup, data.elemental_columns],
  );

  return (
    <section aria-labelledby="ordination-heading">
      <h1 id="ordination-heading">Ordination</h1>
      <div className="explore-controls">
        <label>
          Dataset
          <GroupMultiSelect
            options={dataset.options}
            selected={dataset.selectedPaths}
            onChange={dataset.setSelectedPaths}
          />
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
      {load.kind === 'loading' && <p role="status">Loading dataset…</p>}
      {load.kind === 'error' && <p role="alert">Load failed: {load.message}</p>}
      {(load.kind !== 'loaded' || files.length === 0) && load.kind !== 'loading' && (
        <p role="status">Load a dataset to run ordination.</p>
      )}
      {load.kind === 'loaded' && files.length > 0 && analysisSetup.setup && (
        <p className="setup-in-use">
          Analysis setup: {setupSummary(analysisSetup.setup, data.elemental_columns)}
          {input.missing.length > 0 && ` (not in every selected group: ${input.missing.join(', ')})`}
        </p>
      )}
      {load.kind === 'loaded' && files.length > 0 && !analysisSetup.loaded && (
        <p role="status">Loading the analysis setup…</p>
      )}
      {load.kind === 'loaded' && files.length > 0 && analysisSetup.loaded && input.columns.length < 2 && (
        <p role="alert">Ordination needs at least two selected elements measured in these groups. Change the Analysis setup in the Data Manager.</p>
      )}
      {load.kind === 'loaded' && files.length > 0 && analysisSetup.loaded && input.columns.length >= 2 && (
        <div key={viewKey}>
          {view === 'pca' && <PcaView deps={deps} data={data} extraPaths={extraPaths} groups={combined.groupOf} columns={input.columns} transformation={input.transformation} />}
          {view === 'umap' && <UmapView deps={deps} data={data} extraPaths={extraPaths} groups={combined.groupOf} columns={input.columns} transformation={input.transformation} />}
          {view === 'lda' && (
            <LdaView
              deps={deps}
              data={data}
              extraPaths={extraPaths}
              defaultGroupColumn={defaultGroupColumn(combined)}
              columns={input.columns}
              transformation={input.transformation}
            />
          )}
        </div>
      )}
    </section>
  );
}
