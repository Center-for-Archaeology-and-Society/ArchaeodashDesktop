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
import { useCallback, useEffect, useMemo, useState, type ReactElement } from 'react';
import type {
  ExportsService,
  GroupRowDto,
  GroupsService,
  OrdinationService,
} from '@archaeodash/client';
import { notifyGroupsChanged } from '../data-manager/events.ts';
import { selectionBySource } from './combine.ts';
import { GroupMultiSelect } from '../datasets/GroupMultiSelect.tsx';
import { useGroupFiles } from '../datasets/useGroupFiles.ts';

export { GroupMultiSelect };
import { VisualizeScatter, type ScatterPoint, type ScatterTraceSpec } from './VisualizeScatter.tsx';
import { Multiplot } from './Multiplot.tsx';
import {
  clearSelection,
  ellipsePoints,
  groupColors,
  labelFor,
  normalizeFilterValue,
  replaceSelection,
  rowPassesFilter,
  symbolFor,
  type LabelMode,
  type Selection,
} from './visualize-model.ts';

export interface VisualizeDeps {
  readonly groups: GroupsService;
  readonly ordination: OrdinationService;
  readonly exports: ExportsService;
}

type AsyncState<T> =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'loaded'; data: T };

const ELLIPSE_LEVELS: readonly string[] = ['off', '0.50', '0.90', '0.95', '0.99'] as const;

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

/** Ordination scores aligned with the combined rows. */
interface Scores {
  readonly names: string[];
  readonly labels: string[];
  readonly values: number[][];
  readonly note?: string;
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
  const [filterValue, setFilterValue] = useState('');
  const [ellipseLevel, setEllipseLevel] = useState('off');
  const [symbolColumn, setSymbolColumn] = useState('');
  const [labelMode, setLabelMode] = useState<LabelMode>('anid');
  const [selection, setSelection] = useState<Selection>(clearSelection({ uuids: new Set() }));
  const [assignTarget, setAssignTarget] = useState('');
  const [newGroupName, setNewGroupName] = useState('');
  const [assignState, setAssignState] = useState<AsyncState<null>>({ kind: 'idle' });

  const dataset = useGroupFiles(deps.groups);
  const { ready, options: groupOptions, selectedPaths, setSelectedPaths, orderedPaths, files, combined, rescan } = dataset;
  const loadState = dataset.load;

  // A new set of rows invalidates the current selection.
  useEffect(() => {
    setSelection(clearSelection({ uuids: new Set() }));
  }, [files]);
  const colors = useMemo(() => groupColors(combined.groupNames.length), [combined]);
  const colorOf = useCallback(
    (group: string) => colors[combined.groupNames.indexOf(group)] ?? '#888',
    [colors, combined],
  );

  // Ordination scores over all loaded groups (one request, rows in file order).
  useEffect(() => {
    if (source === 'elements' || files.length === 0) {
      setScores({ kind: 'idle' });
      return;
    }
    const columns = combined.elementalColumns;
    const [first, ...rest] = files.map((f) => f.path);
    if (!first || columns.length < 2) {
      setScores({ kind: 'error', message: 'Ordination needs at least two shared measured columns.' });
      return;
    }
    let cancelled = false;
    setScores({ kind: 'loading' });
    const base = { path: first, extra_paths: rest, columns };
    const run = async (): Promise<Scores> => {
      if (source === 'pca') {
        const r = await deps.ordination.pca(base);
        return {
          names: r.score_names,
          labels: r.score_names.map(
            (n, i) => `${n} (${((r.explained_variance[i] ?? 0) * 100).toFixed(1)}%)`,
          ),
          values: r.scores,
        };
      }
      if (source === 'umap') {
        const r = await deps.ordination.umap({ ...base, seed: 20260914 });
        return {
          names: r.score_names,
          labels: r.score_names.map((n) => `UMAP ${n}`),
          values: r.embedding,
          note: r.warnings.join('; ') || undefined,
        };
      }
      if (files.length < 3) {
        throw new Error('LDA needs at least three groups; select more groups in Dataset.');
      }
      const r = await deps.ordination.lda({ ...base, group_column: '', group_by_file: true });
      return {
        names: r.score_names,
        labels: r.score_names,
        values: r.scores,
        note: r.warnings.join('; ') || undefined,
      };
    };
    run()
      .then((s) => {
        if (!cancelled) setScores({ kind: 'loaded', data: s });
      })
      .catch((err: unknown) => {
        if (!cancelled) setScores({ kind: 'error', message: errorText(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [deps, source, files, combined]);

  // Reset axes to the first two dimensions whenever the axis source changes.
  useEffect(() => {
    setXIdx(0);
    setYIdx(1);
  }, [source]);

  const filteredIdx = useMemo(() => {
    const all = combined.rows.map((_, i) => i);
    if (filterColumn === '' || filterValue === '') return all;
    const colIdx = combined.descriptiveColumns.indexOf(filterColumn);
    if (colIdx < 0) return all;
    return all.filter((i) => rowPassesFilter(combined.rows[i]!, colIdx, filterValue));
  }, [combined, filterColumn, filterValue]);

  const filterValues = useMemo(() => {
    const colIdx = combined.descriptiveColumns.indexOf(filterColumn);
    if (colIdx < 0) return [];
    return [...new Set(combined.rows.map((r) => normalizeFilterValue(r.descriptive[colIdx])))].sort();
  }, [combined, filterColumn]);

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
      if (source === 'elements') return combined.rows[rowIdx]?.elemental[axis] ?? null;
      if (scores.kind !== 'loaded') return null;
      const v = scores.data.values[rowIdx]?.[axis];
      return v === undefined || !Number.isFinite(v) ? null : v;
    },
    [source, combined, scores],
  );

  const traces = useMemo((): ScatterTraceSpec[] => {
    const symIdx = combined.descriptiveColumns.indexOf(symbolColumn);
    const byGroup = new Map<string, ScatterPoint[]>(combined.groupNames.map((g) => [g, []]));
    for (const rowIdx of filteredIdx) {
      const x = coordinate(rowIdx, xIdx);
      const y = coordinate(rowIdx, yIdx);
      if (x === null || y === null) continue;
      const row = combined.rows[rowIdx]!;
      const groupName = combined.groupOf[rowIdx]!;
      const symbol =
        symIdx >= 0
          ? symbolFor(symbolLevels.indexOf(normalizeFilterValue(row.descriptive[symIdx])))
          : symbolFor(0);
      byGroup.get(groupName)?.push({
        uuid: row.analytical_uuid,
        x,
        y,
        label: labelFor(row, labelMode, rowIdx + 1),
        hover:
          row.visible_id !== null && row.visible_id !== undefined && row.visible_id !== ''
            ? `${row.visible_id} (${groupName})`
            : groupName,
        groupName,
        color: colorOf(groupName),
        symbol,
      });
    }
    const level = ellipseLevel === 'off' ? null : Number(ellipseLevel);
    return [...byGroup.entries()]
      .filter(([, points]) => points.length > 0)
      .map(([name, points]) => ({
        name,
        color: colorOf(name),
        symbol: symbolFor(0),
        points,
        showLabels: labelMode !== 'anid',
        ellipse:
          level !== null && points.length >= 3
            ? ellipsePoints(points.map((p) => p.x), points.map((p) => p.y), level)
            : undefined,
      }));
  }, [combined, filteredIdx, coordinate, xIdx, yIdx, symbolColumn, symbolLevels, labelMode, ellipseLevel, colorOf]);

  const selectedRows = useMemo(
    () => combined.rows.filter((r) => selection.uuids.has(r.analytical_uuid)),
    [combined, selection],
  );

  const axisOptions = useMemo(() => {
    if (source === 'elements') {
      return combined.elementalColumns.map((c, i) => ({ value: String(i), label: c }));
    }
    if (scores.kind !== 'loaded') return [];
    return scores.data.labels.map((label, i) => ({ value: String(i), label }));
  }, [source, combined, scores]);

  const axisLabel = (idx: number, fallback: string) =>
    axisOptions[idx]?.label ?? fallback;

  const sameAxis = xIdx === yIdx;

  const assign = useCallback(async () => {
    if (selection.uuids.size === 0) return;
    setAssignState({ kind: 'loading' });
    try {
      const isNew = assignTarget === '__new__';
      const name = newGroupName.trim();
      // New groups land beside the sources with the backend's sanitized-name
      // convention (`groups/<Name>.parquet`); existing targets use their path.
      const firstPath = orderedPaths[0] ?? 'groups/group.parquet';
      const destinationPath = isNew
        ? firstPath.replace(/[^/]+$/, `${name.replace(/[^A-Za-z0-9._-]/g, '_') || 'group'}.parquet`)
        : assignTarget;
      let created = false;
      for (const [sourcePath, uuids] of selectionBySource(combined, selection.uuids)) {
        if (sourcePath === destinationPath) continue;
        const sourceFile = files.find((f) => f.path === sourcePath);
        if (!sourceFile) continue;
        await deps.groups.transferUnits({
          action: 'move',
          source_path: sourcePath,
          destination_path: destinationPath,
          destination_group_name: isNew && !created ? name : null,
          selected_uuids: uuids,
          expected_source_revision: sourceFile.data.revision_id,
        });
        created = true;
      }
      setSelection(replaceSelection([]));
      setAssignState({ kind: 'loaded', data: null });
      // Rescan (a new group may exist, an emptied one may be gone) and keep
      // the current selection plus the destination group.
      await rescan((all) =>
        all.filter((p) => p === destinationPath || selectedPaths.includes(p)),
      );
      dataset.reload();
      notifyGroupsChanged();
    } catch (err: unknown) {
      setAssignState({ kind: 'error', message: errorText(err) });
    }
  }, [deps, combined, files, selection, assignTarget, newGroupName, orderedPaths, selectedPaths, rescan, dataset]);

  const scoreState =
    source === 'elements' ? null : scores.kind === 'loading' ? 'loading' : scores.kind === 'error' ? scores.message : null;

  return (
    <section aria-labelledby="visualize-heading">
      <h1 id="visualize-heading">Visualize &amp; Assign</h1>
      <div className="explore-controls">
        <label>
          Dataset
          <GroupMultiSelect options={groupOptions} selected={selectedPaths} onChange={setSelectedPaths} />
        </label>
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
          <select value={filterColumn} onChange={(e) => { setFilterColumn(e.target.value); setFilterValue(''); }}>
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
            Filter value
            <select value={filterValue} onChange={(e) => setFilterValue(e.target.value)}>
              <option value="">(all)</option>
              {filterValues.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          Ellipse
          <select value={ellipseLevel} onChange={(e) => setEllipseLevel(e.target.value)}>
            {ELLIPSE_LEVELS.map((l) => (
              <option key={l} value={l}>
                {l === 'off' ? 'off' : `${Math.round(Number(l) * 100)}%`}
              </option>
            ))}
          </select>
        </label>
        <label>
          Symbols by
          <select value={symbolColumn} onChange={(e) => setSymbolColumn(e.target.value)}>
            <option value="">(none)</option>
            {combined.descriptiveColumns.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        <label>
          Labels
          <select value={labelMode} onChange={(e) => setLabelMode(e.target.value as LabelMode)}>
            <option value="anid">None</option>
            <option value="sampleId">Sample ID</option>
            <option value="rowNumber">Row number</option>
          </select>
        </label>
        <label>
          Plot mode
          <select value={plotMode} onChange={(e) => setPlotMode(e.target.value as 'single' | 'multiplot')}>
            <option value="single">Single plot</option>
            <option value="multiplot">Multiplot (all pairs)</option>
          </select>
        </label>
      </div>
      {symbolLevels.length > 10 && (
        <p className="muted">
          {symbolColumn} has {symbolLevels.length} values; symbols repeat after ten.
        </p>
      )}
      {loadState.kind === 'loading' && <p role="status">Loading groups…</p>}
      {loadState.kind === 'error' && <p role="alert">Error: {loadState.message}</p>}
      {orderedPaths.length === 0 && <p className="muted">Choose one or more groups in Dataset.</p>}
      {scoreState === 'loading' && <p role="status">Computing {source.toUpperCase()}…</p>}
      {scoreState !== null && scoreState !== 'loading' && <p role="alert">{scoreState}</p>}
      {scores.kind === 'loaded' && scores.data.note && <p className="muted">{scores.data.note}</p>}
      {plotMode === 'multiplot' && combined.rows.length > 0 ? (
        <Multiplot
          columns={combined.elementalColumns}
          values={transposeElementalRows(combined.rows, combined.elementalColumns.length)}
          groupLabels={combined.groupOf}
          groupNames={combined.groupNames}
          rowIndices={filteredIdx}
          rowUuids={combined.rows.map((r) => r.analytical_uuid)}
          onSelect={(uuids) => setSelection(replaceSelection(uuids))}
        />
      ) : sameAxis ? (
        <p className="muted">Pick two different axes to plot.</p>
      ) : (
        <VisualizeScatter
          traces={traces}
          xLabel={axisLabel(xIdx, 'X')}
          yLabel={axisLabel(yIdx, 'Y')}
          dragMode="lasso"
          symbolLegend={symbolLegend}
          onSelect={(uuids) => setSelection(replaceSelection(uuids))}
          onClearSelection={() => setSelection(clearSelection(selection))}
        />
      )}
      <p className="muted" role="status">
        {selection.uuids.size} selected — double-click the plot to clear.
      </p>
      <h2>Assign selected units</h2>
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
          disabled={selection.uuids.size === 0 || assignTarget === '' || assignState.kind === 'loading' || (assignTarget === '__new__' && newGroupName.trim() === '')}
          onClick={() => void assign()}
        >
          Assign {selection.uuids.size} unit{selection.uuids.size === 1 ? '' : 's'}
        </button>
      </div>
      {assignState.kind === 'error' && <p role="alert">Assignment failed: {assignState.message}</p>}
      {assignState.kind === 'loaded' && <p role="status">Assignment committed.</p>}
      <h2>Selected rows</h2>
      <SelectedRowsTable
        rows={selectedRows}
        columns={{ visibleId: combined.visibleIdColumn, legacyRowid: combined.legacyRowidColumn }}
        descriptiveColumns={combined.descriptiveColumns}
        elementalColumns={combined.elementalColumns}
      />
    </section>
  );
}
