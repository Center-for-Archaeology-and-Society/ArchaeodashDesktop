import { runAnalysisJob } from './job-workflow.ts';
import {
  analysisColumns, defaultIdColumn, groupEligibility, pcCountOptions, projectionLabels, sourceFeatureCount, sourceOptions, uniqueIdColumns,
} from './input-options.ts';
import { moveAcrossSources } from './multi-source-move.ts';
import { GroupMultiSelect } from '../datasets/GroupMultiSelect.tsx';
import { useGroupFiles } from '../datasets/useGroupFiles.ts';
import { rowContentKey } from '../datasets/rowKeys.ts';
import { asRowsResponse, defaultGroupColumn, inputPaths } from '../visualize/combine.ts';
import { notifyGroupsChanged } from '../data-manager/events.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { ANALYSIS_SETUP_NAME, UMAP_SEED, analysisInput, setupSummary } from '../analysis-setup/setup.ts';
import { ResultAssignment } from './ResultAssignment.tsx';
import { AnalysisPlots } from './AnalysisPlots.tsx';
import { layoutDendrogram } from './dendrogram-model.ts';
import { euclideanMatchKeys, selectableRows } from './assignment-model.ts';
import { ResultGrid, type GridSelection } from './ResultGrid.tsx';
import type { GridColumn } from './grid-model.ts';
import { EligibleGroupPicker, GroupSizesTable } from './MembershipGroups.tsx';
import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactElement } from 'react';
import type {
  AnalysisSource, ClusterDistanceMetric, ClusterLinkage, TransformationDefinition, AnalysisJobSnapshot, AnalysisJobRequest,
  Transport, ClusterMethod, MembershipMethod, TransferUnitsRequest, BatchTransferUnitsRequest, GroupCandidate,
  ClusterFitResponse, ClusterDiagnosticsResponse,
  MembershipProbabilitiesResponse, EuclideanMatchesResponse, TableCell,
} from '@archaeodash/client';
import { BusyLabel, BusyNote } from '../ui/Busy.tsx';
import { TableCard, num, type TableSheet } from '../ui/TableExport.tsx';
import { PageHeader } from '../ui/PageHeader.tsx';

export type AnalysisKind = 'cluster' | 'membership' | 'euclidean';
/** `ordination` labels the PC-count choices with explained variance (optional). */
export type AnalysisDeps = Pick<Transport, 'groups' | 'clustering' | 'jobs' | 'transformations'> & Partial<Pick<Transport, 'ordination'>>;
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

/** A result table plus what the on-screen grid needs (exports ignore the extras). */
export interface ResultSheet extends TableSheet {
  /** Names the saved grid view (column visibility, compact mode). */
  readonly id: string;
  readonly gridColumns?: readonly GridColumn[];
  /** Selection key per row (null: not selectable); first sheet only. */
  readonly keys?: readonly (string | null)[];
  readonly rowLabel?: (row: readonly TableCell[], index: number) => string;
}

/** The result as display/export tables (first is the main table). */
export function resultTables(result: AnalysisResult, rows: RowInfo = DEFAULT_ROW_INFO, cutK = 2): ResultSheet[] {
  const valid = new Set(selectableRows(result, cutK).map(row => row.key));
  const keyed = (key: string | null | undefined) => (key && valid.has(key) ? key : null);
  switch (result.kind) {
    case 'diagnostics':
      return [{
        id: 'cluster-diagnostics',
        name: 'Cluster diagnostics',
        columns: ['Cluster count', 'Within-cluster sum of squares', 'Mean silhouette'],
        rows: result.data.wss.map((wss, i) => [i + 1, num(wss), i ? num(result.data.silhouette[i - 1]) : null]),
      }];
    case 'fit': {
      const data = result.data;
      const ids = data.analytical_uuids ?? [];
      if (data.cluster) {
        return [{
          id: 'cluster-assignments',
          name: 'Cluster assignments',
          columns: [rows.idColumn, 'Group', 'Cluster', 'Silhouette'],
          rows: data.cluster.map((label, i) => [rows.label(ids[i], i), rows.group(ids[i]), label, num(data.silhouette?.[i])]),
          keys: data.cluster.map((_, i) => keyed(ids[i])),
        }];
      }
      // hclust `merge`: negative entries are samples, positive ones earlier merges.
      const ref = (value: number) => (value < 0 ? rows.label(ids[-value - 1], -value - 1) : `Merge ${value}`);
      const sheets: ResultSheet[] = [];
      const layout = layoutDendrogram(data, Math.max(1, Math.min(cutK, data.n_rows)));
      if (layout) {
        const cluster = new Array<number>(data.n_rows);
        for (const leaf of layout.leaves) cluster[leaf.row - 1] = leaf.group + 1;
        sheets.push({
          id: 'cluster-cut',
          name: `Clusters (cut ${cutK})`,
          columns: [rows.idColumn, 'Group', 'Cluster'],
          rows: Array.from({ length: data.n_rows }, (_, i) => [rows.label(ids[i], i), rows.group(ids[i]), cluster[i] ?? null]),
          keys: Array.from({ length: data.n_rows }, (_, i) => keyed(ids[i])),
        });
      }
      sheets.push({
        id: 'cluster-merges',
        name: 'Merge steps',
        columns: ['Merge step', 'Left branch', 'Right branch', 'Height'],
        rows: (data.merge ?? []).map((pair, i) => [i + 1, ref(pair[0]), ref(pair[1]), num(data.height?.[i])]),
      });
      return sheets;
    }
    case 'membership': {
      const data = result.data;
      const columns = ['ID', 'Group', ...data.eligible_groups, 'Best group', 'Best value', 'In group', 'Projection included'];
      return [{
        id: 'membership',
        name: data.effective_method === 'mahalanobis' ? 'Mahalanobis distances' : 'Membership probabilities',
        columns,
        // As in the legacy table: values to two decimals, Best value hidden until shown.
        gridColumns: columns.map((label, i) => (i >= 2 && i < 2 + data.eligible_groups.length ? { label, digits: 2 }
          : label === 'Best value' ? { label, digits: 2, hiddenByDefault: true } : { label })),
        keys: data.ids.map((_, i) => keyed(data.analytical_uuids?.[i])),
        rows: data.ids.map((id, i) => [
          id, data.groups[i] ?? null, ...(data.probabilities[i] ?? []).map(num), data.best_group[i] ?? null, num(data.best_value[i]),
          data.in_group[i] ? 'Yes' : 'No', data.projection_included?.[i] === false ? 'No' : 'Yes',
        ]),
      }];
    }
    case 'euclidean': {
      const matchKeys = euclideanMatchKeys(result.data.rows);
      return [{
        id: 'euclidean',
        name: 'Euclidean matches',
        columns: ['ID', 'Group', 'Match ID', 'Match group', 'Distance'],
        gridColumns: [{ label: 'ID' }, { label: 'Group' }, { label: 'Match ID' }, { label: 'Match group' }, { label: 'Distance', digits: 4 }],
        rows: result.data.rows.map((row) => [row.id, row.group, row.match_id, row.match_group, num(row.distance)]),
        keys: matchKeys.map(keyed),
        rowLabel: (row) => `${String(row[0] ?? 'sample')} match ${String(row[2] ?? '')}`,
      }];
    }
  }
}

const EXPORT_NAMES: Record<AnalysisResult['kind'], string> = {
  diagnostics: 'Cluster diagnostics',
  fit: 'Cluster results',
  membership: 'Membership probabilities',
  euclidean: 'Euclidean matches',
};

/** Internal row keys and file paths never enter the visible results table. */
export function ResultTable({ result, rows: rowInfo, cutK = 2, selection }: {
  result: AnalysisResult; rows?: RowInfo; cutK?: number;
  /** Checked rows of the main table (row keys; see `selectableRows`). */
  selection?: GridSelection;
}): ReactElement {
  const sheets = resultTables(result, rowInfo, cutK);
  let method = '';
  if (result.kind === 'fit') method = result.data.method;
  if (result.kind === 'membership') method = result.data.effective_method === 'mahalanobis' ? 'Mahalanobis distances (lower is closer)' : 'Hotelling membership probabilities (%)';
  const meta = <>
    {method && <p className="muted">Method: {method}</p>}
    {result.kind === 'membership' && result.data.fallback_reason && <p role="status" className="callout callout-info">Hotelling probabilities were unavailable for this comparison. Showing Mahalanobis distances instead ({result.data.fallback_reason.replaceAll('_', ' ')}).</p>}
    {result.kind === 'membership' && result.data.eligibility_warning && <p role="status" className="callout callout-info">{result.data.eligibility_warning}</p>}
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
      note={`${sheet.rows.length} rows.${index === 0 && selection && sheet.keys?.some(Boolean) ? ' Check rows to assign them below.' : ''}`}
    >
      {sheet.rows.length === 0 ? <p className="empty-note">No eligible results for these settings.</p> : <ResultGrid
        tableId={sheet.id}
        columns={sheet.gridColumns ?? sheet.columns.map(label => ({ label }))}
        rows={sheet.rows}
        {...(index === 0 && selection && sheet.keys ? { keys: sheet.keys, selection } : {})}
        {...(sheet.rowLabel ? { rowLabel: sheet.rowLabel } : {})}
      />}
    </TableCard>)}
  </section>;
}

/** Plots, the result tables and the assignment controls of one result; owns the row selection. */
function ResultsPanel({ result, cutK, onCutKChange, busy, reviewing, onReviewChange, rowInfo, destinations, candidates, multiSource, onAssign }: {
  result: AnalysisResult; cutK: number; onCutKChange: (k: number) => void; busy: boolean; reviewing: boolean;
  onReviewChange: (reviewing: boolean) => void; rowInfo: RowInfo; destinations: readonly string[];
  candidates: readonly GroupCandidate[]; multiSource: boolean;
  onAssign: (request: TransferUnitsRequest | BatchTransferUnitsRequest) => void;
}): ReactElement {
  // Keys (UUIDs, or Euclidean match rows) survive sorting, filtering and column changes.
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  return <>
    <AnalysisPlots result={result} cutK={cutK} onCutKChange={onCutKChange} disabled={busy || reviewing} rowLabel={rowInfo.label} />
    <ResultTable result={result} rows={rowInfo} cutK={cutK} selection={{ selected, onChange: setSelected, disabled: busy || reviewing }} />
    <ResultAssignment result={result} cutK={cutK} destinations={destinations} candidates={candidates} multiSource={multiSource} busy={busy}
      onReviewChange={onReviewChange} onConfirm={onAssign} onBatchConfirm={onAssign} rowLabel={rowInfo.label}
      selected={selected} onSelectedChange={setSelected} />
  </>;
}

export function AnalysisPage({ kind, deps }: { kind: AnalysisKind; deps: AnalysisDeps }): ReactElement {
  const dataset = useGroupFiles(deps.groups);
  const { files, combined, candidates } = dataset;
  const paths = useMemo(() => dataset.ready.map(c => c.path), [dataset.ready]);
  const multiSource = files.length > 1;
  // Which samples are loaded and their values; unchanged when samples only
  // move between groups (see rowKeys.ts).
  const contentKey = useMemo(() => rowContentKey(combined), [combined]);
  const filesRef = useRef(files);
  filesRef.current = files;
  const data = useMemo(
    () => (dataset.load.kind === 'loaded' && files.length > 0 ? asRowsResponse(files, combined) : null),
    [dataset.load.kind, files, combined],
  );
  const [reviewing, setReviewing] = useState(false);
  const [group, setGroup] = useState('');
  const [id, setId] = useState('');
  const [source, setSource] = useState<AnalysisSource>('elements');
  const [pcCount, setPcCount] = useState(2);
  const [pcaVariance, setPcaVariance] = useState<{ explained: number[]; cumulative: number[] } | null>(null);
  const [plotGroup, setPlotGroup] = useState('');
  const [sourceGroup, setSourceGroup] = useState('');
  const [metric, setMetric] = useState<ClusterDistanceMetric>('euclidean');
  // R defaults: average linkage (clusterTab.R), 10 matches, exclude same-group matches (EuclideanDistance.R).
  const [linkage, setLinkage] = useState<ClusterLinkage>('average');
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
  const [diagnosticMaxK, setDiagnosticMaxK] = useState(10);
  const [seed, setSeed] = useState(20260914);
  const [limit, setLimit] = useState(10);
  const [within, setWithin] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [cutK, setCutK] = useState(2);
  const moving = useRef(false);
  const [result, setResult] = useState<AnalysisResult | null>(null);
  // Rows key of the inputs `result` was computed from.
  const [resultRowsKey, setResultRowsKey] = useState('');
  const generation = useRef(0);
  useEffect(() => {
    let active = true;
    deps.transformations.list().then(async list => Promise.all(list.transformations.map(d => deps.transformations.load(d.name))))
      .then(found => { if (active) setDefinitions(found); }).catch(e => { if (active) setError(`Could not load transformations: ${String(e)}`); });
    return () => { active = false; generation.current++; activeJob.current?.abort(); };
  }, [deps]);
  // New samples (a Dataset change) or a new setup: reset settings. With
  // several groups the synthetic group column is the default grouping.
  // Samples moving between groups keep the settings.
  const defaultGroup = defaultGroupColumn(combined);
  const settingsData = useRef<{ key: string; setup: unknown } | null>(null);
  useEffect(() => {
    if (!data) return;
    const ids = uniqueIdColumns(data);
    const previous = settingsData.current;
    settingsData.current = { key: contentKey, setup: analysisSetup.setup };
    if (!previous || previous.key !== contentKey || previous.setup !== analysisSetup.setup) {
      setGroup(defaultGroup);
      setId(defaultIdColumn(data, ids)); setSourceGroup(defaultGroup); setPlotGroup(defaultGroup); setProjection(null);
      setDefinitionName(analysisSetup.setup ? ANALYSIS_SETUP_NAME : '');
      return;
    }
    // Same samples: keep each choice unless its column is gone.
    const has = (c: string) => data.descriptive_columns.includes(c);
    setGroup(g => (has(g) ? g : defaultGroup));
    setSourceGroup(g => (has(g) ? g : defaultGroup));
    setPlotGroup(g => (g === '' || has(g) ? g : defaultGroup));
    setId(i => (ids.includes(i) ? i : defaultIdColumn(data, ids)));
  }, [data, contentKey, defaultGroup, analysisSetup.setup]);
  useEffect(() => { if (dataset.load.kind === 'error') setError(dataset.load.message); }, [dataset.load]);
  // PCA of the same inputs the analysis uses, for "First n PCs (var% / cum%)"
  // labels. Keyed by content, so a reload or a move between groups (same
  // samples, same values) does not re-run it or reset the component count.
  const pcaInputKey = data && columns.length ? JSON.stringify([contentKey, columns, definition]) : '';
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const pcaInput = useMemo(() => (pcaInputKey ? { columns, definition } : null), [pcaInputKey]);
  useEffect(() => {
    setPcaVariance(null);
    if (source !== 'pca' || !pcaInput || !deps.ordination) return;
    let active = true;
    deps.ordination.pca({ ...inputPaths(filesRef.current), columns: pcaInput.columns, transformation: pcaInput.definition, scale: false })
      .then(fitted => { if (active) setPcaVariance({ explained: fitted.explained_variance, cumulative: fitted.cumulative_variance }); })
      .catch(() => { /* Unlabelled choices remain. */ });
    return () => { active = false; };
  }, [source, pcaInput, deps.ordination]);
  const pcMax = !data ? 0 : pcaVariance?.explained.length || Math.max(1, Math.min(columns.length, data.rows.length));
  // Default to every component (GroupMembership.R / EuclideanDistance.R).
  useEffect(() => { if (pcMax) setPcCount(pcMax); }, [pcMax]);
  const idChoices = useMemo(() => (data ? uniqueIdColumns(data) : []), [data]);
  const featureCount = sourceFeatureCount(source, columns.length, pcCount, data ? projectionLabels(data, sourceGroup).length : 0);
  const eligibility = useMemo(() => (data && kind === 'membership' ? groupEligibility(data, group, featureCount) : []), [data, kind, group, featureCount]);

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
  // A result stays valid while the same samples are loaded; a Dataset change
  // that adds or removes samples clears it.
  useEffect(() => {
    if (result && resultRowsKey !== contentKey) { setResult(null); setReviewing(false); }
  }, [result, resultRowsKey, contentKey]);
  async function run(diagnostics = false) {
    if (activeJob.current || moving.current) return;
    const ticket = ++generation.current;
    const rowsKey = contentKey;
    setBusy(true); setError(''); setNotice(''); setReviewing(false); setCutK(2); setResult(null);
    try {
      // UMAP always uses the Ordination embedding's seed; the clustering seed is separate.
      const input = { ...inputPaths(files), columns, transformation: definition, ...sourceOptions(source, pcCount, sourceGroup, UMAP_SEED) };
      let analysis: AnalysisJobRequest;
      if (kind === 'cluster') {
        analysis = diagnostics
          ? { kind: 'cluster_diagnostics', request: { ...input, max_k: diagnosticMaxK, seed, diagnostic_method: diagnosticMethod, metric: diagnosticMethod === 'kmeans' ? 'euclidean' : diagnosticMetric } }
          : { kind: 'cluster_fit', request: { ...input, method, k, seed, plot_group_column: plotGroup || null, iter_max: iterations, nstart: starts, metric: method === 'kmeans' ? 'euclidean' : metric, linkage, minkowski_p: minkowskiP } };
      } else if (kind === 'membership') {
        const eligibleNow = new Set(eligibility.filter(g => g.eligible).map(g => g.group));
        analysis = { kind: 'membership_probabilities', request: { ...input, group_column: group, id_column: id, method: membershipMethod, projection_groups: projection?.filter(g => eligibleNow.has(g)) ?? null } };
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
      if (ticket === generation.current) { setResult(next); setResultRowsKey(rowsKey); }
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
      setReviewing(false);
      // Cluster results describe the same samples after a move (and are
      // matched to rows by sample), so further clusters can be assigned
      // without recomputing. Membership and matches depend on the groups,
      // as does an LDA source grouped by group file.
      const groupDependent = kind !== 'cluster' || (source === 'lda' && sourceGroup === combined.groupColumn);
      if (groupDependent) setResult(null);
      setNotice(groupDependent
        ? `Moved ${outcome.moved} analytical units. Recompute analysis for the updated data.`
        : `Moved ${outcome.moved} analytical units. The clustering still applies to the same samples; assign more clusters or recompute.`);
      try {
        // Keep the selection, plus any group a move created.
        const created = outcome.transactions.flatMap(tx => tx.outputs.map(o => o.path));
        const selected = dataset.selectedPaths;
        // The new revisions reload only the files the move changed.
        await dataset.rescan(all => all.filter(p => selected.includes(p) || created.includes(p)));
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
        {source === 'pca' && <label className="field">Principal components <select value={pcCount} onChange={e => setPcCount(Number(e.target.value))}>
          {pcCountOptions(pcMax, pcaVariance?.explained, pcaVariance?.cumulative).map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
        </select><span className="field-hint">Shown as (PC variance / cumulative variance).</span></label>}
        {source === 'lda' && <label className="field">LDA grouping column <select value={sourceGroup} onChange={e => setSourceGroup(e.target.value)}>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>}
        {source === 'umap' && <p className="field-hint">UMAP uses the same embedding as the Ordination page (seed {UMAP_SEED}).</p>}
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
          <label className="field">Cluster count <input type="number" min={1} max={20} value={k} onChange={e => setK(Number(e.target.value))} /></label>
          {method === 'kmeans' && <><label className="field">Starts <input type="number" min={1} max={100} value={starts} onChange={e => setStarts(Number(e.target.value))} /></label><label className="field">Maximum iterations <input type="number" min={1} max={200} value={iterations} onChange={e => setIterations(Number(e.target.value))} /></label></>}
          {(method === 'kmeans' || method === 'pam') && <label className="field">Plot grouping column <select value={plotGroup} onChange={e => setPlotGroup(e.target.value)}><option value="">No group colors</option>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>}
          <label className="field">Diagnostic maximum k <input type="number" min={2} max={20} value={diagnosticMaxK} onChange={e => setDiagnosticMaxK(Number(e.target.value))} /></label>
          <label className="field">Diagnostic method <select value={diagnosticMethod} onChange={e => setDiagnosticMethod(e.target.value as 'kmeans' | 'pam')}><option value="kmeans">k-means</option><option value="pam">k-medoids (PAM)</option></select></label>
          {diagnosticMethod === 'pam' && <label className="field">Diagnostic distance <select value={diagnosticMetric} onChange={e => setDiagnosticMetric(e.target.value as 'euclidean' | 'manhattan')}><option value="euclidean">Euclidean</option><option value="manhattan">Manhattan</option></select></label>}
          <label className="field">Random seed <input type="number" min={-2147483648} max={2147483627} value={seed} onChange={e => setSeed(Number(e.target.value))} /></label>
        </> : <>
          <label className="field">Group column <select value={group} onChange={e => { setGroup(e.target.value); setProjection(null); }}>{data.descriptive_columns.map(c => <option key={c}>{c}</option>)}</select></label>
          <label className="field">ID column <select value={id} onChange={e => setId(e.target.value)}>{idChoices.map(c => <option key={c}>{c}</option>)}</select><span className="field-hint">Only columns with a unique value per sample.</span></label>
          {kind === 'membership' ? <label className="field">Method <select value={membershipMethod} onChange={e => setMembershipMethod(e.target.value as MembershipMethod)}><option value="hotellings">Hotelling probabilities (Mahalanobis fallback)</option><option value="mahalanobis">Mahalanobis distances</option></select></label> : <>
            <label className="field">Matches per analytical unit <input type="number" min={1} max={100} value={limit} onChange={e => setLimit(Number(e.target.value))} /></label>
            <label className="field-check"><input type="checkbox" checked={within} onChange={e => setWithin(e.target.checked)} />Include same-group matches</label>
          </>}
          {kind === 'membership'
            ? <EligibleGroupPicker groups={eligibility} selected={projection} featureCount={featureCount} onChange={next => { reset(); setProjection(next); }} />
            : <label className="field field-wide">Projection groups <GroupMultiSelect label="Projection groups" options={projectionLabels(data, group).map(g => ({ path: g, name: g }))} selected={projection ?? projectionLabels(data, group)} onChange={next => { reset(); setProjection(next); }} /></label>}
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
    {kind === 'membership' && data && group && <GroupSizesTable groups={eligibility} column={group} />}
    {busy && <BusyNote>{moving.current ? 'Assigning analytical units to groups…' : cancelling ? 'Cancelling analysis…' : job ? `${job.state}: ${job.stage?.replaceAll('_', ' ') ?? 'waiting'} (${job.progress}%)` : 'Submitting analysis…'}</BusyNote>}
    {notice && <p role="status" className="callout callout-info">{notice}</p>}{error && <p role="alert">{error}</p>}
    {result && <div key={generation.current} className="results">
      <ResultsPanel result={result} cutK={cutK} onCutKChange={setCutK} busy={busy} reviewing={reviewing} onReviewChange={setReviewing} rowInfo={rowInfo}
        destinations={paths} candidates={candidates} multiSource={multiSource} onAssign={request => void assign(request)} />
    </div>}
  </section>;
}
