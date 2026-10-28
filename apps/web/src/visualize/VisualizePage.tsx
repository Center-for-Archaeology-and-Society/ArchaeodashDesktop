/**
 * Visualize & Assign route (Section 9.4): Plotly scatter over one or more
 * group files (all groups by default, colored by group) on elemental, PCA,
 * UMAP, or LDA axes, lasso/box selection keyed internally
 * by the hidden analytical_uuid, metadata field/value filter with (Missing)
 * normalization, viridis colors, repeating ten-symbol map, optional data
 * ellipse (0.50–0.99), optional labels (ANID / sample ID / row number), and
 * assignment of the selected units to an existing or new group via the
 * atomic transfer-units transaction.
 */
import { useCallback, useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import type {
  ExportsService,
  GroupRowDto,
  GroupsService,
  OrdinationService,
  TransformationsService,
} from '@archaeodash/client';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { UMAP_SEED, analysisInput, setupSummary } from '../analysis-setup/setup.ts';
import { holdGroupUpdates, notifyGroupsChanged } from '../data-manager/events.ts';
import { useGroupUpdatesHeld, useHeldValue } from '../datasets/groupUpdates.ts';
import { portableGroupNameProblem } from '../datasets/groupNames.ts';
import { selectionBySource } from './combine.ts';
import { GroupMultiSelect } from '../datasets/GroupMultiSelect.tsx';
import { useGroupFiles } from '../datasets/useGroupFiles.ts';
import { realignByUuid, rowContentKey, rowGroupKey } from '../datasets/rowKeys.ts';

export { GroupMultiSelect };
import { VisualizeScatter, type ScatterPoint, type ScatterTraceSpec } from './VisualizeScatter.tsx';
import { Multiplot } from './Multiplot.tsx';
import {
  PALETTE_THEMES,
  clearSelection,
  ellipsePoints,
  emptySelection,
  normalizeFilterValue,
  paletteColors,
  replaceSelection,
  rowPassesFilterValues,
  symbolFor,
  type PaletteTheme,
  type Selection,
} from './visualize-model.ts';
import { includedAfterMove, moveAcrossSources } from '../clustering/multi-source-move.ts';
import { RERUN_DELAY_MS, waitUnlessAborted } from '../ordination/rerun.ts';
import { plotlyText } from '../charts/plotly-text.ts';
import { ValueMultiSelect } from './ValueMultiSelect.tsx';
import { BusyLabel, BusyNote } from '../ui/Busy.tsx';
import { TableCard } from '../ui/TableExport.tsx';
import { PageHeader } from '../ui/PageHeader.tsx';

export interface VisualizeDeps {
  readonly groups: GroupsService;
  readonly ordination: OrdinationService;
  readonly exports: ExportsService;
  /** Applies the Analysis setup transform for the element axes. */
  readonly transformations?: Pick<TransformationsService, 'apply'>;
}

/** Element-axis data: column names and column-major values per combined row. */
interface ElementView {
  readonly columns: string[];
  readonly values: (number | null)[][];
}

type AsyncState<T> =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'loaded'; data: T };

/** Stable empty view while a transformed element view loads or fails. */
const EMPTY_VIEW: ElementView = { columns: [], values: [] };

/** File name the backend gives a new group (`<sanitized name>.parquet`). */
export function newGroupFileName(name: string): string {
  return `${name.trim().replace(/[^A-Za-z0-9._-]/g, '_') || 'group'}.parquet`;
}

/**
 * Why a typed new-group name cannot be used, or null. A name (or file name)
 * matching an existing group would silently merge into it, so it is refused.
 */
export function newGroupNameProblem(
  name: string,
  destinationPath: string,
  existing: readonly { path: string; name: string }[],
): string | null {
  const trimmed = name.trim();
  if (trimmed === '') return 'Enter a name for the new group.';
  const portable = portableGroupNameProblem(trimmed);
  if (portable) return portable;
  const clash = existing.find(
    (g) => g.name.trim().toLowerCase() === trimmed.toLowerCase() || g.path.toLowerCase() === destinationPath.toLowerCase(),
  );
  return clash ? `A group “${clash.name}” already exists there; choose it under Target group instead.` : null;
}

/** Text for a point label: the chosen column's value, or the visible ID. */
function labelValue(
  row: GroupRowDto,
  column: string,
  combined: { visibleIdColumn: string; descriptiveColumns: readonly string[]; elementalColumns: readonly string[] },
  rowNumber: number,
): string {
  const d = combined.descriptiveColumns.indexOf(column);
  if (d >= 0) return row.descriptive[d] ?? '';
  const e = combined.elementalColumns.indexOf(column);
  if (e >= 0) {
    const v = row.elemental[e];
    return v === null || v === undefined ? '' : String(v);
  }
  return row.visible_id ?? row.legacy_rowid ?? String(rowNumber);
}

/** Convert row-major API records into the column-major arrays used by plots. */
export function transposeElementalRows(
  rows: readonly GroupRowDto[],
  columnCount: number,
): (number | null)[][] {
  return Array.from({ length: columnCount }, (_, columnIndex) =>
    rows.map((row) => row.elemental[columnIndex] ?? null),
  );
}

/** Pure selected-rows table: ANID, metadata, predictors — never the UUID. */
export function SelectedRowsTable({
  rows,
  columns,
  descriptiveColumns,
  elementalColumns,
}: {
  rows: readonly GroupRowDto[];
  columns: { visibleId: string; legacyRowid: string };
  descriptiveColumns: readonly string[];
  elementalColumns: readonly string[];
}): ReactElement {
  if (rows.length === 0) return <p className="muted">No rows selected.</p>;
  return (
    <table className="data-table">
      <thead>
        <tr>
          <th>{columns.visibleId}</th>
          {descriptiveColumns.map((c) => (
            <th key={c}>{c}</th>
          ))}
          {elementalColumns.map((c) => (
            <th key={c}>{c}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.analytical_uuid}>
            <td>{r.visible_id ?? r.legacy_rowid ?? ''}</td>
            {descriptiveColumns.map((c, i) => (
              <td key={c}>{normalizeFilterValue(r.descriptive[i])}</td>
            ))}
            {elementalColumns.map((c, i) => (
              <td key={c}>{r.elemental[i] ?? 'NA'}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export type AxisSource = 'elements' | 'pca' | 'umap' | 'lda';

const AXIS_SOURCES: readonly [AxisSource, string][] = [
  ['elements', 'Elements'],
  ['pca', 'PCA'],
  ['umap', 'UMAP'],
  ['lda', 'LDA'],
];

/** Ordination scores for the rows `uuids` (in the order they were requested). */
interface Scores {
  readonly names: string[];
  readonly labels: string[];
  readonly uuids: string[];
  readonly values: number[][];
  readonly note?: string;
}

/** Transformed element values for the rows `uuids`, row-major. */
interface TransformedRows {
  readonly uuids: string[];
  readonly columns: string[];
  readonly rows: (number | null)[][];
}

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function VisualizePage({ deps }: { deps: VisualizeDeps }): ReactElement {
  const [source, setSource] = useState<AxisSource>('elements');
  const [scores, setScores] = useState<AsyncState<Scores>>({ kind: 'idle' });
  const [xIdx, setXIdx] = useState(0);
  const [yIdx, setYIdx] = useState(1);
  const [plotMode, setPlotMode] = useState<'single' | 'multiplot'>('single');
  const [filterColumn, setFilterColumn] = useState('');
  const [filterValues, setFilterValues] = useState<ReadonlySet<string>>(() => new Set());
  // Legacy defaults: viridis, data ellipse on at 0.90, group symbols on,
  // labels off with the visible ID as label column.
  const [paletteTheme, setPaletteTheme] = useState<PaletteTheme>('viridis');
  const [ellipseOn, setEllipseOn] = useState(true);
  const [ellipseLevel, setEllipseLevel] = useState(0.9);
  const [useSymbols, setUseSymbols] = useState(true);
  const [symbolChoice, setSymbolChoice] = useState('');
  const [showLabels, setShowLabels] = useState(false);
  const [labelColumn, setLabelColumn] = useState('');
  const [selection, setSelection] = useState<Selection>(clearSelection({ uuids: new Set() }));
  const [assignTarget, setAssignTarget] = useState('');
  const [newGroupName, setNewGroupName] = useState('');
  const [assignState, setAssignState] = useState<AsyncState<number>>({ kind: 'idle' });
  // Path of a group this page is creating; once it appears in the group list
  // it becomes the target, so the next assignment adds to it instead of
  // tripping the "already exists" check on the same new name.
  const [createdTarget, setCreatedTarget] = useState<string | null>(null);

  const dataset = useGroupFiles(deps.groups);
  const { ready, options: groupOptions, selectedPaths, setSelectedPaths, orderedPaths, files, combined, rescan } = dataset;
  useEffect(() => {
    if (createdTarget === null) return;
    const created = ready.find((c) => c.path.toLowerCase() === createdTarget.toLowerCase());
    if (!created) return;
    setAssignTarget(created.path);
    setNewGroupName('');
    setCreatedTarget(null);
  }, [ready, createdTarget]);
  const loadState = dataset.load;
  // While samples move (and pages and the Data Manager refresh), the plot
  // keeps its last picture and ordinations wait, then both update once.
  const held = useGroupUpdatesHeld();
  const analysisSetup = useAnalysisSetup();
  // Results that depend only on the loaded samples and their values (PCA,
  // UMAP, element transforms) are keyed by `contentKey`, so moving samples
  // between groups does not recompute them; LDA also depends on the groups.
  const rowUuids = useMemo(() => combined.rows.map((r) => r.analytical_uuid), [combined]);
  const contentKey = useMemo(() => rowContentKey(combined), [combined]);
  const groupKey = useMemo(() => rowGroupKey(combined), [combined]);
  // The rows a content-keyed request reads (current at request time).
  const loadedRef = useRef({ files, rowUuids });
  loadedRef.current = { files, rowUuids };
  const setupInputFresh = useMemo(
    () => analysisInput(analysisSetup.setup, combined.elementalColumns),
    [analysisSetup.setup, combined.elementalColumns],
  );
  // Same object while the setup's content is unchanged (a reload rebuilds
  // the column list), so effects keyed on it do not re-run.
  const setupInputKey = JSON.stringify(setupInputFresh);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const setupInput = useMemo(() => setupInputFresh, [setupInputKey]);

  // Element axes use the Analysis setup's elements; with a transform, values
  // come from the backend over all loaded groups pooled (like ordination).
  const rawElementView = useMemo((): ElementView => {
    const columns = setupInput.elements;
    return {
      columns,
      values: columns.map((c) => {
        const i = combined.elementalColumns.indexOf(c);
        return combined.rows.map((r) => r.elemental[i] ?? null);
      }),
    };
  }, [setupInput.elements, combined]);
  const [transformed, setTransformed] = useState<AsyncState<TransformedRows>>({ kind: 'idle' });
  useEffect(() => {
    const definition = setupInput.transformation;
    const apply = deps.transformations?.apply;
    const { files: current, rowUuids: uuids } = loadedRef.current;
    const [first, ...rest] = current.map((f) => f.path);
    if (!definition || !apply || !first || setupInput.columns.length === 0) {
      setTransformed({ kind: 'idle' });
      return;
    }
    let cancelled = false;
    setTransformed({ kind: 'loading' });
    apply({ path: first, extra_paths: rest, definition })
      .then((r) => {
        if (!cancelled) setTransformed({ kind: 'loaded', data: { uuids, columns: r.columns, rows: r.rows } });
      })
      .catch((err: unknown) => {
        if (!cancelled) setTransformed({ kind: 'error', message: errorText(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [deps, contentKey, setupInput]);
  const transformedView = useMemo((): AsyncState<ElementView> => {
    if (transformed.kind !== 'loaded') return transformed;
    const { uuids, columns, rows } = transformed.data;
    const aligned = realignByUuid(uuids, rows, rowUuids);
    return {
      kind: 'loaded',
      data: { columns, values: columns.map((_, j) => aligned.map((row) => row?.[j] ?? null)) },
    };
  }, [transformed, rowUuids]);
  const elementView: ElementView =
    setupInput.transformation && transformedView.kind === 'loaded' ? transformedView.data
      : setupInput.transformation ? EMPTY_VIEW
        : rawElementView;

  // A new set of rows invalidates the current selection.
  useEffect(() => {
    setSelection(clearSelection({ uuids: new Set() }));
  }, [contentKey]);
  const colors = useMemo(() => paletteColors(paletteTheme, combined.groupNames.length), [combined, paletteTheme]);
  const colorOf = useCallback(
    (group: string) => colors[combined.groupNames.indexOf(group)] ?? '#888',
    [colors, combined],
  );

  // Ordination scores over all loaded groups (one request), kept by row UUID.
  // PCA and UMAP re-run only when the samples, their values, or the setup
  // change; LDA (grouped by file) also when samples change group.
  const scoresRowsKey = source === 'lda' ? groupKey : contentKey;
  // Rows key of the last run: a rerun after the group selection changes
  // waits RERUN_DELAY_MS so a burst of checks/unchecks starts one run.
  const lastScoresRowsKey = useRef<string | null>(null);
  // Inputs of the last finished run: after a hold, unchanged inputs keep it.
  const lastScoresRun = useRef<string | null>(null);
  useEffect(() => {
    if (held) return;
    const runKey = JSON.stringify([source, scoresRowsKey, setupInput, analysisSetup.loaded]);
    if (runKey === lastScoresRun.current) return;
    lastScoresRun.current = null;
    const { files: current, rowUuids: uuids } = loadedRef.current;
    if (source === 'elements' || current.length === 0 || !analysisSetup.loaded) {
      setScores({ kind: 'idle' });
      return;
    }
    const columns = setupInput.columns;
    const [first, ...rest] = current.map((f) => f.path);
    if (!first || columns.length < 2) {
      setScores({ kind: 'error', message: 'Ordination needs at least two selected elements shared by the loaded groups.' });
      return;
    }
    // Leaving this effect (new inputs, new source, unmount) aborts the run,
    // which cancels its backend work.
    const controller = new AbortController();
    const { signal } = controller;
    const rowsChanged = lastScoresRowsKey.current !== null && lastScoresRowsKey.current !== scoresRowsKey;
    lastScoresRowsKey.current = scoresRowsKey;
    setScores({ kind: 'loading' });
    const base = { path: first, extra_paths: rest, columns, transformation: setupInput.transformation ?? null };
    const run = async (): Promise<Scores> => {
      if (source === 'pca') {
        const r = await deps.ordination.pca(base, { signal });
        return {
          names: r.score_names,
          labels: r.score_names.map(
            (n, i) => `${n} (${((r.explained_variance[i] ?? 0) * 100).toFixed(1)}%)`,
          ),
          uuids,
          values: r.scores,
        };
      }
      if (source === 'umap') {
        const r = await deps.ordination.umap({ ...base, seed: UMAP_SEED }, { signal });
        return {
          names: r.score_names,
          labels: r.score_names.map((n) => `UMAP ${n}`),
          uuids,
          values: r.embedding,
          note: r.warnings.join('; ') || undefined,
        };
      }
      if (current.length < 3) {
        throw new Error('LDA needs at least three groups; select more groups in Dataset.');
      }
      const r = await deps.ordination.lda({ ...base, group_column: '', group_by_file: true }, { signal });
      return {
        names: r.score_names,
        labels: r.score_names,
        uuids,
        values: r.scores,
        note: r.warnings.join('; ') || undefined,
      };
    };
    void (async () => {
      if (rowsChanged && !(await waitUnlessAborted(RERUN_DELAY_MS, signal))) return;
      try {
        const s = await run();
        if (!signal.aborted) {
          lastScoresRun.current = runKey;
          setScores({ kind: 'loaded', data: s });
        }
      } catch (err: unknown) {
        if (!signal.aborted) setScores({ kind: 'error', message: errorText(err) });
      }
    })();
    return () => controller.abort();
  }, [deps, source, scoresRowsKey, setupInput, analysisSetup.loaded, held]);
  const scoreRows = useMemo(
    () => (scores.kind === 'loaded' ? realignByUuid(scores.data.uuids, scores.data.values, rowUuids) : []),
    [scores, rowUuids],
  );

  // Reset axes to the first two dimensions whenever the axis source changes.
  useEffect(() => {
    setXIdx(0);
    setYIdx(1);
  }, [source]);

  const filteredIdx = useMemo(() => {
    const all = combined.rows.map((_, i) => i);
    if (filterColumn === '' || filterValues.size === 0) return all;
    const colIdx = combined.descriptiveColumns.indexOf(filterColumn);
    if (colIdx < 0) return all;
    return all.filter((i) => rowPassesFilterValues(combined.rows[i]!, colIdx, filterValues));
  }, [combined, filterColumn, filterValues]);

  // Like the legacy app, keep only selected points that are still displayed,
  // so a filter change never leaves hidden units queued for assignment.
  useEffect(() => {
    setSelection((current) => {
      if (current.uuids.size === 0) return current;
      const shown = new Set(filteredIdx.map((i) => combined.rows[i]!.analytical_uuid));
      const kept = [...current.uuids].filter((u) => shown.has(u));
      return kept.length === current.uuids.size ? current : replaceSelection(kept);
    });
  }, [filteredIdx, combined]);

  const filterChoices = useMemo(() => {
    const colIdx = combined.descriptiveColumns.indexOf(filterColumn);
    if (colIdx < 0) return [];
    return [...new Set(combined.rows.map((r) => normalizeFilterValue(r.descriptive[colIdx])))].sort();
  }, [combined, filterColumn]);

  // Symbols default to the current group column (legacy "Use group symbols").
  const symbolColumn = !useSymbols
    ? ''
    : combined.descriptiveColumns.includes(symbolChoice)
      ? symbolChoice
      : combined.groupColumn;
  const labelColumnName = labelColumn !== '' && (combined.descriptiveColumns.includes(labelColumn) || combined.elementalColumns.includes(labelColumn))
    ? labelColumn
    : combined.visibleIdColumn;

  const symbolLevels = useMemo(() => {
    const colIdx = combined.descriptiveColumns.indexOf(symbolColumn);
    if (colIdx < 0) return [] as string[];
    return [...new Set(combined.rows.map((r) => normalizeFilterValue(r.descriptive[colIdx])))].sort();
  }, [combined, symbolColumn]);

  const symbolLegend = useMemo(
    () => symbolLevels.map((name, i) => ({ name, symbol: symbolFor(i) })),
    [symbolLevels],
  );

  const coordinate = useCallback(
    (rowIdx: number, axis: number): number | null => {
      if (source === 'elements') return elementView.values[axis]?.[rowIdx] ?? null;
      const v = scoreRows[rowIdx]?.[axis];
      return v === undefined || !Number.isFinite(v) ? null : v;
    },
    [source, elementView, scoreRows],
  );

  const axisNames = useMemo(
    () => (source === 'elements' ? elementView.columns : scores.kind === 'loaded' ? scores.data.names : []),
    [source, elementView, scores],
  );
  // Keep the axes valid when the available dimensions shrink (for example
  // after elements are removed from the Analysis setup).
  useEffect(() => {
    const n = axisNames.length;
    if (n === 0) return;
    setXIdx((x) => (x < n ? x : 0));
    setYIdx((y) => (y < n ? y : Math.min(1, n - 1)));
  }, [axisNames]);

  const traces = useMemo((): ScatterTraceSpec[] => {
    const symIdx = combined.descriptiveColumns.indexOf(symbolColumn);
    const byGroup = new Map<string, ScatterPoint[]>(combined.groupNames.map((g) => [g, []]));
    for (const rowIdx of filteredIdx) {
      const x = coordinate(rowIdx, xIdx);
      const y = coordinate(rowIdx, yIdx);
      if (x === null || y === null) continue;
      const row = combined.rows[rowIdx]!;
      const groupName = combined.groupOf[rowIdx]!;
      const symbolValue = symIdx >= 0 ? normalizeFilterValue(row.descriptive[symIdx]) : '';
      const symbol = symIdx >= 0 ? symbolFor(symbolLevels.indexOf(symbolValue)) : symbolFor(0);
      const label = labelValue(row, labelColumnName, combined, rowIdx + 1);
      const id = row.visible_id ?? row.legacy_rowid ?? `Row ${rowIdx + 1}`;
      // Legacy hover: group, ID, label column, symbol field, x and y values.
      const hoverLines = [groupName, `${combined.visibleIdColumn}: ${id}`];
      if (labelColumnName !== combined.visibleIdColumn) hoverLines.push(`${labelColumnName}: ${label}`);
      if (symIdx >= 0 && symbolColumn !== combined.groupColumn) hoverLines.push(`${symbolColumn}: ${symbolValue}`);
      hoverLines.push(`${axisNames[xIdx] ?? 'x'}: ${x}`, `${axisNames[yIdx] ?? 'y'}: ${y}`);
      byGroup.get(groupName)?.push({
        uuid: row.analytical_uuid,
        x,
        y,
        label,
        // IDs, metadata and names are user data: escaped so Plotly shows
        // them as text, not as its pseudo-HTML.
        hover: hoverLines.map(plotlyText).join('<br>'),
        groupName,
        color: colorOf(groupName),
        symbol,
      });
    }
    return [...byGroup.entries()]
      .filter(([, points]) => points.length > 0)
      .map(([name, points]) => ({
        name,
        color: colorOf(name),
        symbol: symbolFor(0),
        points,
        showLabels,
        ellipse: ellipseOn ? ellipsePoints(points.map((p) => p.x), points.map((p) => p.y), ellipseLevel) : undefined,
      }));
  }, [combined, filteredIdx, coordinate, xIdx, yIdx, symbolColumn, symbolLevels, labelColumnName, showLabels, ellipseOn, ellipseLevel, colorOf, axisNames]);

  const shownTraces = useHeldValue(traces, held);
  const multiplotData = useHeldValue(
    {
      columns: elementView.columns,
      values: elementView.values,
      groupLabels: combined.groupOf,
      groupNames: combined.groupNames,
      palette: colors,
      rowIndices: filteredIdx,
      rowUuids,
    },
    held,
  );

  const selectUuids = useCallback((uuids: readonly string[]) => setSelection(replaceSelection(uuids)), []);
  const clearSelected = useCallback(() => setSelection(emptySelection()), []);

  const selectedRows = useMemo(
    () => combined.rows.filter((r) => selection.uuids.has(r.analytical_uuid)),
    [combined, selection],
  );

  const axisOptions = useMemo(() => {
    if (source === 'elements') {
      return elementView.columns.map((c, i) => ({ value: String(i), label: c }));
    }
    if (scores.kind !== 'loaded') return [];
    return scores.data.labels.map((label, i) => ({ value: String(i), label }));
  }, [source, elementView, scores]);

  const axisLabel = (idx: number, fallback: string) =>
    axisOptions[idx]?.label ?? fallback;

  const sameAxis = xIdx === yIdx;

  // New groups land beside the first source with the backend's sanitized-name
  // convention (`<folder>/<Name>.parquet`); existing targets use their path.
  const newDestinationPath = (orderedPaths[0] ?? 'groups/group.parquet').replace(/[^/\\]+$/, newGroupFileName(newGroupName));
  const existingGroups = useMemo(
    () => ready.map((c) => ({ path: c.path, name: c.group?.group_name ?? c.path })),
    [ready],
  );
  const newNameProblem =
    assignTarget === '__new__' && newGroupName.trim() !== ''
      ? newGroupNameProblem(newGroupName, newDestinationPath, existingGroups)
      : null;

  const assign = useCallback(async () => {
    if (selection.uuids.size === 0) return;
    const isNew = assignTarget === '__new__';
    const name = newGroupName.trim();
    const destinationPath = isNew ? newDestinationPath : assignTarget;
    if (isNew) {
      const problem = newGroupNameProblem(name, destinationPath, existingGroups);
      if (problem) {
        setAssignState({ kind: 'error', message: problem });
        return;
      }
    }
    const bySource = selectionBySource(combined, selection.uuids);
    const sourceOf = new Map<string, string>();
    for (const [sourcePath, uuids] of bySource) for (const u of uuids) sourceOf.set(u, sourcePath);
    const sources = [...bySource.keys()].filter((p) => p !== destinationPath);
    if (sources.length === 0) {
      setAssignState({ kind: 'error', message: 'Every selected unit is already in that group; nothing was moved.' });
      return;
    }
    setAssignState({ kind: 'loading' });
    // Set before the move: a multi-source move can create the group and then
    // fail part-way, and the group should still become the target.
    if (isNew) setCreatedTarget(destinationPath);
    const release = holdGroupUpdates();
    // Include a group this move creates; an existing destination keeps its
    // inclusion (an unchecked group stays unchecked).
    const keep = (all: string[]) => includedAfterMove(all, selectedPaths, isNew ? [destinationPath] : []);
    try {
      const outcome = await moveAcrossSources(
        deps.groups,
        {
          action: 'move',
          source_path: sources[0]!,
          destination_path: destinationPath,
          destination_group_name: isNew ? name : null,
          selected_uuids: [...sourceOf.keys()],
          expected_source_revision: '',
        },
        { sourceOf, sources, revisions: new Map(files.map((f) => [f.path, f.data.revision_id])) },
      );
      setSelection(replaceSelection([]));
      setAssignState({ kind: 'loaded', data: outcome.moved });
      // Rescan (a new group may exist, an emptied one may be gone) and keep
      // the current selection plus a newly created destination group. The new revisions
      // reload only the files that changed; plot coordinates are kept by
      // sample, so PCA and UMAP are not recomputed.
      await rescan(keep);
      await notifyGroupsChanged();
    } catch (err: unknown) {
      setAssignState({ kind: 'error', message: errorText(err) });
      // Part of a multi-source move may have committed: refresh revisions.
      await rescan(keep).catch(() => undefined);
      dataset.reload();
      await notifyGroupsChanged();
    } finally {
      release();
    }
  }, [deps, combined, files, selection, assignTarget, newGroupName, newDestinationPath, existingGroups, selectedPaths, rescan, dataset]);

  const scoreState =
    source === 'elements' ? null : scores.kind === 'loading' ? 'loading' : scores.kind === 'error' ? scores.message : null;

  return (
    <section className="page" aria-labelledby="visualize-heading">
      <PageHeader id="visualize-heading" title="Visualize & Assign" description="Plot samples on elements or ordination scores, lasso a selection, and move it to a group.">
        <label className="field field-inline">
          Dataset
          <GroupMultiSelect options={groupOptions} selected={selectedPaths} onChange={setSelectedPaths} />
        </label>
      </PageHeader>
      <div className="explore-controls">
        <label>
          Axes
          <select aria-label="Axis source" value={source} onChange={(e) => setSource(e.target.value as AxisSource)}>
            {AXIS_SOURCES.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          X axis
          <select value={String(xIdx)} onChange={(e) => setXIdx(Number(e.target.value))}>
            {axisOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Y axis
          <select value={String(yIdx)} onChange={(e) => setYIdx(Number(e.target.value))}>
            {axisOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>
        {sameAxis && (
          <span role="alert">
            X and Y must be different axes; the legacy plot rejects identical selections.
          </span>
        )}
      </div>
      <div className="explore-controls">
        <label>
          Filter field
          <select value={filterColumn} onChange={(e) => { setFilterColumn(e.target.value); setFilterValues(new Set()); }}>
            <option value="">(none)</option>
            {combined.descriptiveColumns.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        {filterColumn !== '' && (
          <label>
            Filter values
            <ValueMultiSelect label="Filter values" values={filterChoices} selected={filterValues} onChange={setFilterValues} />
          </label>
        )}
        {filterColumn !== '' && (
          <button type="button" className="btn btn-sm" onClick={() => { setFilterColumn(''); setFilterValues(new Set()); }}>
            Clear filter
          </button>
        )}
        <label>
          Colors
          <select value={paletteTheme} onChange={(e) => setPaletteTheme(e.target.value as PaletteTheme)}>
            {PALETTE_THEMES.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Plot mode
          <select value={plotMode} onChange={(e) => setPlotMode(e.target.value as 'single' | 'multiplot')}>
            <option value="single">Single plot</option>
            <option value="multiplot">Multiplot</option>
          </select>
        </label>
      </div>
      <div className="explore-controls">
        <label className="field-check">
          <input type="checkbox" checked={ellipseOn} onChange={(e) => setEllipseOn(e.target.checked)} /> Data ellipse
        </label>
        {ellipseOn && (
          <label title="Data ellipses, not confidence ellipses: at 0.95 the ellipse encloses roughly 95% of a group's points.">
            Ellipse level {ellipseLevel.toFixed(2)}
            <input
              type="range"
              aria-label="Ellipse level"
              min={0.5}
              max={0.99}
              step={0.01}
              value={ellipseLevel}
              onChange={(e) => setEllipseLevel(Number(e.target.value))}
            />
          </label>
        )}
        <label className="field-check">
          <input type="checkbox" checked={useSymbols} onChange={(e) => setUseSymbols(e.target.checked)} /> Symbols
        </label>
        {useSymbols && (
          <label>
            Symbol field
            <select value={symbolColumn} onChange={(e) => setSymbolChoice(e.target.value)}>
              {combined.descriptiveColumns.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="field-check">
          <input type="checkbox" checked={showLabels} onChange={(e) => setShowLabels(e.target.checked)} /> Point labels
        </label>
        {showLabels && (
          <label>
            Label column
            <select value={labelColumnName} onChange={(e) => setLabelColumn(e.target.value)}>
              <option value={combined.visibleIdColumn}>{combined.visibleIdColumn}</option>
              {[...combined.descriptiveColumns, ...combined.elementalColumns]
                .filter((c) => c !== combined.visibleIdColumn)
                .map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
            </select>
          </label>
        )}
      </div>
      {symbolLevels.length > 10 && (
        <p className="muted">
          {symbolColumn} has {symbolLevels.length} values; symbols repeat after ten.
        </p>
      )}
      {loadState.kind === 'loading' && <BusyNote>Loading groups…</BusyNote>}
      {loadState.kind === 'error' && <p role="alert">Error: {loadState.message}</p>}
      {orderedPaths.length === 0 && <p className="muted">No groups are included. Tick groups in the Data Manager, or choose them in Dataset.</p>}
      {analysisSetup.setup && combined.rows.length > 0 && (
        <p className="setup-in-use">
          Analysis setup: {setupSummary(analysisSetup.setup, combined.elementalColumns)}
          {setupInput.missing.length > 0 && ` (not in every selected group: ${setupInput.missing.join(', ')})`}
        </p>
      )}
      {transformedView.kind === 'loading' && <BusyNote>Transforming element values…</BusyNote>}
      {transformedView.kind === 'error' && <p role="alert">Could not transform element values: {transformedView.message}</p>}
      {scoreState === 'loading' && <BusyNote>Computing {source.toUpperCase()}…</BusyNote>}
      {scoreState !== null && scoreState !== 'loading' && <p role="alert">{scoreState}</p>}
      {scores.kind === 'loaded' && scores.data.note && <p className="muted">{scores.data.note}</p>}
      {plotMode === 'multiplot' && combined.rows.length > 0 ? (
        <Multiplot
          {...multiplotData}
          onSelect={selectUuids}
        />
      ) : sameAxis ? (
        <p className="muted">Pick two different axes to plot.</p>
      ) : (
        <VisualizeScatter
          traces={shownTraces}
          xLabel={axisLabel(xIdx, 'X')}
          yLabel={axisLabel(yIdx, 'Y')}
          dragMode="lasso"
          symbolLegend={symbolLegend}
          onSelect={selectUuids}
          onClearSelection={clearSelected}
        />
      )}
      <p className="muted" role="status">
        {selection.uuids.size} selected — double-click the plot to clear.
      </p>
      <section className="card">
      <header className="card-header"><h2 className="card-title">Assign selected samples</h2></header>
      <div className="explore-controls">
        <label>
          Target group
          <select value={assignTarget} onChange={(e) => setAssignTarget(e.target.value)}>
            <option value="">(choose existing…)</option>
            {ready.map((c) => (
              <option key={c.path} value={c.path}>
                {c.group?.group_name ?? c.path}
              </option>
            ))}
            <option value="__new__">New group…</option>
          </select>
        </label>
        {assignTarget === '__new__' && (
          <label>
            New group name
            <input
              value={newGroupName}
              onChange={(e) => setNewGroupName(e.target.value)}
              aria-label="New group name"
            />
          </label>
        )}
        <button
          type="button"
          className="btn btn-primary"
          disabled={selection.uuids.size === 0 || assignTarget === '' || assignState.kind === 'loading' || (assignTarget === '__new__' && (newGroupName.trim() === '' || newNameProblem !== null))}
          onClick={() => void assign()}
        >
          <BusyLabel busy={assignState.kind === 'loading'} busyText="Assigning…">
            Assign {selection.uuids.size} unit{selection.uuids.size === 1 ? '' : 's'}
          </BusyLabel>
        </button>
      </div>
      {assignState.kind === 'loading' && (
        <BusyNote>{assignTarget === '__new__' ? `Creating group “${newGroupName.trim()}” and moving units…` : 'Moving units into the group…'}</BusyNote>
      )}
      {assignState.kind === 'error' && <p role="alert">Assignment failed: {assignState.message}</p>}
      {newNameProblem && <p role="alert">{newNameProblem}</p>}
      {assignState.kind === 'loaded' && (
        <p role="status" className="callout callout-success">
          Moved {assignState.data} unit{assignState.data === 1 ? '' : 's'}.
        </p>
      )}
      </section>
      <TableCard
        title="Selected rows"
        fileName="Selected samples"
        note={`${selectedRows.length} selected.`}
        sheets={() => [{
          name: 'Selected samples',
          columns: [combined.visibleIdColumn, ...combined.descriptiveColumns, ...combined.elementalColumns],
          rows: selectedRows.map((r) => [r.visible_id ?? r.legacy_rowid ?? '', ...r.descriptive.map((v) => v ?? null), ...r.elemental.map((v) => (v === null ? null : v))]),
        }]}
      >
        <SelectedRowsTable
          rows={selectedRows}
          columns={{ visibleId: combined.visibleIdColumn, legacyRowid: combined.legacyRowidColumn }}
          descriptiveColumns={combined.descriptiveColumns}
          elementalColumns={combined.elementalColumns}
        />
      </TableCard>
    </section>
  );
}
