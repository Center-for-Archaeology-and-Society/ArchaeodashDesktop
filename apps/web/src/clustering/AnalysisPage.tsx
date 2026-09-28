import { AnalysisPlots } from './AnalysisPlots.tsx';
import { useEffect, useRef, useState, type ReactElement } from 'react';
import type {
  Transport, GroupRowsResponse, ClusterMethod, MembershipMethod,
  ClusterFitResponse, ClusterDiagnosticsResponse,
  MembershipProbabilitiesResponse, EuclideanMatchesResponse,
} from '@archaeodash/client';

export type AnalysisKind = 'cluster' | 'membership' | 'euclidean';
export type AnalysisDeps = Pick<Transport, 'groups' | 'clustering'>;
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
      headers = ['ID', 'Group', ...data.eligible_groups, 'Best group', 'Best value'];
      rows = data.ids.map((id, i) => [id, data.groups[i], ...(data.probabilities[i] ?? []), data.best_group[i], data.best_value[i]]);
      break;
    }
    case 'euclidean':
      headers = ['ID', 'Group', 'Match ID', 'Match group', 'Distance'];
      rows = result.data.rows.map(row => [row.id, row.group, row.match_id, row.match_group, row.distance]);
  }
  return <section aria-label="Analysis results">
    {method && <p>Method: {method}</p>}
    <p>{rows.length} result rows. Showing {Math.min(shown, rows.length)}.</p>
    {rows.length === 0 ? <p>No eligible results for these settings.</p> : <table className="data-table">
      <thead><tr>{headers.map((h, i) => <th key={i}>{h}</th>)}</tr></thead>
      <tbody>{rows.slice(0, shown).map((row, i) => <tr key={i}>{row.map((v, j) => <td key={j}>{cell(v)}</td>)}</tr>)}</tbody>
    </table>}
    {shown < rows.length && <button onClick={() => setShown(n => n + 100)}>Show 100 more rows</button>}
  </section>;
}

export function AnalysisPage({ kind, deps }: { kind: AnalysisKind; deps: AnalysisDeps }): ReactElement {
  const [paths, setPaths] = useState<string[]>([]);
  const [path, setPath] = useState('');
  const [data, setData] = useState<GroupRowsResponse | null>(null);
  const [columns, setColumns] = useState<string[]>([]);
  const [group, setGroup] = useState('');
  const [id, setId] = useState('');
  const [method, setMethod] = useState<ClusterMethod>('kmeans');
  const [membershipMethod, setMembershipMethod] = useState<MembershipMethod>('hotellings');
  const [k, setK] = useState(2);
  const [seed, setSeed] = useState(20260914);
  const [limit, setLimit] = useState(5);
  const [within, setWithin] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [result, setResult] = useState<AnalysisResult | null>(null);
  const generation = useRef(0);
  useEffect(() => {
    let active = true;
    deps.groups.scan().then(found => { if (active) setPaths(found.map(p => p.path)); })
      .catch(e => { if (active) setError(String(e)); });
    return () => { active = false; generation.current++; };
  }, [deps]);
  useEffect(() => {
    let active = true;
    setData(null);
    if (path) deps.groups.rows(path).then(rows => {
      if (!active) return;
      setData(rows); setColumns(rows.elemental_columns); setGroup(rows.descriptive_columns[0] ?? '');
      setId(rows.visible_id_column);
    }).catch(e => { if (active) setError(String(e)); });
    return () => { active = false; };
  }, [deps, path]);
  function reset() { generation.current++; setResult(null); setError(''); setBusy(false); }
  async function run(diagnostics = false) {
    const ticket = ++generation.current;
    setBusy(true); setError(''); setResult(null);
    try {
      let next: AnalysisResult;
      if (kind === 'cluster') {
        next = diagnostics
          ? { kind: 'diagnostics', data: await deps.clustering.diagnostics({ path, columns, max_k: k, seed }) }
          : { kind: 'fit', data: await deps.clustering.fit({ path, columns, method, k, seed, iter_max: 100, nstart: 25 }) };
      } else if (kind === 'membership') {
        next = { kind: 'membership', data: await deps.clustering.membershipProbabilities({ path, columns, group_column: group, id_column: id, method: membershipMethod }) };
      } else {
        next = { kind: 'euclidean', data: await deps.clustering.euclideanMatches({ path, columns, group_column: group, id_column: id, limit, within_group: within }) };
      }
      if (ticket === generation.current) setResult(next);
    } catch (e) { if (ticket === generation.current) setError(String(e)); }
    finally { if (ticket === generation.current) setBusy(false); }
  }
  const disabled = busy || !data || !columns.length || (kind !== 'cluster' && (!group || !id));
  return <section aria-label={titles[kind]}>
    <h1>{titles[kind]}</h1>
    <p>Analyze measured elemental columns from one group file. Results are temporary.</p>
    <label>Dataset <select value={path} onChange={e => { reset(); setData(null); setPath(e.target.value); }}>
      <option value="">Choose a group file…</option>{paths.map(p => <option key={p}>{p}</option>)}
    </select></label>
    {data && <fieldset disabled={busy} onChange={reset}><legend>Analysis settings</legend>
      <label>Elemental columns <select multiple value={columns} onChange={e => setColumns(Array.from(e.target.selectedOptions, o => o.value))}>
        {data.elemental_columns.map(c => <option key={c}>{c}</option>)}
      </select></label>
      {kind === 'cluster' ? <>
        <label>Method <select value={method} onChange={e => setMethod(e.target.value as ClusterMethod)}>
          <option value="kmeans">k-means</option><option value="pam">k-medoids (PAM)</option>
          <option value="hclust_ward_d2">Hierarchical (Ward.D2)</option><option value="diana">Divisive (DIANA)</option>
        </select></label>
        <label>Cluster count / diagnostic maximum <input type="number" min={2} max={20} value={k} onChange={e => setK(Number(e.target.value))} /></label>
        <label>Random seed <input type="number" min={-2147483648} max={2147483627} value={seed} onChange={e => setSeed(Number(e.target.value))} /></label>
      </> : <>
        <label>Group column <select value={group} onChange={e => setGroup(e.target.value)}>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>
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
    {busy && <p role="status">Computing…</p>}{error && <p role="alert">{error}</p>}
    {result && <div key={generation.current}><AnalysisPlots result={result} /><ResultTable result={result} /></div>}
  </section>;
}
