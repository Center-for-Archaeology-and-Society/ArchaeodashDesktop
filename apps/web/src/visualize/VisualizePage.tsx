/**
 * Visualize & Assign route (Section 9.4): Plotly scatter of two elemental
 * predictors with PCA variance labels, lasso/box selection keyed internally
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
  GroupRowsResponse,
  GroupsService,
  OrdinationService,
  PcaResponse,
} from '@archaeodash/client';
import { VisualizeScatter, type ScatterPoint, type ScatterTraceSpec } from './VisualizeScatter.tsx';
import { Multiplot } from './Multiplot.tsx';
import {
  clearSelection,
  colorFor,
  ellipsePoints,
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

function pcaVarianceLabels(result: PcaResponse): [string, string] {
  const pc1 = result.explained_variance[0] ?? 0;
  const pc2 = result.explained_variance[1] ?? 0;
  return [
    `${result.score_names[0] ?? 'PC1'} (${(pc1 * 100).toFixed(1)}%)`,
    `${result.score_names[1] ?? 'PC2'} (${(pc2 * 100).toFixed(1)}%)`,
  ];
}

export function VisualizePage({ deps }: { deps: VisualizeDeps }): ReactElement {
  const [candidates, setCandidates] = useState<Awaited<ReturnType<GroupsService['scan']>>>([]);
  const [selectedPath, setSelectedPath] = useState('');
  const [data, setData] = useState<GroupRowsResponse | null>(null);
  const [loadState, setLoadState] = useState<AsyncState<GroupRowsResponse>>({ kind: 'idle' });
  const [pca, setPca] = useState<AsyncState<PcaResponse>>({ kind: 'idle' });
  const [xIdx, setXIdx] = useState(0);
  const [yIdx, setYIdx] = useState(1);
  const [usePca, setUsePca] = useState(false);
  const [plotMode, setPlotMode] = useState<'single' | 'multiplot'>('single');
  const [filterColumn, setFilterColumn] = useState('');
  const [filterValue, setFilterValue] = useState('');
  const [ellipseLevel, setEllipseLevel] = useState('off');
  const [symbolColumn, setSymbolColumn] = useState('');
  const [labelMode, setLabelMode] = useState<LabelMode>('sampleId');
  const [selection, setSelection] = useState<Selection>(clearSelection({ uuids: new Set() }));
  const [assignTarget, setAssignTarget] = useState('');
  const [newGroupName, setNewGroupName] = useState('');
  const [assignState, setAssignState] = useState<AsyncState<null>>({ kind: 'idle' });

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
        const first = list.find((c) => c.ready && c.group)?.path ?? '';
        setSelectedPath((current) => current || first);
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
    setLoadState({ kind: 'loading' });
    setSelection(clearSelection({ uuids: new Set() }));
    deps.groups
      .rows(selectedPath)
      .then((rows) => {
        if (cancelled) return;
        setData(rows);
        setLoadState({ kind: 'loaded', data: rows });
      })
      .catch((err: unknown) => {
        if (!cancelled) setLoadState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [deps, selectedPath]);

  // PCA scores back the "PCA coordinates" X/Y mode (variance labels per §9.4).
  useEffect(() => {
    if (!selectedPath || !usePca || data === null) return;
    let cancelled = false;
    setPca({ kind: 'loading' });
    deps.ordination
      .pca({ path: selectedPath, columns: data.elemental_columns })
      .then((r) => {
        if (!cancelled) setPca({ kind: 'loaded', data: r });
      })
      .catch((err: unknown) => {
        if (!cancelled) setPca({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [deps, selectedPath, usePca, data]);

  // Indices into data.rows that survive the metadata filter; PCA scores are
  // row-aligned with data.rows, so filtering by index covers both modes.
  const filteredIdx = useMemo(() => {
    if (!data) return [] as number[];
    const all = data.rows.map((_, i) => i);
    if (filterColumn === '' || filterValue === '') return all;
    const colIdx = data.descriptive_columns.indexOf(filterColumn);
    if (colIdx < 0) return all;
    return data.rows
      .map((r, i) => [r, i] as const)
      .filter(([r]) => rowPassesFilter(r, colIdx, filterValue))
      .map(([, i]) => i);
  }, [data, filterColumn, filterValue]);

  const filterValues = useMemo(() => {
    if (!data || filterColumn === '') return [];
    const colIdx = data.descriptive_columns.indexOf(filterColumn);
    if (colIdx < 0) return [];
    return [...new Set(data.rows.map((r) => normalizeFilterValue(r.descriptive[colIdx])))].sort();
  }, [data, filterColumn]);

  const plotPoints = useMemo((): { x: number[]; y: number[]; uuids: string[] } => {
    if (!data) return { x: [], y: [], uuids: [] };
    if (usePca) {
      if (pca.kind !== 'loaded') return { x: [], y: [], uuids: [] };
      return {
        x: filteredIdx.map((i) => pca.data.scores[i]?.[xIdx] ?? 0),
        y: filteredIdx.map((i) => pca.data.scores[i]?.[yIdx] ?? 0),
        uuids: filteredIdx.map((i) => data.rows[i]!.analytical_uuid),
      };
    }
    return {
      x: filteredIdx.map((i) => data.rows[i]!.elemental[xIdx] ?? 0),
      y: filteredIdx.map((i) => data.rows[i]!.elemental[yIdx] ?? 0),
      uuids: filteredIdx.map((i) => data.rows[i]!.analytical_uuid),
    };
  }, [data, filteredIdx, usePca, pca, xIdx, yIdx]);

  const groupNames = useMemo(() => {
    if (!data) return [];
    const colIdx = data.descriptive_columns.indexOf(
      data.descriptive_columns.find((c) => c.toLowerCase() === 'group') ?? '',
    );
    if (colIdx < 0) return [];
    return [...new Set(data.rows.map((r) => normalizeFilterValue(r.descriptive[colIdx])))].sort();
  }, [data]);

  const traces = useMemo((): ScatterTraceSpec[] => {
    if (!data) return [];
    const groupColIdx = data.descriptive_columns.findIndex((c) => c.toLowerCase() === 'group');
    const symIdx = symbolColumn === '' ? -1 : data.descriptive_columns.indexOf(symbolColumn);
    const byGroup = new Map<string, ScatterPoint[]>();
    filteredIdx.forEach((rowIdx, i) => {
      const row = data.rows[rowIdx]!;
      const groupName = groupColIdx >= 0 ? normalizeFilterValue(row.descriptive[groupColIdx]) : 'All';
      const known = [...byGroup.keys()];
      const groupIndex = known.indexOf(groupName) >= 0 ? known.indexOf(groupName) : byGroup.size;
      const point: ScatterPoint = {
        uuid: row.analytical_uuid,
        x: plotPoints.x[i] ?? 0,
        y: plotPoints.y[i] ?? 0,
        label: labelFor(row, labelMode, rowIdx + 1),
        hover:
          row.visible_id !== null && row.visible_id !== undefined && row.visible_id !== ''
            ? `${row.visible_id} (${groupName})`
            : groupName,
        groupName,
        color: colorFor(groupIndex),
        symbol: symbolFor(symIdx >= 0 ? i : groupIndex),
      };
      const bucket = byGroup.get(groupName);
      if (bucket) bucket.push(point);
      else byGroup.set(groupName, [point]);
    });
    const level = ellipseLevel === 'off' ? null : Number(ellipseLevel);
    return [...byGroup.entries()].map(([name, points], gi) => {
      const xs = points.map((p) => p.x);
      const ys = points.map((p) => p.y);
      return {
        name,
        color: colorFor(gi),
        symbol: symbolFor(gi),
        points,
        showLabels: labelMode !== 'anid',
        ellipse: level !== null && points.length >= 3 ? ellipsePoints(xs, ys, level) : undefined,
      };
    });
  }, [data, filteredIdx, plotPoints, symbolColumn, labelMode, ellipseLevel]);

  const selectedRows = useMemo(() => {
    if (!data) return [] as GroupRowDto[];
    return data.rows.filter((r) => selection.uuids.has(r.analytical_uuid));
  }, [data, selection]);

  const axisOptions = useMemo(() => {
    if (usePca && pca.kind === 'loaded') {
      return pca.data.score_names.map((name, i) => ({
        value: String(i),
        label: `${name} (${((pca.data.explained_variance[i] ?? 0) * 100).toFixed(1)}%)`,
      }));
    }
    return (data?.elemental_columns ?? []).map((c, i) => ({ value: String(i), label: c }));
  }, [usePca, pca, data]);

  const sameAxis = xIdx === yIdx;

  const assign = useCallback(async () => {
    if (!data || selection.uuids.size === 0) return;
    setAssignState({ kind: 'loading' });
    try {
      const isNew = assignTarget === '__new__';
      // New groups land beside the source with the backend's sanitized-name
      // convention (`groups/<Name>.parquet`); existing targets use their path.
      const destinationPath = isNew
        ? selectedPath.replace(/[^/]+$/, `${newGroupName.trim().replace(/[^A-Za-z0-9._-]/g, '_') || 'group'}.parquet`)
        : assignTarget;
      await deps.groups.transferUnits({
        action: 'move',
        source_path: selectedPath,
        destination_path: destinationPath,
        destination_group_name: isNew ? newGroupName.trim() : null,
        selected_uuids: [...selection.uuids],
        expected_source_revision: data.revision_id,
      });
      setSelection(replaceSelection([]));
      setAssignState({ kind: 'loaded', data: null });
      // Reload rows to reflect the new revision.
      const fresh = await deps.groups.rows(selectedPath);
      setData(fresh);
      setLoadState({ kind: 'loaded', data: fresh });
    } catch (err: unknown) {
      setAssignState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, [deps, data, selection, assignTarget, newGroupName, selectedPath]);

  const xLabel = usePca
    ? (pca.kind === 'loaded' ? pcaVarianceLabels(pca.data)[0] : 'PC1')
    : (data?.elemental_columns[xIdx] ?? 'X');
  const yLabel = usePca
    ? (pca.kind === 'loaded' ? pcaVarianceLabels(pca.data)[1] : 'PC2')
    : (data?.elemental_columns[yIdx] ?? 'Y');

  return (
    <section aria-labelledby="visualize-heading">
      <h1 id="visualize-heading">Visualize &amp; Assign</h1>
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
        <label>
          <input type="checkbox" checked={usePca} onChange={(e) => setUsePca(e.target.checked)} /> PCA
          coordinates
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
            {(data?.descriptive_columns ?? []).map((c) => (
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
            {(data?.descriptive_columns ?? []).map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        <label>
          Labels
          <select value={labelMode} onChange={(e) => setLabelMode(e.target.value as LabelMode)}>
            <option value="anid">ANID (internal)</option>
            <option value="sampleId">Sample ID</option>
            <option value="rowNumber">Row number</option>
          </select>
        </label>
      </div>
      {loadState.kind === 'loading' && <p role="status">Loading dataset…</p>}
      {loadState.kind === 'error' && <p role="alert">Error: {loadState.message}</p>}
      <div className="explore-controls">
        <label>
          Plot mode
          <select value={plotMode} onChange={(e) => setPlotMode(e.target.value as 'single' | 'multiplot')}>
            <option value="single">Single plot</option>
            <option value="multiplot">Multiplot (all pairs)</option>
          </select>
        </label>
      </div>
      {plotMode === 'multiplot' && data ? (
        <Multiplot
          columns={data.elemental_columns}
          values={data.rows.map((r) => r.elemental)}
          groupLabels={data.rows.map((r) => {
            const gi = data.descriptive_columns.findIndex((c) => c.toLowerCase() === 'group');
            return gi >= 0 ? normalizeFilterValue(r.descriptive[gi]) : 'All';
          })}
          groupNames={[...new Set(data.rows.map((r) => {
            const gi = data.descriptive_columns.findIndex((c) => c.toLowerCase() === 'group');
            return gi >= 0 ? normalizeFilterValue(r.descriptive[gi]) : 'All';
          }))]}
          rowIndices={filteredIdx}
          rowUuids={data.rows.map((r) => r.analytical_uuid)}
          onSelect={(uuids) => setSelection(replaceSelection(uuids))}
        />
      ) : sameAxis ? (
        <p className="muted">Pick two different axes to plot.</p>
      ) : (
        <VisualizeScatter
          traces={traces}
          xLabel={xLabel}
          yLabel={yLabel}
          dragMode="lasso"
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
            {ready
              .filter((c) => c.path !== selectedPath)
              .map((c) => (
                <option key={c.path} value={c.path}>
                  {c.group?.group_name} ({c.path})
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
      {data && (
        <SelectedRowsTable
          rows={selectedRows}
          columns={{ visibleId: data.visible_id_column, legacyRowid: data.legacy_rowid_column }}
          descriptiveColumns={data.descriptive_columns}
          elementalColumns={data.elemental_columns}
        />
      )}
    </section>
  );
}
