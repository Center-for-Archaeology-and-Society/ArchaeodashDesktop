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
import { BusyNote } from '../ui/Busy.tsx';
import { TableCard, num, type TableSheet } from '../ui/TableExport.tsx';
import { PageHeader } from '../ui/PageHeader.tsx';
import { TabPanels } from '../ui/TabPanels.tsx';

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

/** Score scatter with labelled axes (Plotly, SVG renderer), colored by group. */
function ScatterPlot({
  x,
  y,
  xLabel,
  yLabel,
  groups,
  ids,
  title,
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
}): ReactElement | null {
  const data = useMemo(() => {
    const labels = groups && groups.length === x.length ? groups : x.map(() => 'All rows');
    const names = [...new Set(labels)];
    const colors = names.length === 1 ? ['#1f8a63'] : groupColors(names.length);
    return names.map((name, gi) => {
      const idx = labels.flatMap((g, i) => (g === name ? [i] : []));
      return {
        type: 'scatter',
        mode: 'markers',
        name,
        x: idx.map((i) => x[i]),
        y: idx.map((i) => y[i]),
        text: idx.map((i) => `${ids?.[i] || `Row ${i + 1}`} · ${name}`),
        hovertemplate: `%{text}<br>${xLabel}: %{x:.3f}<br>${yLabel}: %{y:.3f}<extra></extra>`,
        marker: { color: colors[gi], size: 7, opacity: 0.85 },
      } as Data;
    });
  }, [x, y, xLabel, yLabel, groups, ids]);
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

/** Pure PCA result presentation (scree chart, variance table, scores). */
export function PcaResult({
  result,
  onRecompute,
  onExportScores,
  groups,
  rows,
  columns,
}: {
  result: PcaResponse;
  onRecompute: () => void;
  onExportScores?: () => void;
  groups?: readonly string[];
  rows?: RowLabels;
  /** Input columns (loading rows); defaults to the response's column names. */
  columns?: readonly string[];
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
  const loadingNames = (columns ?? (result as PcaResponse & { column_names?: string[] }).column_names ?? []).slice(0, result.rotation.length);
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
              <button type="button" className="btn btn-sm btn-ghost" onClick={onExportScores} title="Scores with every descriptive column (CSV)">
                Scores with sample data (CSV)
              </button>
            )}
          </div>
        </header>
        <ScatterPlot
          x={result.scores.map((r) => r[0] ?? 0)}
          y={result.scores.map((r) => r[1] ?? 0)}
          xLabel={pc1}
          yLabel={pc2}
          groups={groups}
          ids={rows?.ids}
          title="PCA scores"
        />
      </section>
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
  /** Sample IDs and groups per row, for tables and hover text. */
  readonly rows?: RowLabels;
  /** Columns to ordinate (Analysis setup); defaults to every measured column. */
  readonly columns?: readonly string[];
  /** Analysis setup transform applied on demand before ordinating. */
  readonly transformation?: TransformationDefinition;
}

function PcaView({ deps, data, extraPaths, groups, rows, columns, transformation }: { deps: OrdinationDeps } & OrdinationInput): ReactElement {
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
      {state.kind === 'loading' && <BusyNote>Computing PCA…</BusyNote>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && (
        <PcaResult
          result={state.data}
          groups={groups}
          rows={rows}
          columns={request.columns}
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

function UmapView({ deps, data, extraPaths, groups, rows, columns, transformation }: { deps: OrdinationDeps } & OrdinationInput): ReactElement {
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
  if (state.kind === 'loading') return <BusyNote>Computing UMAP…</BusyNote>;
  if (state.kind === 'error') return <p role="alert">Error: {state.message}</p>;
  if (state.kind !== 'loaded') return <p>No UMAP yet.</p>;
  return <UmapResult result={state.data} groups={groups} rows={rows} />;
}

/** Pure UMAP result presentation (seed echo, embedding scatter, score table). */
export function UmapResult({ result, groups, rows }: { result: UmapResponse; groups?: readonly string[]; rows?: RowLabels }): ReactElement {
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
        <ScatterPlot
          x={result.embedding.map((r) => r[0] ?? 0)}
          y={result.embedding.map((r) => r[1] ?? 0)}
          xLabel="V1"
          yLabel="V2"
          groups={groups}
          ids={rows?.ids}
          title="UMAP embedding"
        />
      </section>
      <ScoreTable title="Score table" fileName="UMAP scores" scoreNames={result.score_names} scores={result.embedding} rows={rows} />
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
  rows,
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
      {state.kind === 'loaded' && (
        <>
          <section className="card">
            <header className="card-header">
              <h2 className="card-title">Linear discriminants</h2>
            </header>
            <p className="card-note">
              Groups: {state.data.levels.join(', ')} — priors{' '}
              {state.data.prior.map((p) => p.toFixed(3)).join(', ')}
            </p>
            {state.data.warnings.length > 0 && (
              <p role="status" className="callout callout-info">Warnings: {state.data.warnings.join('; ')}</p>
            )}
            <ScatterPlot
              x={state.data.scores.map((r) => r[0] ?? 0)}
              y={state.data.scores.map((r) => r[1] ?? 0)}
              xLabel="LD1"
              yLabel="LD2"
              groups={colorBy}
              ids={rows?.ids}
              title="LDA scores"
            />
          </section>
          <ScoreTable
            title="Score table"
            fileName="LDA scores"
            scoreNames={state.data.score_names}
            scores={state.data.scores}
            rows={rows ? { ...rows, groups: colorBy } : undefined}
          />
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
          pca: () => <PcaView deps={deps} data={data} extraPaths={extraPaths} groups={combined.groupOf} rows={rowLabels} columns={input.columns} transformation={input.transformation} />,
          umap: () => <UmapView deps={deps} data={data} extraPaths={extraPaths} groups={combined.groupOf} rows={rowLabels} columns={input.columns} transformation={input.transformation} />,
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
