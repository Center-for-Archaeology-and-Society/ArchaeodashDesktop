import { runAnalysisJob } from './job-workflow.ts';
import { analysisColumns, projectionLabels, sourceOptions } from './input-options.ts';
import { moveAcrossSources } from './multi-source-move.ts';
import { GroupMultiSelect } from '../datasets/GroupMultiSelect.tsx';
import { useGroupFiles } from '../datasets/useGroupFiles.ts';
import { asRowsResponse, defaultGroupColumn, inputPaths } from '../visualize/combine.ts';
import { notifyGroupsChanged } from '../data-manager/events.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { ANALYSIS_SETUP_NAME, analysisInput, setupSummary } from '../analysis-setup/setup.ts';
import { ResultAssignment } from './ResultAssignment.tsx';
import { AnalysisPlots } from './AnalysisPlots.tsx';
import { layoutDendrogram } from './dendrogram-model.ts';
import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactElement } from 'react';
import type {
  AnalysisSource, ClusterDistanceMetric, ClusterLinkage, TransformationDefinition, AnalysisJobSnapshot, AnalysisJobRequest,
  Transport, ClusterMethod, MembershipMethod, TransferUnitsRequest, BatchTransferUnitsRequest,
  ClusterFitResponse, ClusterDiagnosticsResponse,
  MembershipProbabilitiesResponse, EuclideanMatchesResponse, TableCell,
} from '@archaeodash/client';
import { BusyLabel, BusyNote } from '../ui/Busy.tsx';
import { TableCard, num, type TableSheet } from '../ui/TableExport.tsx';
import { PageHeader } from '../ui/PageHeader.tsx';

export type AnalysisKind = 'cluster' | 'membership' | 'euclidean';
export type AnalysisDeps = Pick<Transport, 'groups' | 'clustering' | 'jobs' | 'transformations'>;
export type AnalysisResult =
  | { kind: 'fit'; data: ClusterFitResponse }
  | { kind: 'diagnostics'; data: ClusterDiagnosticsResponse }
  | { kind: 'membership'; data: MembershipProbabilitiesResponse }
  | { kind: 'euclidean'; data: EuclideanMatchesResponse };
const titles = { cluster: 'Cluster', membership: 'Probabilities and Distances', euclidean: 'Euclidean Distance' };
const descriptions = {
  cluster: 'Group samples by similarity with k-means, k-medoids, or hierarchical clustering, then assign the clusters to groups.',
  membership: 'Hotelling membership probabilities or Mahalanobis distances of every sample to each group.',
  euclidean: 'The nearest samples to each sample by Euclidean distance.',
};
const display = (value: TableCell | undefined): string => value === null || value === undefined ? 'Unavailable' : String(value);

/**
 * Per-row labels for a result: the sample's ANID (visible ID) from the
 * loaded rows, matched by its hidden identity; `fallback(i)` otherwise.
 */
export interface RowInfo {
  readonly idColumn: string;
  readonly label: (uuid: string | undefined, index: number) => string;
  readonly group: (uuid: string | undefined) => string | null;
}

const DEFAULT_ROW_INFO: RowInfo = { idColumn: 'ID', label: (_uuid, i) => `Unit ${i + 1}`, group: () => null };

/** The result as display/export tables (first is the main table). */
export function resultTables(result: AnalysisResult, rows: RowInfo = DEFAULT_ROW_INFO, cutK = 2): TableSheet[] {
  switch (result.kind) {
    case 'diagnostics':
      return [{
        name: 'Cluster diagnostics',
        columns: ['Cluster count', 'Within-cluster sum of squares', 'Mean silhouette'],
        rows: result.data.wss.map((wss, i) => [i + 1, num(wss), i ? num(result.data.silhouette[i - 1]) : null]),
      }];
    case 'fit': {
      const data = result.data;
      const ids = data.analytical_uuids ?? [];
      if (data.cluster) {
        return [{
          name: 'Cluster assignments',
          columns: [rows.idColumn, 'Group', 'Cluster', 'Silhouette'],
          rows: data.cluster.map((label, i) => [rows.label(ids[i], i), rows.group(ids[i]), label, num(data.silhouette?.[i])]),
        }];
      }
      // hclust `merge`: negative entries are samples, positive ones earlier merges.
      const ref = (value: number) => (value < 0 ? rows.label(ids[-value - 1], -value - 1) : `Merge ${value}`);
      const sheets: TableSheet[] = [];
      const layout = layoutDendrogram(data, Math.max(1, Math.min(cutK, data.n_rows)));
      if (layout) {
        const cluster = new Array<number>(data.n_rows);
        for (const leaf of layout.leaves) cluster[leaf.row - 1] = leaf.group + 1;
        sheets.push({
          name: `Clusters (cut ${cutK})`,
          columns: [rows.idColumn, 'Group', 'Cluster'],
          rows: Array.from({ length: data.n_rows }, (_, i) => [rows.label(ids[i], i), rows.group(ids[i]), cluster[i] ?? null]),
        });
      }
      sheets.push({
        name: 'Merge steps',
        columns: ['Merge step', 'Left branch', 'Right branch', 'Height'],
        rows: (data.merge ?? []).map((pair, i) => [i + 1, ref(pair[0]), ref(pair[1]), num(data.height?.[i])]),
      });
      return sheets;
    }
    case 'membership': {
      const data = result.data;
      return [{
        name: data.effective_method === 'mahalanobis' ? 'Mahalanobis distances' : 'Membership probabilities',
        columns: ['ID', 'Group', ...data.eligible_groups, 'Best group', 'Best value', 'In group', 'Projection included'],
        rows: data.ids.map((id, i) => [
          id, data.groups[i] ?? null, ...(data.probabilities[i] ?? []).map(num), data.best_group[i] ?? null, num(data.best_value[i]),
          data.in_group[i] ? 'Yes' : 'No', data.projection_included?.[i] === false ? 'No' : 'Yes',
        ]),
      }];
    }
    case 'euclidean':
      return [{
        name: 'Euclidean matches',
        columns: ['ID', 'Group', 'Match ID', 'Match group', 'Distance'],
        rows: result.data.rows.map((row) => [row.id, row.group, row.match_id, row.match_group, num(row.distance)]),
      }];
  }
}

const EXPORT_NAMES: Record<AnalysisResult['kind'], string> = {
  diagnostics: 'Cluster diagnostics',
  fit: 'Cluster results',
  membership: 'Membership probabilities',
  euclidean: 'Euclidean matches',
};

/** Internal row keys and file paths never enter the visible results table. */
export function ResultTable({ result, rows: rowInfo, cutK = 2 }: { result: AnalysisResult; rows?: RowInfo; cutK?: number }): ReactElement {
  const [shown, setShown] = useState(100);
  const sheets = resultTables(result, rowInfo, cutK);
  let method = '';
  if (result.kind === 'fit') method = result.data.method;
  if (result.kind === 'membership') method = result.data.effective_method === 'mahalanobis' ? 'Mahalanobis distances (lower is closer)' : 'Hotelling membership probabilities (%)';
  const meta = <>
    {method && <p className="muted">Method: {method}</p>}
    {result.kind === 'membership' && result.data.fallback_reason && <p role="status" className="callout callout-info">Hotelling probabilities were unavailable for this comparison. Showing Mahalanobis distances instead ({result.data.fallback_reason.replaceAll('_', ' ')}).</p>}
    {result.data.source && <p className="muted">Source: {result.data.source}; columns: {result.data.column_names?.join(', ')}</p>}
    {(result.kind === 'fit' || result.kind === 'diagnostics') && result.data.metric && <p className="muted">Distance: {result.data.metric}{result.kind === 'fit' && result.data.merge ? (result.data.method === 'diana' ? '; method: DIANA' : `; linkage: ${result.data.linkage}`) : ''}</p>}
    {result.kind === 'diagnostics' && <p className="muted">Diagnostic method: {result.data.diagnostic_method ?? 'kmeans'}</p>}
  </>;
  return <section aria-label="Analysis results" className="result-tables">
    {meta}
    {sheets.map((sheet, index) => <TableCard
      key={sheet.name}
      title={sheet.name}
      fileName={index === 0 ? EXPORT_NAMES[result.kind] : `${EXPORT_NAMES[result.kind]} - ${sheet.name}`}
      sheets={() => (index === 0 ? sheets : [sheet])}
      note={`${sheet.rows.length} rows${sheet.rows.length > shown ? `; showing ${shown}` : ''}.`}
    >
      {sheet.rows.length === 0 ? <p className="empty-note">No eligible results for these settings.</p> : <table className="data-table">
        <thead><tr>{sheet.columns.map((h, i) => <th key={i}>{h}</th>)}</tr></thead>
        <tbody>{sheet.rows.slice(0, shown).map((row, i) => <tr key={i}>{row.map((v, j) => <td key={j}>{display(v)}</td>)}</tr>)}</tbody>
      </table>}
      {shown < sheet.rows.length && <button type="button" className="btn btn-sm" onClick={() => setShown(n => n + 100)}>Show 100 more rows</button>}
    </TableCard>)}
  </section>;
}

export function AnalysisPage({ kind, deps }: { kind: AnalysisKind; deps: AnalysisDeps }): ReactElement {
  const dataset = useGroupFiles(deps.groups);
  const { files, combined, candidates } = dataset;
  const paths = useMemo(() => dataset.ready.map(c => c.path), [dataset.ready]);
  const multiSource = files.length > 1;
  const data = useMemo(
    () => (dataset.load.kind === 'loaded' && files.length > 0 ? asRowsResponse(files, combined) : null),
    [dataset.load.kind, files, combined],
  );
  const [reviewing, setReviewing] = useState(false);
  const [group, setGroup] = useState('');
  const [id, setId] = useState('');
  const [source, setSource] = useState<AnalysisSource>('elements');
  const [pcCount, setPcCount] = useState(2);
  const [plotGroup, setPlotGroup] = useState('');
  const [sourceGroup, setSourceGroup] = useState('');
  const [metric, setMetric] = useState<ClusterDistanceMetric>('euclidean');
  const [linkage, setLinkage] = useState<ClusterLinkage>('ward_d2');
  const [minkowskiP, setMinkowskiP] = useState(2);
  const [starts, setStarts] = useState(25);
  const [iterations, setIterations] = useState(100);
  const [diagnosticMetric, setDiagnosticMetric] = useState<'euclidean' | 'manhattan'>('euclidean');
  const [diagnosticMethod, setDiagnosticMethod] = useState<'kmeans' | 'pam'>('kmeans');
  const [projection, setProjection] = useState<string[] | null>(null);
  const [definitions, setDefinitions] = useState<TransformationDefinition[]>([]);
  const [definitionName, setDefinitionName] = useState('');
  const analysisSetup = useAnalysisSetup();
  // Elements come from the Data Manager's Analysis setup; this page only
  // chooses whether to use its transform, measured values, or a saved
  // definition.
  const setupInput = useMemo(
    () => (data ? analysisInput(analysisSetup.setup, data.elemental_columns) : null),
    [data, analysisSetup.setup],
  );
  const savedDefinitions = definitions.filter(d => d.name !== ANALYSIS_SETUP_NAME);
  const usingSetup = definitionName === ANALYSIS_SETUP_NAME && !!analysisSetup.setup;
  const savedDefinition = savedDefinitions.find(d => d.name === definitionName) ?? null;
  const definition = usingSetup ? (setupInput?.transformation ?? null) : savedDefinition;
  const columns = useMemo(
    () => (!data ? [] : savedDefinition && !usingSetup ? analysisColumns(data, savedDefinition) : (setupInput?.columns ?? data.elemental_columns)),
    [data, savedDefinition, usingSetup, setupInput],
  );
  const [job, setJob] = useState<AnalysisJobSnapshot | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const activeJob = useRef<AbortController | null>(null);
  const [method, setMethod] = useState<ClusterMethod>('kmeans');
  const [membershipMethod, setMembershipMethod] = useState<MembershipMethod>('hotellings');
  const [k, setK] = useState(2);
  const [seed, setSeed] = useState(20260914);
  const [limit, setLimit] = useState(5);
  const [within, setWithin] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [cutK, setCutK] = useState(2);
  const moving = useRef(false);
  const [result, setResult] = useState<AnalysisResult | null>(null);
  const generation = useRef(0);
  useEffect(() => {
    let active = true;
    deps.transformations.list().then(async list => Promise.all(list.transformations.map(d => deps.transformations.load(d.name))))
      .then(found => { if (active) setDefinitions(found); }).catch(e => { if (active) setError(`Could not load transformations: ${String(e)}`); });
    return () => { active = false; generation.current++; activeJob.current?.abort(); };
  }, [deps]);
  // New rows (selection change or a committed move): reset settings. With
  // several groups the synthetic group column is the default grouping.
  const defaultGroup = defaultGroupColumn(combined);
  useEffect(() => {
    if (!data) return;
    setGroup(defaultGroup);
    setId(data.visible_id_column); setSourceGroup(defaultGroup); setPlotGroup(defaultGroup); setProjection(null);
    setDefinitionName(analysisSetup.setup ? ANALYSIS_SETUP_NAME : '');
  }, [data, defaultGroup, analysisSetup.setup]);
  useEffect(() => { if (dataset.load.kind === 'error') setError(dataset.load.message); }, [dataset.load]);
  useEffect(() => {
    if (data && columns.length) setPcCount(current => Math.max(1, Math.min(current, columns.length, data.rows.length)));
  }, [columns.length, data]);

  // ANID and group of each loaded row, by hidden identity.
  const rowInfo = useMemo<RowInfo>(() => {
    const byUuid = new Map(combined.rows.map((r, i) => [r.analytical_uuid, { id: r.visible_id ?? r.legacy_rowid ?? '', group: combined.groupOf[i] ?? null }]));
    return {
      idColumn: combined.visibleIdColumn,
      label: (uuid, i) => (uuid && byUuid.get(uuid)?.id) || `Unit ${i + 1}`,
      group: uuid => (uuid ? byUuid.get(uuid)?.group ?? null : null),
    };
  }, [combined]);

  function reset() { activeJob.current?.abort(); activeJob.current = null; generation.current++; setResult(null); setReviewing(false); setError(''); setNotice(''); setBusy(false); }
  async function run(diagnostics = false) {
    if (activeJob.current || moving.current) return;
    const ticket = ++generation.current;
    setBusy(true); setError(''); setNotice(''); setReviewing(false); setCutK(2); setResult(null);
    try {
      const input = { ...inputPaths(files), columns, transformation: definition, ...sourceOptions(source, pcCount, sourceGroup, seed) };
      let analysis: AnalysisJobRequest;
      if (kind === 'cluster') {
        analysis = diagnostics
          ? { kind: 'cluster_diagnostics', request: { ...input, max_k: k, seed, diagnostic_method: diagnosticMethod, metric: diagnosticMethod === 'kmeans' ? 'euclidean' : diagnosticMetric } }
          : { kind: 'cluster_fit', request: { ...input, method, k, seed, plot_group_column: plotGroup || null, iter_max: iterations, nstart: starts, metric: method === 'kmeans' ? 'euclidean' : metric, linkage, minkowski_p: minkowskiP } };
      } else if (kind === 'membership') {
        analysis = { kind: 'membership_probabilities', request: { ...input, group_column: group, id_column: id, method: membershipMethod, projection_groups: projection } };
      } else {
        analysis = { kind: 'euclidean_matches', request: { ...input, group_column: group, id_column: id, limit, within_group: within, projection_groups: projection } };
      }
      const controller = new AbortController();
      activeJob.current = controller;
      setCancelling(false); setJob(null);
      const output = await runAnalysisJob(deps.jobs, analysis, { signal: controller.signal, onProgress: value => { if (ticket === generation.current) setJob(value); } });
      const next: AnalysisResult = output.kind === 'cluster_fit' ? { kind: 'fit', data: output.result }
        : output.kind === 'cluster_diagnostics' ? { kind: 'diagnostics', data: output.result }
        : output.kind === 'membership_probabilities' ? { kind: 'membership', data: output.result }
        : { kind: 'euclidean', data: output.result };
      if (ticket === generation.current) setResult(next);
    } catch (e) { if (ticket === generation.current) { if (e instanceof DOMException && e.name === 'AbortError') setNotice('Analysis cancelled.'); else setError(String(e)); } }
    finally { if (ticket === generation.current) { setBusy(false); setCancelling(false); activeJob.current = null; } }
  }
  async function assign(request: TransferUnitsRequest | BatchTransferUnitsRequest) {
    if (moving.current) return;
    moving.current = true;
    const ticket = generation.current;
    setBusy(true); setError(''); setNotice('');
    try {
      const revisions = new Map<string, string>();
      for (const c of candidates) if (c.group?.revision_id) revisions.set(c.path, c.group.revision_id);
      for (const f of files) revisions.set(f.path, f.data.revision_id);
      const outcome = await moveAcrossSources(deps.groups, request, {
        sourceOf: new Map(combined.rows.map((r, i) => [r.analytical_uuid, combined.pathOf[i]!])),
        sources: files.map(f => f.path),
        revisions,
      });
      if (ticket !== generation.current) return;
      setResult(null); setReviewing(false);
      setNotice(`Moved ${outcome.moved} analytical units. Recompute analysis for the updated data.`);
      try {
        // Keep the selection, plus any group a move created.
        const created = outcome.transactions.flatMap(tx => tx.outputs.map(o => o.path));
        const selected = dataset.selectedPaths;
        await dataset.rescan(all => all.filter(p => selected.includes(p) || created.includes(p)));
        dataset.reload();
        notifyGroupsChanged();
      } catch (e) {
        setError(`Move committed, but reloading failed. Reopen the dataset. ${e instanceof Error ? e.message : String(e)}`);
      }
    } catch (e) {
      if (ticket === generation.current) setError(`Move failed; the selection was not retried. Recompute analysis if the data changed. ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      moving.current = false;
      if (ticket === generation.current) setBusy(false);
    }
  }
  const disabled = busy || !data || !columns.length || (kind !== 'cluster' && (!group || !id)) || (source === 'lda' && !sourceGroup);
  const submit = (event: FormEvent) => { event.preventDefault(); void run(); };
  const elementsNote = !data ? '' : savedDefinition && !usingSetup
    ? `Columns from “${savedDefinition.name}”: ${columns.length}.`
    : `${setupSummary(analysisSetup.setup, data.elemental_columns)}${definitionName === '' && analysisSetup.setup ? ' (measured values, no transform)' : ''}.`;
  return <section className="page" aria-label={titles[kind]}>
    <PageHeader title={titles[kind]} description={descriptions[kind]}>
      <label className="field field-inline">Dataset <GroupMultiSelect disabled={busy} options={dataset.options} selected={dataset.selectedPaths} onChange={next => { reset(); dataset.setSelectedPaths(next); }} /></label>
    </PageHeader>
    {dataset.load.kind === 'loading' && <BusyNote>Loading dataset…</BusyNote>}
    {dataset.orderedPaths.length === 0 && dataset.load.kind !== 'loading' && <p className="empty-note">No groups are included. Tick groups in the Data Manager, or choose them in Dataset.</p>}
    <form className="card settings-card" onSubmit={submit} aria-label="Analysis settings form">
      <header className="card-header"><h2 className="card-title">Analysis settings</h2></header>
      {data ? <fieldset className="form-grid" disabled={busy} onChange={reset}><legend className="visually-hidden">Analysis settings</legend>
        <label className="field">Values <select value={definitionName} onChange={e => setDefinitionName(e.target.value)}>
          {analysisSetup.setup && <option value={ANALYSIS_SETUP_NAME}>Analysis setup (Data Manager)</option>}
          <option value="">Measured values</option>{savedDefinitions.map(d => <option key={d.name}>{d.name}</option>)}
        </select></label>
        <label className="field">Analysis source <select value={source} onChange={e => setSource(e.target.value as AnalysisSource)}>
          <option value="elements">Elements / ratios</option><option value="pca">PCA scores</option><option value="umap">UMAP dimensions</option><option value="lda">Linear discriminants</option>
        </select></label>
        {source === 'pca' && <label className="field">Principal components <input type="number" min={1} max={Math.min(columns.length, data.rows.length)} value={pcCount} onChange={e => setPcCount(Number(e.target.value))} /></label>}
        {source === 'lda' && <label className="field">LDA grouping column <select value={sourceGroup} onChange={e => setSourceGroup(e.target.value)}>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>}
        {source === 'umap' && <label className="field">UMAP seed <input type="number" min={0} max={2147483627} value={seed} onChange={e => setSeed(Number(e.target.value))} /></label>}
        {kind === 'cluster' ? <>
          <label className="field">Method <select value={method} onChange={e => { setMethod(e.target.value as ClusterMethod); setMetric('euclidean'); }}>
            <option value="kmeans">k-means</option><option value="pam">k-medoids (PAM)</option>
            <option value="hclust">Hierarchical</option><option value="diana">Divisive (DIANA)</option>
          </select></label>
          {method !== 'kmeans' && <label className="field">Distance <select value={metric} onChange={e => setMetric(e.target.value as ClusterDistanceMetric)}>
            <option value="euclidean">Euclidean</option><option value="manhattan">Manhattan</option>
            {method === 'hclust' && <><option value="minkowski">Minkowski</option><option value="maximum">Maximum</option></>}
          </select></label>}
          {method === 'hclust' && <label className="field">Linkage <select value={linkage} onChange={e => setLinkage(e.target.value as ClusterLinkage)}><option value="average">Average</option><option value="complete">Complete</option><option value="ward_d">Ward.D</option><option value="ward_d2">Ward.D2</option></select></label>}
          {method === 'hclust' && metric === 'minkowski' && <label className="field">Minkowski power <input type="number" min={1} step={0.1} value={minkowskiP} onChange={e => setMinkowskiP(Number(e.target.value))} /></label>}
          <label className="field">Cluster count / diagnostic maximum <input type="number" min={1} max={20} value={k} onChange={e => setK(Number(e.target.value))} /></label>
          {method === 'kmeans' && <><label className="field">Starts <input type="number" min={1} max={100} value={starts} onChange={e => setStarts(Number(e.target.value))} /></label><label className="field">Maximum iterations <input type="number" min={1} max={200} value={iterations} onChange={e => setIterations(Number(e.target.value))} /></label></>}
          {(method === 'kmeans' || method === 'pam') && <label className="field">Plot grouping column <select value={plotGroup} onChange={e => setPlotGroup(e.target.value)}><option value="">No group colors</option>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>}
          <label className="field">Diagnostic method <select value={diagnosticMethod} onChange={e => setDiagnosticMethod(e.target.value as 'kmeans' | 'pam')}><option value="kmeans">k-means</option><option value="pam">k-medoids (PAM)</option></select></label>
          {diagnosticMethod === 'pam' && <label className="field">Diagnostic distance <select value={diagnosticMetric} onChange={e => setDiagnosticMetric(e.target.value as 'euclidean' | 'manhattan')}><option value="euclidean">Euclidean</option><option value="manhattan">Manhattan</option></select></label>}
          <label className="field">Random seed <input type="number" min={-2147483648} max={2147483627} value={seed} onChange={e => setSeed(Number(e.target.value))} /></label>
        </> : <>
          <label className="field">Group column <select value={group} onChange={e => { setGroup(e.target.value); setProjection(null); }}>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>
          <label className="field">ID column <select value={id} onChange={e => setId(e.target.value)}>{[...new Set([data.visible_id_column, ...data.descriptive_columns])].map(c => <option key={c}>{c}</option>)}</select></label>
          {kind === 'membership' ? <label className="field">Method <select value={membershipMethod} onChange={e => setMembershipMethod(e.target.value as MembershipMethod)}><option value="hotellings">Hotelling probabilities (Mahalanobis fallback)</option><option value="mahalanobis">Mahalanobis distances</option></select></label> : <>
            <label className="field">Matches per analytical unit <input type="number" min={1} max={100} value={limit} onChange={e => setLimit(Number(e.target.value))} /></label>
            <label className="field-check"><input type="checkbox" checked={within} onChange={e => setWithin(e.target.checked)} />Include same-group matches</label>
          </>}
          <label className="field field-wide">Projection groups <GroupMultiSelect label="Projection groups" options={projectionLabels(data, group).map(g => ({ path: g, name: g }))} selected={projection ?? projectionLabels(data, group)} onChange={next => { reset(); setProjection(next); }} /></label>
          {kind === 'euclidean' && <p className="field-hint field-wide">Same-group matches are filtered after the match limit is applied.</p>}
        </>}
      </fieldset> : <p className="empty-note">Load a dataset to choose settings.</p>}
      {data && <p className="setup-in-use">Elements: {elementsNote} Choose elements in the Data Manager’s Analysis setup.</p>}
      <div className="form-actions">
        <button type="submit" className="btn btn-primary" disabled={disabled}><BusyLabel busy={busy && !moving.current && !!activeJob.current} busyText="Running…">Run analysis</BusyLabel></button>
        {kind === 'cluster' && <button type="button" className="btn" disabled={disabled} onClick={() => void run(true)}>Run cluster diagnostics</button>}
        {busy && !moving.current && <button type="button" className="btn btn-ghost" disabled={cancelling} onClick={() => { setCancelling(true); activeJob.current?.abort(); }}>Cancel analysis</button>}
      </div>
    </form>
    {busy && <BusyNote>{moving.current ? 'Assigning analytical units to groups…' : cancelling ? 'Cancelling analysis…' : job ? `${job.state}: ${job.stage?.replaceAll('_', ' ') ?? 'waiting'} (${job.progress}%)` : 'Submitting analysis…'}</BusyNote>}
    {notice && <p role="status" className="callout callout-info">{notice}</p>}{error && <p role="alert">{error}</p>}
    {result && <div key={generation.current} className="results">
      <AnalysisPlots result={result} cutK={cutK} onCutKChange={setCutK} disabled={busy || reviewing} rowLabel={rowInfo.label} />
      <ResultTable result={result} rows={rowInfo} cutK={cutK} />
      <ResultAssignment result={result} cutK={cutK} destinations={paths} candidates={candidates} multiSource={multiSource} busy={busy} onReviewChange={setReviewing} onConfirm={request => void assign(request)} onBatchConfirm={request => void assign(request)} rowLabel={rowInfo.label} />
    </div>}
  </section>;
}
