import { runAnalysisJob } from './job-workflow.ts';
import { analysisColumns, projectionLabels, sourceOptions } from './input-options.ts';
import { moveAcrossSources } from './multi-source-move.ts';
import { GroupMultiSelect } from '../datasets/GroupMultiSelect.tsx';
import { useGroupFiles } from '../datasets/useGroupFiles.ts';
import { asRowsResponse, defaultGroupColumn, inputPaths } from '../visualize/combine.ts';
import { notifyGroupsChanged } from '../data-manager/events.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { ANALYSIS_SETUP_NAME, analysisInput } from '../analysis-setup/setup.ts';
import { ResultAssignment } from './ResultAssignment.tsx';
import { AnalysisPlots } from './AnalysisPlots.tsx';
import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import type {
  AnalysisSource, ClusterDistanceMetric, ClusterLinkage, TransformationDefinition, AnalysisJobSnapshot, AnalysisJobRequest,
  Transport, ClusterMethod, MembershipMethod, TransferUnitsRequest, BatchTransferUnitsRequest,
  ClusterFitResponse, ClusterDiagnosticsResponse,
  MembershipProbabilitiesResponse, EuclideanMatchesResponse,
} from '@archaeodash/client';
import { BusyNote } from '../ui/Busy.tsx';

export type AnalysisKind = 'cluster' | 'membership' | 'euclidean';
export type AnalysisDeps = Pick<Transport, 'groups' | 'clustering' | 'jobs' | 'transformations'>;
export type AnalysisResult =
  | { kind: 'fit'; data: ClusterFitResponse }
  | { kind: 'diagnostics'; data: ClusterDiagnosticsResponse }
  | { kind: 'membership'; data: MembershipProbabilitiesResponse }
  | { kind: 'euclidean'; data: EuclideanMatchesResponse };
const titles = { cluster: 'Cluster', membership: 'Probabilities and Distances', euclidean: 'Euclidean Distance' };
const cell = (value: string | number | null | undefined): string => value == null ? 'Unavailable' : String(value);

/** Internal row keys and file paths never enter the visible results table. */
export function ResultTable({ result }: { result: AnalysisResult }): ReactElement {
  const [shown, setShown] = useState(100);
  let headers: string[];
  let rows: (string | number | null | undefined)[][];
  let method = '';
  switch (result.kind) {
    case 'diagnostics':
      headers = ['Cluster count', 'Within-cluster sum of squares', 'Mean silhouette'];
      rows = result.data.wss.map((wss, i) => [i + 1, wss, i ? result.data.silhouette[i - 1] : null]);
      break;
    case 'fit': {
      const data = result.data;
      method = data.method;
      if (data.cluster) {
        headers = ['Analytical unit', 'Cluster', 'Silhouette'];
        rows = data.cluster.map((label, i) => [i + 1, label, data.silhouette?.[i]]);
      } else {
        headers = ['Merge step', 'Left branch', 'Right branch', 'Height'];
        rows = (data.merge ?? []).map((pair, i) => [i + 1, pair[0], pair[1], data.height?.[i]]);
      }
      break;
    }
    case 'membership': {
      const data = result.data;
      const distance = data.effective_method === 'mahalanobis';
      method = distance ? 'Mahalanobis distances (lower is closer)' : 'Hotelling membership probabilities (%)';
      headers = ['ID', 'Group', ...data.eligible_groups, 'Best group', 'Best value', 'In group', 'Projection included'];
      rows = data.ids.map((id, i) => [id, data.groups[i], ...(data.probabilities[i] ?? []), data.best_group[i], data.best_value[i], data.in_group[i] ? 'Yes' : 'No', data.projection_included?.[i] === false ? 'No' : 'Yes']);
      break;
    }
    case 'euclidean':
      headers = ['ID', 'Group', 'Match ID', 'Match group', 'Distance'];
      rows = result.data.rows.map(row => [row.id, row.group, row.match_id, row.match_group, row.distance]);
  }
  return <section aria-label="Analysis results">
    {method && <p>Method: {method}</p>}
    {result.kind === 'membership' && result.data.fallback_reason && <p role="status">Hotelling probabilities were unavailable for this comparison. Showing Mahalanobis distances instead ({result.data.fallback_reason.replaceAll('_', ' ')}).</p>}
    {result.data.source && <p>Source: {result.data.source}; columns: {result.data.column_names?.join(', ')}</p>}
    {(result.kind === 'fit' || result.kind === 'diagnostics') && result.data.metric && <p>Distance: {result.data.metric}{result.kind === 'fit' && result.data.merge ? (result.data.method === 'diana' ? '; method: DIANA' : `; linkage: ${result.data.linkage}`) : ''}</p>}
    {result.kind === 'diagnostics' && <p>Diagnostic method: {result.data.diagnostic_method ?? 'kmeans'}</p>}
    <p>{rows.length} result rows. Showing {Math.min(shown, rows.length)}.</p>
    {rows.length === 0 ? <p>No eligible results for these settings.</p> : <table className="data-table">
      <thead><tr>{headers.map((h, i) => <th key={i}>{h}</th>)}</tr></thead>
      <tbody>{rows.slice(0, shown).map((row, i) => <tr key={i}>{row.map((v, j) => <td key={j}>{cell(v)}</td>)}</tr>)}</tbody>
    </table>}
    {shown < rows.length && <button onClick={() => setShown(n => n + 100)}>Show 100 more rows</button>}
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
  const [columns, setColumns] = useState<string[]>([]);
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
  // The Data Manager's Analysis setup is offered first and used by default;
  // other saved definitions stay selectable.
  const setupInput = useMemo(
    () => (data ? analysisInput(analysisSetup.setup, data.elemental_columns) : null),
    [data, analysisSetup.setup],
  );
  const savedDefinitions = definitions.filter(d => d.name !== ANALYSIS_SETUP_NAME);
  const usingSetup = definitionName === ANALYSIS_SETUP_NAME && !!analysisSetup.setup;
  const definition = usingSetup ? (setupInput?.transformation ?? null) : savedDefinitions.find(d => d.name === definitionName) ?? null;
  const columnChoices = !data ? [] : usingSetup ? (setupInput?.columns ?? []) : analysisColumns(data, definition);
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
    const followSetup = !!analysisSetup.setup && !!setupInput;
    setColumns(followSetup ? setupInput.columns : data.elemental_columns); setGroup(defaultGroup);
    setId(data.visible_id_column); setSourceGroup(defaultGroup); setPlotGroup(defaultGroup); setProjection(null); setDefinitionName(followSetup ? ANALYSIS_SETUP_NAME : '');
  }, [data, defaultGroup, analysisSetup.setup, setupInput]);
  useEffect(() => { if (dataset.load.kind === 'error') setError(dataset.load.message); }, [dataset.load]);
  useEffect(() => {
    if (data && columns.length) setPcCount(current => Math.max(1, Math.min(current, columns.length, data.rows.length)));
  }, [columns.length, data]);
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
  return <section aria-label={titles[kind]}>
    <h1>{titles[kind]}</h1>
    <p>Analyze measured or transformed columns and recomputed ordination scores. Results are temporary.</p>
    <label>Dataset <GroupMultiSelect disabled={busy} options={dataset.options} selected={dataset.selectedPaths} onChange={next => { reset(); dataset.setSelectedPaths(next); }} /></label>
    {dataset.load.kind === 'loading' && <BusyNote>Loading dataset…</BusyNote>}
    {dataset.orderedPaths.length === 0 && dataset.load.kind !== 'loading' && <p>No groups are included. Tick groups in the Data Manager, or choose them in Dataset.</p>}
    {data && <fieldset disabled={busy} onChange={reset}><legend>Analysis settings</legend>
      <label>Transformation <select value={definitionName} onChange={e => {
        const name = e.target.value;
        setDefinitionName(name);
        if (name === ANALYSIS_SETUP_NAME) setColumns(setupInput?.columns ?? []);
        else setColumns(analysisColumns(data, savedDefinitions.find(d => d.name === name) ?? null));
      }}>
        {analysisSetup.setup && <option value={ANALYSIS_SETUP_NAME}>Analysis setup (Data Manager)</option>}
        <option value="">Measured values</option>{savedDefinitions.map(d => <option key={d.name}>{d.name}</option>)}
      </select></label>
      <label>Analysis source <select value={source} onChange={e => setSource(e.target.value as AnalysisSource)}>
        <option value="elements">Elements / ratios</option><option value="pca">PCA scores</option><option value="umap">UMAP dimensions</option><option value="lda">Linear discriminants</option>
      </select></label>
      {source === 'pca' && <label>Principal components <input type="number" min={1} max={Math.min(columns.length, data.rows.length)} value={pcCount} onChange={e => setPcCount(Number(e.target.value))} /></label>}
      {source === 'lda' && <label>LDA grouping column <select value={sourceGroup} onChange={e => setSourceGroup(e.target.value)}>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>}
      {source === 'umap' && <label>UMAP seed <input type="number" min={0} max={2147483627} value={seed} onChange={e => setSeed(Number(e.target.value))} /></label>}
      <label>Input columns <select multiple value={columns} onChange={e => setColumns(Array.from(e.target.selectedOptions, o => o.value))}>
        {columnChoices.map(c => <option key={c}>{c}</option>)}
      </select></label>
      {kind === 'cluster' ? <>
        <label>Method <select value={method} onChange={e => { setMethod(e.target.value as ClusterMethod); setMetric('euclidean'); }}>
          <option value="kmeans">k-means</option><option value="pam">k-medoids (PAM)</option>
          <option value="hclust">Hierarchical</option><option value="diana">Divisive (DIANA)</option>
        </select></label>
        {method !== 'kmeans' && <label>Distance <select value={metric} onChange={e => setMetric(e.target.value as ClusterDistanceMetric)}>
          <option value="euclidean">Euclidean</option><option value="manhattan">Manhattan</option>
          {method === 'hclust' && <><option value="minkowski">Minkowski</option><option value="maximum">Maximum</option></>}
        </select></label>}
        {method === 'hclust' && <label>Linkage <select value={linkage} onChange={e => setLinkage(e.target.value as ClusterLinkage)}><option value="average">Average</option><option value="complete">Complete</option><option value="ward_d">Ward.D</option><option value="ward_d2">Ward.D2</option></select></label>}
        {method === 'hclust' && metric === 'minkowski' && <label>Minkowski power <input type="number" min={1} step={0.1} value={minkowskiP} onChange={e => setMinkowskiP(Number(e.target.value))} /></label>}
        {method === 'kmeans' && <><label>Starts <input type="number" min={1} max={100} value={starts} onChange={e => setStarts(Number(e.target.value))} /></label><label>Maximum iterations <input type="number" min={1} max={200} value={iterations} onChange={e => setIterations(Number(e.target.value))} /></label></>}
        {(method === 'kmeans' || method === 'pam') && <label>Plot grouping column <select value={plotGroup} onChange={e => setPlotGroup(e.target.value)}><option value="">No group colors</option>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>}
        <label>Diagnostic method <select value={diagnosticMethod} onChange={e => setDiagnosticMethod(e.target.value as 'kmeans' | 'pam')}><option value="kmeans">k-means</option><option value="pam">k-medoids (PAM)</option></select></label>
        {diagnosticMethod === 'pam' && <label>Diagnostic distance <select value={diagnosticMetric} onChange={e => setDiagnosticMetric(e.target.value as 'euclidean' | 'manhattan')}><option value="euclidean">Euclidean</option><option value="manhattan">Manhattan</option></select></label>}
        <label>Cluster count / diagnostic maximum <input type="number" min={1} max={20} value={k} onChange={e => setK(Number(e.target.value))} /></label>
        <label>Random seed <input type="number" min={-2147483648} max={2147483627} value={seed} onChange={e => setSeed(Number(e.target.value))} /></label>
      </> : <>
        <label>Group column <select value={group} onChange={e => { setGroup(e.target.value); setProjection(null); }}>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>
        <label>Projection groups <select multiple value={projection ?? projectionLabels(data, group)} onChange={e => setProjection(Array.from(e.target.selectedOptions, o => o.value))}>{projectionLabels(data, group).map(g => <option key={g}>{g}</option>)}</select></label>
        <label>ID column <select value={id} onChange={e => setId(e.target.value)}>{[...new Set([data.visible_id_column, ...data.descriptive_columns])].map(c => <option key={c}>{c}</option>)}</select></label>
        {kind === 'membership' ? <label>Method <select value={membershipMethod} onChange={e => setMembershipMethod(e.target.value as MembershipMethod)}><option value="hotellings">Hotelling probabilities (Mahalanobis fallback)</option><option value="mahalanobis">Mahalanobis distances</option></select></label> : <>
          <label>Matches per analytical unit <input type="number" min={1} max={100} value={limit} onChange={e => setLimit(Number(e.target.value))} /></label>
          <label><input type="checkbox" checked={within} onChange={e => setWithin(e.target.checked)} />Include same-group matches</label>
          <p>Same-group matches are filtered after the match limit is applied.</p>
        </>}
      </>}
    </fieldset>}
    <button disabled={disabled} onClick={() => void run()}>Run analysis</button>
    {kind === 'cluster' && <button disabled={disabled} onClick={() => void run(true)}>Run cluster diagnostics</button>}
    {busy && <BusyNote>{moving.current ? 'Assigning analytical units to groups…' : cancelling ? 'Cancelling analysis…' : job ? `${job.state}: ${job.stage?.replaceAll('_', ' ') ?? 'waiting'} (${job.progress}%)` : 'Submitting analysis…'}</BusyNote>}
    {busy && !moving.current && <button disabled={cancelling} onClick={() => { setCancelling(true); activeJob.current?.abort(); }}>Cancel analysis</button>}{notice && <p role="status">{notice}</p>}{error && <p role="alert">{error}</p>}
    {result && <div key={generation.current}><AnalysisPlots result={result} cutK={cutK} onCutKChange={setCutK} disabled={busy || reviewing} /><ResultAssignment result={result} cutK={cutK} destinations={paths} candidates={candidates} multiSource={multiSource} busy={busy} onReviewChange={setReviewing} onConfirm={request => void assign(request)} onBatchConfirm={request => void assign(request)} /><ResultTable result={result} /></div>}
  </section>;
}
