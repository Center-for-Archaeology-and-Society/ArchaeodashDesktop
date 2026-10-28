/**
 * Ordination route (Section 8.5–8.7): PCA with explained-variance bars, a
 * score scatter (group or cos² colouring)/table, the variables plot coloured by
 * contribution and the min(4, K) contribution table; UMAP embedding with the
 * fixed legacy seed default; and LDA group-gated on the first descriptive
 * column, with the LD1/LD2 vector view or its one-dimensional alternative. Ordinations are ephemeral
 * (Section 5): nothing is persisted; recompute re-requests. PCA scores export
 * as ephemeral CSV via the exports service (Section 7.3).
 */
import { useCallback, useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
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
import { realignRowsByUuid, rowContentKey } from '../datasets/rowKeys.ts';
import { asRowsResponse, defaultGroupColumn, inputPaths } from '../visualize/combine.ts';
import { groupColors } from '../visualize/visualize-model.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { UMAP_DEFAULT_DIMENSIONS, UMAP_SEED, analysisInput, setupSummary, umapMaxDimensions } from '../analysis-setup/setup.ts';
import { BusyLabel, BusyNote } from '../ui/Busy.tsx';
import { TableCard, num, type TableSheet } from '../ui/TableExport.tsx';
import { PageHeader } from '../ui/PageHeader.tsx';
import { TabPanels } from '../ui/TabPanels.tsx';
import { ContributionTableCard, LdaScalingBars, LdaStripPlot, LdaVectorPlot, PcaVariablesPlot } from './OrdinationCharts.tsx';
import { GRADIENT_SCALE, componentCount, individualCos2, ldaLayout } from './ordination-model.ts';
import { RERUN_DELAY_MS, supersedingRuns, waitUnlessAborted } from './rerun.ts';
import { plotlyText } from '../charts/plotly-text.ts';

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

/** Sample labels and groups aligned with the ordinated rows. */
export interface RowLabels {
  /** Name of the ID column (ANID). */
  readonly idColumn: string;
  readonly ids: readonly string[];
  readonly groups?: readonly string[];
}

const ROWS_SHOWN = 200;

/** Score sheet: ID, group, then one column per score. */
export function scoreSheet(name: string, scoreNames: readonly string[], scores: readonly number[][], rows?: RowLabels): TableSheet {
  return {
    name,
    columns: [rows?.idColumn ?? 'Row', 'Group', ...scoreNames],
    rows: scores.map((row, i) => [rows?.ids[i] || `Row ${i + 1}`, rows?.groups?.[i] ?? null, ...row.map(num)]),
  };
}

function ScoreTable({
  title,
  fileName,
  scoreNames,
  scores,
  rows,
}: {
  title: string;
  fileName: string;
  scoreNames: string[];
  scores: number[][];
  rows?: RowLabels;
}): ReactElement {
  const [shown, setShown] = useState(ROWS_SHOWN);
  const sheet = useMemo(() => scoreSheet(title, scoreNames, scores, rows), [title, scoreNames, scores, rows]);
  return (
    <TableCard
      title={title}
      fileName={fileName}
      sheets={() => [sheet]}
      note={`${scores.length} rows${scores.length > shown ? `; showing ${shown}` : ''}.`}
    >
      <table className="data-table numeric">
        <thead>
          <tr>
            {sheet.columns.map((name) => (
              <th key={name}>{name}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sheet.rows.slice(0, shown).map((row, i) => (
            <tr key={i}>
              {row.map((value, j) => (
                <td key={j}>{typeof value === 'number' ? value.toFixed(4) : value ?? 'NA'}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {shown < scores.length && (
        <button type="button" className="btn btn-sm" onClick={() => setShown((n) => n + ROWS_SHOWN)}>
          Show {ROWS_SHOWN} more rows
        </button>
      )}
    </TableCard>
  );
}

/** Score scatter with labelled axes (Plotly, SVG renderer), colored by group
 *  or, when `colorValues` is given, by a continuous value with a colorbar. */
function ScatterPlot({
  x,
  y,
  xLabel,
  yLabel,
  groups,
  ids,
  title,
  colorValues,
  colorTitle = 'value',
}: {
  x: number[];
  y: number[];
  xLabel: string;
  yLabel: string;
  /** Group name per point; omitted or mismatched draws one series. */
  groups?: readonly string[];
  /** Sample ID (ANID) per point, for hover text. */
  ids?: readonly string[];
  /** Name for saved images. */
  title?: string;
  /** Continuous value per point (e.g. cos²); replaces group colouring. */
  colorValues?: readonly number[];
  colorTitle?: string;
}): ReactElement | null {
  const data = useMemo(() => {
    const labels = groups && groups.length === x.length ? groups : x.map(() => 'All rows');
    if (colorValues && colorValues.length === x.length) {
      return [
        {
          type: 'scatter',
          mode: 'markers',
          name: colorTitle,
          x,
          y,
          text: labels.map((g, i) => plotlyText(`${ids?.[i] || `Row ${i + 1}`} · ${g}`)),
          customdata: [...colorValues],
          hovertemplate: `%{text}<br>${xLabel}: %{x:.3f}<br>${yLabel}: %{y:.3f}<br>${colorTitle}: %{customdata:.3f}<extra></extra>`,
          marker: {
            color: [...colorValues],
            colorscale: GRADIENT_SCALE,
            showscale: true,
            colorbar: { title: { text: colorTitle }, thickness: 12 },
            size: 7,
            opacity: 0.85,
          },
        } as Data,
      ];
    }
    const names = [...new Set(labels)];
    const colors = names.length === 1 ? ['#1f8a63'] : groupColors(names.length);
    return names.map((name, gi) => {
      const idx = labels.flatMap((g, i) => (g === name ? [i] : []));
      return {
        type: 'scatter',
        mode: 'markers',
        name: plotlyText(name),
        x: idx.map((i) => x[i]),
        y: idx.map((i) => y[i]),
        text: idx.map((i) => plotlyText(`${ids?.[i] || `Row ${i + 1}`} · ${name}`)),
        hovertemplate: `%{text}<br>${xLabel}: %{x:.3f}<br>${yLabel}: %{y:.3f}<extra></extra>`,
        marker: { color: colors[gi], size: 7, opacity: 0.85 },
      } as Data;
    });
  }, [x, y, xLabel, yLabel, groups, ids, colorValues, colorTitle]);
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
      saveName={title ?? `${yLabel} vs ${xLabel}`}
      className="plotly-chart plot-point-scatter"
    />
  );
}

/** Components shown in the scree chart and variance table. */
const SCREE_COMPONENTS = 10;

type PcaColorBy = 'group' | 'cos2';

/** Pure PCA result presentation (scree chart, variance table, scores,
 *  variables plot, contribution table). */
export function PcaResult({
  result,
  onRecompute,
  onExportScores,
  exportBusy = false,
  exportError,
  groups,
  rows,
  columns,
  initialColorBy = 'group',
}: {
  result: PcaResponse;
  onRecompute: () => void;
  onExportScores?: (format: 'csv' | 'xlsx') => void;
  /** The score export is running. */
  exportBusy?: boolean;
  /** Message from the last failed score export. */
  exportError?: string;
  groups?: readonly string[];
  rows?: RowLabels;
  /** Input columns (loading rows); defaults to the response's column names. */
  columns?: readonly string[];
  initialColorBy?: PcaColorBy;
}): ReactElement {
  const [colorBy, setColorBy] = useState<PcaColorBy>(initialColorBy);
  const components = Math.min(componentCount(result.scores), result.score_names.length);
  const scoreX = useMemo(() => result.scores.map((r) => r[0] ?? 0), [result]);
  const scoreY = useMemo(() => result.scores.map((r) => r[1] ?? 0), [result]);
  const cos2 = useMemo(() => (colorBy === 'cos2' ? individualCos2(result.scores, 0, 1) : undefined), [result, colorBy]);
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
        hovertemplate: '%{x}: %{y:.1f}%<extra></extra>',
      } as Data,
      {
        type: 'scatter',
        mode: 'lines+markers',
        name: 'Cumulative',
        x: names,
        y: result.cumulative_variance.slice(0, k).map((v) => v * 100),
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
  const varianceSheet: TableSheet = {
    name: 'Explained variance',
    columns: ['Component', 'Variance (%)', 'Cumulative (%)', 'Standard deviation'],
    rows: result.score_names.map((name, i) => [name, num((result.explained_variance[i] ?? NaN) * 100), num((result.cumulative_variance[i] ?? NaN) * 100), num(result.sdev[i])]),
  };
  const loadingNames = useMemo(
    () => (columns ?? (result as PcaResponse & { column_names?: string[] }).column_names ?? []).slice(0, result.rotation.length),
    [columns, result],
  );
  const loadingsSheet: TableSheet = {
    name: 'Loadings',
    columns: ['Element', ...result.score_names],
    rows: result.rotation.map((row, i) => [loadingNames[i] ?? `Column ${i + 1}`, ...row.map(num)]),
  };
  const pc1 = `${result.score_names[0] ?? 'PC1'} (${pct(result.explained_variance[0])})`;
  const pc2 = `${result.score_names[1] ?? 'PC2'} (${pct(result.explained_variance[1])})`;
  return (
    <div className="results">
      <div className="ordination-scree">
        <section className="card">
          <header className="card-header"><h2 className="card-title">Explained variance</h2></header>
          <PlotlyChart data={scree} layout={screeLayout} ariaLabel="Explained variance per component" saveName="PCA explained variance" height={300} />
        </section>
        <TableCard title="Variance" fileName="PCA explained variance" sheets={() => [varianceSheet, loadingsSheet]}>
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
        </TableCard>
      </div>
      {result.scale === null || result.scale === undefined ? (
        <p className="muted">
          Unscaled PCA: on untransformed values, elements with large concentrations (e.g. Fe, Na,
          K) dominate the first components. For compositional work choose Log10 in the Data
          Manager&apos;s Analysis setup.
        </p>
      ) : null}
      <section className="card">
        <header className="card-header">
          <h2 className="card-title">Scores</h2>
          <div className="card-actions">
            <button type="button" className="btn btn-sm btn-ghost" onClick={onRecompute}>
              Recompute
            </button>
            {onExportScores && (
              <>
                <button type="button" className="btn btn-sm btn-ghost" onClick={() => onExportScores('xlsx')} disabled={exportBusy} title="Scores with every descriptive column (Excel workbook)">
                  <BusyLabel busy={exportBusy}>Scores with sample data (Excel)</BusyLabel>
                </button>
                <button type="button" className="btn btn-sm btn-ghost" onClick={() => onExportScores('csv')} disabled={exportBusy} title="Scores with every descriptive column (CSV)">
                  CSV
                </button>
              </>
            )}
          </div>
        </header>
        {exportError && <p role="alert" className="export-error">Export failed: {exportError}</p>}
        {components >= 2 ? (
          <>
            <label className="field field-inline">
              Color by
              <select value={colorBy} onChange={(e) => setColorBy(e.target.value as PcaColorBy)}>
                <option value="group">Group</option>
                <option value="cos2">Quality of representation (cos²)</option>
              </select>
            </label>
            {colorBy === 'cos2' && (
              <p className="card-note">
                cos² on {result.score_names[0]}–{result.score_names[1]}: the share of each sample&apos;s squared distance from the centre that these two components capture.
              </p>
            )}
            <ScatterPlot
              x={scoreX}
              y={scoreY}
              xLabel={pc1}
              yLabel={pc2}
              groups={groups}
              ids={rows?.ids}
              title={colorBy === 'cos2' ? 'PCA scores (cos2)' : 'PCA scores'}
              colorValues={cos2}
              colorTitle="cos²"
            />
          </>
        ) : (
          <p role="status">Only one principal component was computed; the score and variable plots need two.</p>
        )}
      </section>
      {components >= 2 && componentCount(result.rotation) >= 2 && (
        <section className="card">
          <header className="card-header"><h2 className="card-title">Variables</h2></header>
          <p className="card-note">
            Arrows to each variable&apos;s coordinates (loading × component standard deviation), coloured by its contribution (%) to {result.score_names[0]} and {result.score_names[1]}, weighted by their eigenvalues.
            {result.scale ? ' The circle is the unit correlation circle.' : ''}
          </p>
          <PcaVariablesPlot rotation={result.rotation} sdev={result.sdev} names={loadingNames} xLabel={pc1} yLabel={pc2} scaled={Boolean(result.scale)} />
        </section>
      )}
      <ContributionTableCard rotation={result.rotation} names={loadingNames} scoreNames={result.score_names} />
      <ScoreTable title="Score table" fileName="PCA scores" scoreNames={result.score_names} scores={result.scores} rows={rows} />
      <TableCard title="Loadings" fileName="PCA loadings" sheets={() => [loadingsSheet]}>
        <table className="data-table numeric">
          <thead>
            <tr>
              {loadingsSheet.columns.map((c) => (
                <th key={c}>{c}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {loadingsSheet.rows.map((row, i) => (
              <tr key={i}>
                {row.map((v, j) => (
                  <td key={j}>{typeof v === 'number' ? v.toFixed(4) : v ?? 'NA'}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </TableCard>
    </div>
  );
}

function useOrdination<TResp>(
  request: (signal: AbortSignal) => Promise<TResp>,
  deps: readonly unknown[],
): { state: AsyncState<TResp>; reload: () => void } {
  const [state, setState] = useState<AsyncState<TResp>>({ kind: 'idle' });
  // Only the latest request may run: starting one aborts the previous run,
  // which cancels its backend work, and its late response is dropped.
  const [runs] = useState(supersedingRuns);
  // The first run starts at once; reruns from changed inputs (group
  // selection, setup) wait RERUN_DELAY_MS so a burst of changes runs once.
  const started = useRef(false);
  const run = useCallback(async (wait: boolean) => {
    const signal = runs.next();
    setState({ kind: 'loading' });
    if (wait && !(await waitUnlessAborted(RERUN_DELAY_MS, signal))) return;
    try {
      const r = await request(signal);
      if (!signal.aborted) setState({ kind: 'loaded', data: r });
    } catch (err: unknown) {
      if (!signal.aborted) setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  useEffect(() => {
    const wait = started.current;
    started.current = true;
    void run(wait);
    return runs.stop;
  }, [run, runs]);
  return { state, reload: () => void run(false) };
}

/** The rows being ordinated: combined selected groups, request paths, and
 *  the group name of each row (for coloring). */
interface OrdinationInput {
  readonly data: GroupRowsResponse;
  readonly extraPaths: readonly string[];
  readonly groups?: readonly string[];
  /** Sample IDs and groups per row, for tables and hover text. */
  readonly rows?: RowLabels;
  /** Columns to ordinate (Analysis setup); defaults to every measured column. */
  readonly columns?: readonly string[];
  /** Analysis setup transform applied on demand before ordinating. */
  readonly transformation?: TransformationDefinition;
}

/**
 * Rows identity for PCA and UMAP, which depend only on the loaded samples and
 * their values: they re-run when `rowsKey` (see rowKeys.ts) or the settings
 * change, not when samples move between groups, and their scores are matched
 * to the current rows by sample UUID.
 */
interface RowIdentity {
  readonly rowsKey: string;
  readonly rowUuids: readonly string[];
}

/** Scores and the UUIDs of the rows they were computed for. */
interface ForRows<T> {
  readonly uuids: readonly string[];
  readonly result: T;
}

/** Latest value in a ref, for content-keyed requests that read it. */
function useLatest<T>(value: T): { readonly current: T } {
  const ref = useRef(value);
  ref.current = value;
  return ref;
}

function PcaView({ deps, data, extraPaths, groups, rows, columns, transformation, rowsKey, rowUuids }: { deps: OrdinationDeps } & OrdinationInput & RowIdentity): ReactElement {
  const request = useMemo(
    () => ({
      path: data.path,
      extra_paths: [...extraPaths],
      columns: [...(columns ?? data.elemental_columns)],
      transformation: transformation ?? null,
    }),
    [data, extraPaths, columns, transformation],
  );
  const latest = useLatest({ request, rowUuids });
  const settingsKey = JSON.stringify([request.columns, request.transformation]);
  const { state: computed, reload } = useOrdination<ForRows<PcaResponse>>(async (signal) => {
    const { request: current, rowUuids: uuids } = latest.current;
    return { uuids, result: await deps.ordination.pca(current, { signal }) };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deps, rowsKey, settingsKey]);
  const state = useMemo((): AsyncState<PcaResponse> => {
    if (computed.kind !== 'loaded') return computed;
    const { uuids, result } = computed.data;
    return { kind: 'loaded', data: { ...result, scores: realignRowsByUuid(uuids, result.scores, rowUuids) } };
  }, [computed, rowUuids]);
  const [exportBusy, setExportBusy] = useState(false);
  const [exportError, setExportError] = useState('');
  const exportScores = (format: 'csv' | 'xlsx') => {
    setExportBusy(true);
    setExportError('');
    void Promise.resolve()
      .then(() => deps.exports.pcaScores({ ...request, format }))
      .then(downloadExportResult)
      .catch((cause: unknown) => setExportError(cause instanceof Error ? cause.message : String(cause)))
      .finally(() => setExportBusy(false));
  };
  return (
    <>
      {state.kind === 'loading' && <BusyNote>Computing PCA…</BusyNote>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && (
        <PcaResult
          result={state.data}
          groups={groups}
          rows={rows}
          columns={request.columns}
          onRecompute={reload}
          onExportScores={exportScores}
          exportBusy={exportBusy}
          exportError={exportError}
        />
      )}
      {state.kind === 'idle' && <p>No PCA yet.</p>}
    </>
  );
}

function UmapView({ deps, data, extraPaths, groups, rows, columns, transformation, rowsKey, rowUuids }: { deps: OrdinationDeps } & OrdinationInput & RowIdentity): ReactElement {
  const [dimensions, setDimensions] = useState(UMAP_DEFAULT_DIMENSIONS);
  const requestColumns = [...(columns ?? data.elemental_columns)];
  const maxDimensions = umapMaxDimensions(requestColumns.length);
  const nComponents = Math.min(dimensions, maxDimensions);
  const request = {
    path: data.path,
    extra_paths: [...extraPaths],
    columns: requestColumns,
    transformation: transformation ?? null,
    seed: UMAP_SEED,
    n_components: nComponents,
  };
  const latest = useLatest({ request, rowUuids });
  const settingsKey = JSON.stringify([request.columns, request.transformation, nComponents]);
  const { state } = useOrdination<ForRows<UmapResponse>>(async (signal) => {
    const { request: current, rowUuids: uuids } = latest.current;
    return { uuids, result: await deps.ordination.umap(current, { signal }) };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deps, rowsKey, settingsKey]);
  const result = useMemo(
    () => (state.kind === 'loaded'
      ? { ...state.data.result, embedding: realignRowsByUuid(state.data.uuids, state.data.result.embedding, rowUuids) }
      : null),
    [state, rowUuids],
  );
  return (
    <>
      <label className="field field-inline">
        Dimensions
        <select value={nComponents} onChange={(e) => setDimensions(Number(e.target.value))}>
          {Array.from({ length: maxDimensions - 1 }, (_, i) => i + UMAP_DEFAULT_DIMENSIONS).map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
        <span className="field-hint">
          Up to {maxDimensions} for {requestColumns.length} {requestColumns.length === 1 ? 'column' : 'columns'}. Each count is its own embedding; Clustering uses the same one at the same count.
        </span>
      </label>
      {state.kind === 'loading' && <BusyNote>Computing UMAP…</BusyNote>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind !== 'loading' && state.kind !== 'error' && !result && <p>No UMAP yet.</p>}
      {state.kind === 'loaded' && result && <UmapResult result={result} groups={groups} rows={rows} />}
    </>
  );
}

/** Pure UMAP result presentation (seed echo, embedding scatter with axis
 *  choice past two dimensions, score table). */
export function UmapResult({ result, groups, rows }: { result: UmapResponse; groups?: readonly string[]; rows?: RowLabels }): ReactElement {
  const names = result.score_names;
  const [axes, setAxes] = useState<[number, number]>([0, 1]);
  // A smaller embedding than before keeps the axes in range.
  const xAxis = axes[0] < names.length ? axes[0] : 0;
  const yAxis = axes[1] < names.length ? axes[1] : Math.min(1, names.length - 1);
  const axisSelect = (label: string, value: number, set: (v: number) => void) => (
    <label className="field field-inline">
      {label}
      <select value={value} onChange={(e) => set(Number(e.target.value))}>
        {names.map((n, i) => (
          <option key={n} value={i}>
            {n}
          </option>
        ))}
      </select>
    </label>
  );
  return (
    <div className="results">
      <section className="card">
        <header className="card-header">
          <h2 className="card-title">UMAP embedding</h2>
        </header>
        <p className="card-note">
          seed {result.seed}, n_neighbors {result.n_neighbors}, n_epochs {result.n_epochs}
          {result.warnings.length > 0 && ` — warnings: ${result.warnings.join('; ')}`}. Re-running with the same seed gives the same embedding.
        </p>
        {names.length > 2 && (
          <div className="card-actions">
            {axisSelect('X axis', xAxis, (v) => setAxes([v, yAxis]))}
            {axisSelect('Y axis', yAxis, (v) => setAxes([xAxis, v]))}
          </div>
        )}
        <ScatterPlot
          x={result.embedding.map((r) => r[xAxis] ?? 0)}
          y={result.embedding.map((r) => r[yAxis] ?? 0)}
          xLabel={names[xAxis] ?? 'V1'}
          yLabel={names[yAxis] ?? 'V2'}
          groups={groups}
          ids={rows?.ids}
          title="UMAP embedding"
        />
      </section>
      <ScoreTable title="Score table" fileName="UMAP scores" scoreNames={result.score_names} scores={result.embedding} rows={rows} />
    </div>
  );
}

/**
 * Pure LDA result presentation: score scatter and LD1/LD2 vector plot when two
 * discriminants exist; with one, an LD1 strip plot by group and LD1
 * coefficient bars instead (IMPLEMENTATION.md §3.2, Section 8.7).
 */
export function LdaResult({
  result,
  groups,
  rows,
  columns,
}: {
  result: LdaResponse;
  groups?: readonly string[];
  rows?: RowLabels;
  /** Input columns (scaling rows); defaults to the response's column names. */
  columns?: readonly string[];
}): ReactElement {
  // Memoized: `layout.names` feeds the vector plot's memo, and a fresh tuple
  // per render would redraw the Plotly chart on every parent render.
  const layout = useMemo(() => ldaLayout(result), [result]);
  const variables = useMemo(
    () => (result.column_names.length > 0 ? result.column_names : columns ?? []).slice(0, result.scaling.length),
    [result, columns],
  );
  const ld1 = useMemo(() => result.scores.map((r) => r[0] ?? 0), [result]);
  const ld2 = useMemo(() => result.scores.map((r) => r[1] ?? 0), [result]);
  return (
    <>
      <section className="card">
        <header className="card-header">
          <h2 className="card-title">Linear discriminants</h2>
        </header>
        <p className="card-note">
          Groups: {result.levels.join(', ')} — priors{' '}
          {result.prior.map((p) => p.toFixed(3)).join(', ')}
        </p>
        {result.warnings.length > 0 && (
          <p role="status" className="callout callout-info">Warnings: {result.warnings.join('; ')}</p>
        )}
        {layout.kind === 'none' && <p role="alert">The LDA returned no discriminants to plot.</p>}
        {layout.kind === 'one' && (
          <>
            <p role="status" className="callout callout-info">
              Only one linear discriminant ({layout.name}) exists for these groups, so there is no {layout.name}/LD2 plane: showing {layout.name} scores by group and {layout.name} coefficients instead of the scatter and vector plot.
            </p>
            <LdaStripPlot values={ld1} groups={groups} ids={rows?.ids} axisName={layout.name} />
          </>
        )}
        {layout.kind === 'two' && (
          <ScatterPlot
            x={ld1}
            y={ld2}
            xLabel={layout.names[0]}
            yLabel={layout.names[1]}
            groups={groups}
            ids={rows?.ids}
            title="LDA scores"
          />
        )}
      </section>
      {layout.kind !== 'none' && (
        <section className="card">
          <header className="card-header">
            <h2 className="card-title">{layout.kind === 'two' ? 'Discriminant vectors' : `${layout.name} coefficients`}</h2>
          </header>
          <p className="card-note">Each variable&apos;s scaling coefficients on the discriminants.</p>
          {layout.kind === 'two' ? (
            <LdaVectorPlot scaling={result.scaling} names={variables} axisNames={layout.names} />
          ) : (
            <LdaScalingBars scaling={result.scaling} names={variables} axisName={layout.name} />
          )}
        </section>
      )}
      <ScoreTable
        title="Score table"
        fileName="LDA scores"
        scoreNames={result.score_names}
        scores={result.scores}
        rows={rows ? { ...rows, groups } : undefined}
      />
    </>
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
  rows,
}: { deps: OrdinationDeps; defaultGroupColumn?: string } & Omit<OrdinationInput, 'extraPaths'> & {
  extraPaths?: readonly string[];
}): ReactElement {
  const [groupColumn, setGroupColumn] = useState(
    defaultGroupColumn ?? data.descriptive_columns[0] ?? '',
  );
  const { state } = useOrdination<LdaResponse>(
    (signal) =>
      deps.ordination.lda(
        {
          path: data.path,
          extra_paths: [...extraPaths],
          columns: [...(columns ?? data.elemental_columns)],
          transformation: transformation ?? null,
          group_column: groupColumn,
        },
        { signal },
      ),
    [deps, data, extraPaths, groupColumn, columns, transformation],
  );

  // Stable per data/column: the scatter and strip plots memoize on it.
  const colorBy = useMemo(
    () => data.rows.map((r) => r.descriptive[data.descriptive_columns.indexOf(groupColumn)] ?? '(Missing)'),
    [data, groupColumn],
  );
  if (!groupColumn) {
    return <p role="alert">LDA needs a descriptive group column; this dataset has none.</p>;
  }
  return (
    <div className="results">
      <label className="field field-inline">
        Group column
        <select value={groupColumn} onChange={(e) => setGroupColumn(e.target.value)}>
          {data.descriptive_columns.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      {state.kind === 'loading' && <BusyNote>Computing LDA…</BusyNote>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'idle' && <p>No LDA yet.</p>}
      {state.kind === 'loaded' && <LdaResult result={state.data} groups={colorBy} rows={rows} columns={columns} />}
    </div>
  );
}

export function OrdinationPage({ deps }: { deps: OrdinationDeps }): ReactElement {
  const dataset = useGroupFiles(deps.groups);
  const { files, combined, load } = dataset;
  const [view, setView] = useState<OrdinationView>('pca');
  const data = useMemo(() => asRowsResponse(files, combined), [files, combined]);
  const extraPaths = useMemo(() => inputPaths(files).extra_paths, [files]);
  // Remount the views only for a new set of samples (a Dataset change), not
  // when samples move between groups: PCA and UMAP results stay valid then.
  const rowUuids = useMemo(() => combined.rows.map((r) => r.analytical_uuid), [combined]);
  const rowsKey = useMemo(() => rowContentKey(combined), [combined]);
  const viewKey = rowsKey;
  const analysisSetup = useAnalysisSetup();
  const input = useMemo(
    () => analysisInput(analysisSetup.setup, data.elemental_columns),
    [analysisSetup.setup, data.elemental_columns],
  );
  const rowLabels = useMemo<RowLabels>(
    () => ({
      idColumn: combined.visibleIdColumn,
      ids: combined.rows.map((r) => r.visible_id ?? r.legacy_rowid ?? ''),
      groups: combined.groupOf,
    }),
    [combined],
  );

  return (
    <section className="page" aria-labelledby="ordination-heading">
      <PageHeader id="ordination-heading" title="Ordination" description="Principal components, UMAP, and linear discriminants of the selected groups, using the Data Manager’s Analysis setup.">
        <label className="field field-inline">
          Dataset
          <GroupMultiSelect
            options={dataset.options}
            selected={dataset.selectedPaths}
            onChange={dataset.setSelectedPaths}
          />
        </label>
      </PageHeader>
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
      {load.kind === 'loading' && <BusyNote>Loading dataset…</BusyNote>}
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
        <BusyNote>Loading the analysis setup…</BusyNote>
      )}
      {load.kind === 'loaded' && files.length > 0 && analysisSetup.loaded && input.columns.length < 2 && (
        <p role="alert">Ordination needs at least two selected elements measured in these groups. Change the Analysis setup in the Data Manager.</p>
      )}
      {load.kind === 'loaded' && files.length > 0 && analysisSetup.loaded && input.columns.length >= 2 && (
        <TabPanels key={viewKey} active={view} panels={{
          pca: () => <PcaView deps={deps} data={data} extraPaths={extraPaths} groups={combined.groupOf} rows={rowLabels} columns={input.columns} transformation={input.transformation} rowsKey={rowsKey} rowUuids={rowUuids} />,
          umap: () => <UmapView deps={deps} data={data} extraPaths={extraPaths} groups={combined.groupOf} rows={rowLabels} columns={input.columns} transformation={input.transformation} rowsKey={rowsKey} rowUuids={rowUuids} />,
          lda: () => (
            <LdaView
              deps={deps}
              data={data}
              extraPaths={extraPaths}
              defaultGroupColumn={defaultGroupColumn(combined)}
              columns={input.columns}
              transformation={input.transformation}
              rows={rowLabels}
            />
          ),
        }} />
      )}
    </section>
  );
}
