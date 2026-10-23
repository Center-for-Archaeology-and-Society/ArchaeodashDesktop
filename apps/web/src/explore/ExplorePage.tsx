/**
 * Explore route (Section 9.4): dataset table with elemental columns locked,
 * plus the missing-value, histogram, crosstab, and compositional-profile
 * views over one loaded group file. Legacy tab set: Dataset, Crosstabs,
 * Univariate Plots, Compositional Profile Plot.
 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactElement,
} from 'react';
import type {
  ExploreCrosstabResponse,
  ExploreHistogramResponse,
  ExploreMissingProfileResponse,
  ExploreCompositionalProfileResponse,
  ExploreService,
  ExportsService,
  GroupRowsResponse,
  GroupsService,
} from '@archaeodash/client';
import type { Data } from 'plotly.js-dist-min';
import { downloadExportResult } from '../exports.ts';
import { PlotlyChart } from '../charts/PlotlyChart.tsx';
import { groupColors } from '../visualize/visualize-model.ts';
import {
  asRowsResponse,
  combineGroupFiles,
  defaultGroupColumn,
  inputPaths,
  type LoadedGroupFile,
} from '../visualize/combine.ts';
import { GroupMultiSelect } from '../datasets/GroupMultiSelect.tsx';
import { useGroupFiles } from '../datasets/useGroupFiles.ts';
import { HiddenIdNote } from './HiddenIdNote.tsx';
import { CellAutosave, cellKey } from './autosave.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { useCachedResult, useDataCache } from '../datasets/DataCache.tsx';
import { analysisInput } from '../analysis-setup/setup.ts';
export interface ExploreDeps {
  readonly groups: GroupsService;
  readonly explore: ExploreService;
  readonly exports: ExportsService;
}

type AsyncState<T> =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'loaded'; data: T };

type ExploreView = 'table' | 'missing' | 'histogram' | 'crosstab' | 'profile';

const VIEWS: readonly [ExploreView, string][] = [
  ['table', 'Dataset'],
  ['missing', 'Missing values'],
  ['histogram', 'Univariate Plots'],
  ['crosstab', 'Crosstabs'],
  ['profile', 'Compositional Profile Plot'],
];

/**
 * Dataset table over the selected groups (descriptive edits only). Edits
 * autosave when a cell loses focus (or on Enter); Escape reverts the cell.
 * Saves go to each row's own group file through `CellAutosave`, which also
 * provides undo/redo. The synthetic group column, and columns a row's file
 * does not have, are read-only.
 */
/** Tables longer than this render only the rows in view. */
export const VIRTUALIZE_FROM = 200;
const DEFAULT_ROW_HEIGHT = 34;
const OVERSCAN_ROWS = 15;

export function DataTable({
  files,
  deps,
  autosave,
  onReload,
}: {
  files: readonly LoadedGroupFile[];
  deps: ExploreDeps;
  autosave: CellAutosave;
  /** Reload the groups from disk (after a conflict). */
  onReload: () => void;
}): ReactElement {
  const combined = useMemo(() => combineGroupFiles(files), [files]);
  // Large tables render only the rows in view (plus a margin), so scrolling
  // and typing stay fast however many rows are loaded.
  const total = combined.rows.length;
  const virtual = total > VIRTUALIZE_FROM;
  const scrollRef = useRef<HTMLDivElement>(null);
  const [rowHeight, setRowHeight] = useState(DEFAULT_ROW_HEIGHT);
  const [windowStart, setWindowStart] = useState(0);
  const [windowSize, setWindowSize] = useState(60);
  const updateWindow = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    setWindowStart(Math.max(0, Math.floor(el.scrollTop / rowHeight) - OVERSCAN_ROWS));
    setWindowSize(Math.ceil(el.clientHeight / rowHeight) + 2 * OVERSCAN_ROWS);
  }, [rowHeight]);
  useLayoutEffect(() => {
    if (!virtual) return;
    // Measure the real row height once rows are on screen.
    const row = scrollRef.current?.querySelector<HTMLTableRowElement>('tbody tr[data-row]');
    if (row && row.offsetHeight > 0 && Math.abs(row.offsetHeight - rowHeight) > 0.5) setRowHeight(row.offsetHeight);
    updateWindow();
  }, [virtual, rowHeight, updateWindow]);
  const first = virtual ? Math.min(windowStart, Math.max(0, total - windowSize)) : 0;
  const last = virtual ? Math.min(total, first + windowSize) : total;
  const visibleRows = virtual ? combined.rows.slice(first, last) : combined.rows;
  const columnCount = 2 + combined.descriptiveColumns.length + combined.elementalColumns.length;
  // Drafts key on uuid+NUL+column (the UUID itself is never rendered).
  const [drafts, setDrafts] = useState<Map<string, string>>(new Map());
  const [, setTick] = useState(0);
  const [exportError, setExportError] = useState('');
  const fileOf = useMemo(() => new Map(files.map((f) => [f.path, f])), [files]);

  useEffect(() => autosave.onChange(() => setTick((t) => t + 1)), [autosave]);

  const cellRef = useRef(new Map<string, { path: string; uuid: string; column: string; label: string }>());
  const savedValue = useCallback(
    (key: string, original: string | null): string | null =>
      autosave.saved.has(key) ? (autosave.saved.get(key) ?? null) : original,
    [autosave],
  );

  const commitKey = useCallback(
    (key: string, draft: string, original: string | null) => {
      const cell = cellRef.current.get(key);
      setDrafts((prev) => {
        if (!prev.has(key)) return prev;
        const next = new Map(prev);
        next.delete(key);
        return next;
      });
      if (!cell) return;
      autosave.commit({ ...cell, before: savedValue(key, original), after: draft });
    },
    [autosave, savedValue],
  );

  // Commit unsaved drafts when the table goes away (tab switch, page change)
  // or the window is hidden/closed; blur does not fire reliably then.
  const draftsRef = useRef(drafts);
  draftsRef.current = drafts;
  const originals = useRef(new Map<string, string | null>());
  useEffect(() => {
    const flushDrafts = () => {
      for (const [key, draft] of draftsRef.current) {
        const cell = cellRef.current.get(key);
        if (cell) autosave.commit({ ...cell, before: savedValue(key, originals.current.get(key) ?? null), after: draft });
      }
      draftsRef.current = new Map();
    };
    window.addEventListener('pagehide', flushDrafts);
    return () => {
      window.removeEventListener('pagehide', flushDrafts);
      flushDrafts();
    };
  }, [autosave, savedValue]);

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const mod = event.ctrlKey || event.metaKey;
    if (!mod) return;
    const key = event.key.toLowerCase();
    const target = event.target as HTMLElement;
    // While a cell has unsaved typing, leave Ctrl+Z to the text field.
    const editing = target instanceof HTMLInputElement && target.dataset.dirty === 'true';
    if (editing) return;
    if (key === 'z' && !event.shiftKey) {
      event.preventDefault();
      autosave.undo();
    } else if ((key === 'z' && event.shiftKey) || key === 'y') {
      event.preventDefault();
      autosave.redo();
    }
  };

  const nextUndo = autosave.undoStack.at(-1);
  const nextRedo = autosave.redoStack.at(-1);
  const status = autosave.saving
    ? 'Saving…'
    : autosave.failedCount > 0
      ? `${autosave.failedCount} edit${autosave.failedCount === 1 ? '' : 's'} not saved`
      : 'All changes saved';

  return (
    <div onKeyDown={onKeyDown}>
      <HiddenIdNote />
      <div className="table-actions autosave-bar">
        <button
          type="button"
          disabled={!nextUndo}
          title={nextUndo ? `Undo ${nextUndo.label} (Ctrl+Z)` : 'Nothing to undo'}
          onClick={() => autosave.undo()}
        >
          Undo
        </button>
        <button
          type="button"
          disabled={!nextRedo}
          title={nextRedo ? `Redo ${nextRedo.label} (Ctrl+Shift+Z)` : 'Nothing to redo'}
          onClick={() => autosave.redo()}
        >
          Redo
        </button>
        <span role="status" className={autosave.failedCount > 0 ? 'autosave-status error' : 'autosave-status'}>
          {status}
        </span>
        {autosave.failedCount > 0 && autosave.conflicts.size === 0 && (
          <button type="button" onClick={() => autosave.retry()}>
            Retry
          </button>
        )}
        {autosave.conflicts.size > 0 && (
          <button type="button" onClick={onReload}>
            Reload changed groups
          </button>
        )}
        <button
          type="button"
          onClick={() =>
            void autosave
              .idle()
              .then(() => deps.exports.measuredData({ ...inputPaths(files), raw_text: false }))
              .then(downloadExportResult)
              .catch((err: unknown) => setExportError(err instanceof Error ? err.message : String(err)))
          }
        >
          Export measured data (CSV)
        </button>
      </div>
      {autosave.error && autosave.failedCount > 0 && (
        <p role="alert">
          Save failed: {autosave.error}
          {autosave.conflicts.size > 0 && ' Reload to see the current values; unsaved edits are lost.'}
        </p>
      )}
      {exportError && <p role="alert">Export failed: {exportError}</p>}
      {virtual && (
        <p className="table-window-note">
          {`${total.toLocaleString()} rows; scroll the table to see more (rows ${(first + 1).toLocaleString()}–${last.toLocaleString()} are drawn).`}
        </p>
      )}
      <div
        className={virtual ? 'table-scroll is-virtual' : 'table-scroll'}
        ref={scrollRef}
        onScroll={virtual ? updateWindow : undefined}
      >
        <table className="data-table" aria-rowcount={total + 1}>
          <thead>
            <tr>
              <th>{combined.visibleIdColumn}</th>
              <th>{combined.legacyRowidColumn}</th>
              {combined.descriptiveColumns.map((c) => (
                <th key={c}>{c}</th>
              ))}
              {combined.elementalColumns.map((c) => (
                <th key={c} className="locked-col" title="Measured elemental values are read-only">
                  {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {virtual && first > 0 && (
              <tr aria-hidden="true" className="table-spacer" style={{ height: first * rowHeight }}>
                <td colSpan={columnCount} />
              </tr>
            )}
            {visibleRows.map((row, offset) => {
              const rowIndex = first + offset;
              const path = combined.pathOf[rowIndex]!;
              const own = fileOf.get(path)?.data.descriptive_columns ?? [];
              const rowLabel = row.visible_id ?? `row ${rowIndex + 1}`;
              return (
                <tr key={row.analytical_uuid} data-row aria-rowindex={rowIndex + 2}>
                  <td>{row.visible_id ?? ''}</td>
                  <td>{row.legacy_rowid ?? ''}</td>
                  {row.descriptive.map((value, i) => {
                    const column = combined.descriptiveColumns[i] ?? '';
                    if (column === combined.groupColumn || !own.includes(column)) {
                      return (
                        <td key={column} className="locked-col">
                          {value ?? ''}
                        </td>
                      );
                    }
                    const key = cellKey(row.analytical_uuid, column);
                    cellRef.current.set(key, {
                      path,
                      uuid: row.analytical_uuid,
                      column,
                      label: `${column} for ${rowLabel}`,
                    });
                    originals.current.set(key, value ?? null);
                    const draft = drafts.get(key);
                    const cellStatus = autosave.status.get(key);
                    return (
                      <td key={column} className={cellStatus ? `cell-${cellStatus}` : undefined}>
                        <input
                          data-dirty={draft !== undefined ? 'true' : undefined}
                          value={draft ?? savedValue(key, value ?? null) ?? ''}
                          aria-label={`${column} for ${rowLabel}`}
                          aria-invalid={cellStatus === 'error' ? true : undefined}
                          title={cellStatus === 'error' ? 'Not saved' : undefined}
                          onChange={(e) => {
                            const text = e.target.value;
                            setDrafts((prev) => new Map(prev).set(key, text));
                          }}
                          onBlur={() => {
                            if (draft !== undefined) commitKey(key, draft, value ?? null);
                          }}
                          onKeyDown={(e) => {
                            if (e.key === 'Enter' && draft !== undefined) {
                              e.preventDefault();
                              commitKey(key, draft, value ?? null);
                            } else if (e.key === 'Escape' && draft !== undefined) {
                              e.preventDefault();
                              setDrafts((prev) => {
                                const next = new Map(prev);
                                next.delete(key);
                                return next;
                              });
                            }
                          }}
                        />
                      </td>
                    );
                  })}
                  {row.elemental.map((v, i) => (
                    <td key={combined.elementalColumns[i] ?? i} className="locked-col">
                      {v === null ? 'NA' : v}
                    </td>
                  ))}
                </tr>
              );
            })}
            {virtual && last < total && (
              <tr aria-hidden="true" className="table-spacer" style={{ height: (total - last) * rowHeight }}>
                <td colSpan={columnCount} />
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

/** Request input over the selected group files. */
type InputPaths = { path: string; extra_paths: string[] };

/** Missing-value bands (Good ≤5% / OK ≤40% / Bad ≤80% / Remove). */
/** Cache key for an Explore result: view, file revisions, and request. */
export function exploreKey(kind: string, revKey: string, request: unknown): string {
  return `explore:${kind}:${revKey}:${JSON.stringify(request)}`;
}

function MissingView({
  deps,
  input,
  columns,
  revKey,
}: {
  deps: ExploreDeps;
  input: InputPaths;
  columns: string[];
  /** Revisions of the loaded files; part of the cache key. */
  revKey: string;
}): ReactElement {
  const request = useMemo(() => ({ ...input, columns, transformation: null }), [input, columns]);
  const state: AsyncState<ExploreMissingProfileResponse> = useCachedResult(
    exploreKey('missing', revKey, request),
    () => deps.explore.missingProfile(request),
  );

  if (state.kind === 'loading') return <p role="status">Computing missing profile…</p>;
  if (state.kind === 'error') return <p role="alert">Error: {state.message}</p>;
  if (state.kind !== 'loaded') return <p>Choose a dataset first.</p>;
  return (
    <table className="data-table">
      <thead>
        <tr>
          <th>Feature</th>
          <th>Missing</th>
          <th>%</th>
          <th>Band</th>
        </tr>
      </thead>
      <tbody>
        {state.data.rows.map((row) => (
          <tr key={row.feature} data-band={row.band}>
            <td>{row.feature}</td>
            <td>{row.num_missing}</td>
            <td>{row.pct_missing.toFixed(1)}</td>
            <td>{row.band}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Histogram with the legacy bins 2–100, default 30. */
function HistogramView({
  deps,
  input,
  columns,
  revKey,
}: {
  deps: ExploreDeps;
  input: InputPaths;
  columns: string[];
  revKey: string;
}): ReactElement {
  const [column, setColumn] = useState(columns[0] ?? '');
  const [bins, setBins] = useState(30);
  const request = useMemo(() => ({ ...input, column, bins, transformation: null }), [input, column, bins]);
  const state: AsyncState<ExploreHistogramResponse> = useCachedResult(
    column ? exploreKey('histogram', revKey, request) : null,
    () => deps.explore.histogram(request),
  );

  const maxCount = state.kind === 'loaded' ? Math.max(...state.data.counts, 1) : 1;
  return (
    <div>
      <label>
        Field
        <select value={column} onChange={(e) => setColumn(e.target.value)}>
          {columns.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      <label>
        Bins (2–100)
        <input
          type="number"
          min={2}
          max={100}
          value={bins}
          onChange={(e) => setBins(Math.min(100, Math.max(2, Number(e.target.value) || 30)))}
        />
      </label>
      {state.kind === 'loading' && <p role="status">Computing histogram…</p>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && (
        <div className="histogram" role="img" aria-label={`Histogram of ${column}`}>
          {state.data.counts.map((count, i) => (
            <div
              key={i}
              className="histogram-bar"
              style={{ height: `${(count / maxCount) * 100}%` }}
              title={`${state.data.breaks[i]}–${state.data.breaks[i + 1]}: ${count}`}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/** Crosstab: counts or mean/median/sd of a numeric second field by group. */
function CrosstabView({
  deps,
  input,
  data,
  defaultGroupColumn,
  revKey,
}: {
  deps: ExploreDeps;
  input: InputPaths;
  data: GroupRowsResponse;
  defaultGroupColumn: string;
  revKey: string;
}): ReactElement {
  const [groupColumn, setGroupColumn] = useState(defaultGroupColumn);
  // Default to the first other descriptive field with two or more distinct
  // non-missing values; an all-missing column makes an empty-looking table.
  const [valueColumn, setValueColumn] = useState(() => {
    const informative = data.descriptive_columns.find((c, i) => {
      if (c === defaultGroupColumn) return false;
      const seen = new Set<string>();
      for (const row of data.rows) {
        const v = row.descriptive[i];
        if (v !== null && v !== undefined && v.trim() !== '') seen.add(v);
        if (seen.size > 1) return true;
      }
      return false;
    });
    return informative ?? data.descriptive_columns[1] ?? data.elemental_columns[0] ?? '';
  });
  const [method, setMethod] = useState<'count' | 'mean' | 'median' | 'sd'>('count');
  const request = useMemo(
    () => ({ ...input, group_column: groupColumn, value_column: valueColumn, summary_method: method }),
    [input, groupColumn, valueColumn, method],
  );
  const state: AsyncState<ExploreCrosstabResponse> = useCachedResult(
    groupColumn && valueColumn ? exploreKey('crosstab', revKey, request) : null,
    () => deps.explore.crosstab(request),
  );

  return (
    <div>
      <label>
        Group field
        <select value={groupColumn} onChange={(e) => setGroupColumn(e.target.value)}>
          {data.descriptive_columns.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      <label>
        Second field
        <select value={valueColumn} onChange={(e) => setValueColumn(e.target.value)}>
          {data.descriptive_columns.concat(data.elemental_columns).map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      <label>
        Summary
        <select value={method} onChange={(e) => setMethod(e.target.value as typeof method)}>
          <option value="count">count</option>
          <option value="mean">mean</option>
          <option value="median">median</option>
          <option value="sd">sd</option>
        </select>
      </label>
      {state.kind === 'loading' && <p role="status">Computing crosstab…</p>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && state.data.kind === 'count' && (
        <CrosstabCountTable rows={state.data.rows} groupLabel={groupColumn} valueLabel={valueColumn} />
      )}
      {state.kind === 'loaded' && state.data.kind === 'summary' && (
        <table className="data-table">
          <thead>
            <tr>
              <th>{groupColumn}</th>
              <th>{state.data.result_column}</th>
            </tr>
          </thead>
          <tbody>
            {state.data.rows.map((row, i) => (
              <tr key={i}>
                <td>{row.group ?? '(Missing)'}</td>
                <td>{row.result === null ? '(Missing)' : row.result.toFixed(3)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

/**
 * Count crosstab as a long table (legacy `count(group, value)`): one row per
 * group/value combination, columns named after the chosen fields, then the
 * count. NA levels show as `(Missing)` and keep the backend's sort order.
 */
export function CrosstabCountTable({
  rows,
  groupLabel,
  valueLabel,
}: {
  rows: readonly { group: string | null; value: string | null; count: number }[];
  groupLabel: string;
  valueLabel: string;
}): ReactElement {
  const label = (v: string | null) => v ?? '(Missing)';
  return (
    <div className="table-scroll">
      <table className="data-table crosstab-table">
        <thead>
          <tr>
            <th scope="col">{groupLabel}</th>
            <th scope="col">{valueLabel}</th>
            <th scope="col">count</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i}>
              <td>{label(r.group)}</td>
              <td>{label(r.value)}</td>
              <td>{r.count}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Plotly traces for the legacy `comp.profile`: one translucent line per
 * sample across the ordered elements, colored by group (or a single red
 * series when ungrouped), on a log concentration axis. Samples of one group
 * share a trace, separated by null gaps, so the legend lists groups.
 */
export function profileTraces(
  rows: readonly { rowid: number; element: string; value: number | null; group_label: string | null }[],
  grouped: boolean,
): Data[] {
  const byGroup = new Map<string, { x: (string | null)[]; y: (number | null)[]; lastRow: number }>();
  for (const r of rows) {
    const key = grouped ? (r.group_label ?? '(Missing)') : 'All samples';
    let series = byGroup.get(key);
    if (!series) {
      series = { x: [], y: [], lastRow: r.rowid };
      byGroup.set(key, series);
    }
    if (series.lastRow !== r.rowid) {
      series.x.push(null);
      series.y.push(null);
      series.lastRow = r.rowid;
    }
    series.x.push(r.element);
    // Log axis: non-positive concentrations cannot be drawn, so they gap.
    series.y.push(r.value !== null && r.value > 0 ? r.value : null);
  }
  const names = [...byGroup.keys()].sort();
  const colors = groupColors(names.length);
  return names.map((name, i) => {
    const series = byGroup.get(name)!;
    return {
      // SVG, not WebGL: WebKitGTK webviews often lack WebGL.
      type: 'scatter',
      mode: 'lines',
      name,
      x: series.x,
      y: series.y,
      connectgaps: false,
      opacity: grouped ? 0.45 : 0.25,
      line: { color: grouped ? colors[i] : '#d62728', width: 1 },
      hovertemplate: `${name}<br>%{x}: %{y}<extra></extra>`,
    } as Data;
  });
}

/** Compositional profile: one line per sample across ordered elements. */
function ProfileView({
  deps,
  input,
  columns,
  groupColumns,
  defaultGroupColumn,
  revKey,
}: {
  deps: ExploreDeps;
  input: InputPaths;
  columns: string[];
  groupColumns: string[];
  defaultGroupColumn: string;
  revKey: string;
}): ReactElement {
  const [groupColumn, setGroupColumn] = useState(defaultGroupColumn);
  const [logScale, setLogScale] = useState(true);
  const request = useMemo(
    () => ({ ...input, columns, group_column: groupColumn || null, transformation: null }),
    [input, columns, groupColumn],
  );
  const state: AsyncState<ExploreCompositionalProfileResponse> = useCachedResult(
    exploreKey('profile', revKey, request),
    () => deps.explore.compositionalProfile(request),
  );

  const traces = useMemo(
    () => (state.kind === 'loaded' ? profileTraces(state.data.rows, groupColumn !== '') : []),
    [state, groupColumn],
  );
  const layout = useMemo(
    () => ({
      xaxis: { title: { text: '' }, categoryorder: 'array' as const, categoryarray: columns },
      yaxis: { title: { text: 'Concentration (ppm)' }, type: logScale ? ('log' as const) : ('linear' as const) },
      showlegend: groupColumn !== '',
      hovermode: 'closest' as const,
    }),
    [columns, logScale, groupColumn],
  );

  return (
    <div>
      <div className="explore-controls">
        <label>
          Color by
          <select value={groupColumn} onChange={(e) => setGroupColumn(e.target.value)}>
            <option value="">(none)</option>
            {groupColumns.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        <label>
          <input type="checkbox" checked={logScale} onChange={(e) => setLogScale(e.target.checked)} /> Log scale
        </label>
      </div>
      {state.kind === 'loading' && <p role="status">Computing profile…</p>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && traces.length > 0 && (
        <PlotlyChart data={traces} layout={layout} ariaLabel="Compositional profile" height={480} />
      )}
      <p className="muted">One line per sample across the measured elements.</p>
    </div>
  );
}

export function ExplorePage({ deps }: { deps: ExploreDeps }): ReactElement {
  const dataset = useGroupFiles(deps.groups);
  const { files, combined, load } = dataset;
  const [view, setView] = useState<ExploreView>('table');

  const data = useMemo(() => asRowsResponse(files, combined), [files, combined]);
  // Plots use the Analysis setup's elements; values stay as measured here.
  const { setup } = useAnalysisSetup();
  const plotColumns = useMemo(() => analysisInput(setup, combined.elementalColumns).columns, [setup, combined.elementalColumns]);
  // One autosave (and undo history) per loaded set of files, kept while you
  // switch Explore tabs. Saves update revisions in place, without reloading.
  const autosave = useMemo(
    () =>
      new CellAutosave(
        (request) => deps.groups.patchDescriptiveValues(request),
        new Map(files.map((f) => [f.path, f.data.revision_id])),
      ),
    [deps, files],
  );
  // Views that read from disk wait for pending saves first.
  const [savesSettled, setSavesSettled] = useState(true);
  useEffect(() => {
    setSavesSettled(!autosave.saving);
    return autosave.onChange(() => setSavesSettled(!autosave.saving));
  }, [autosave]);
  const input = useMemo(() => inputPaths(files), [files]);
  // Remount views when the selection or a file revision changes.
  const viewKey = files.map((f) => `${f.path}@${f.data.revision_id}`).join('|');

  // Compute the missing-value profile as soon as the data is in, so its tab
  // opens instantly (the result is cached with the file revisions).
  const cache = useDataCache();
  // Saved table edits change revisions, so results recompute after them.
  const revKey = autosave.revisionsKey;
  useEffect(() => {
    if (!cache || load.kind !== 'loaded' || files.length === 0 || !savesSettled) return;
    const request = { ...input, columns: plotColumns, transformation: null };
    void cache.result(exploreKey('missing', revKey, request), () => deps.explore.missingProfile(request)).catch(() => undefined);
  }, [cache, load.kind, files.length, savesSettled, input, plotColumns, revKey, deps]);

  return (
    <section aria-labelledby="explore-heading">
      <h1 id="explore-heading">Explore</h1>
      <div className="explore-controls">
        <label>
          Dataset
          <GroupMultiSelect
            options={dataset.options}
            selected={dataset.selectedPaths}
            onChange={dataset.setSelectedPaths}
          />
        </label>
        <button type="button" onClick={dataset.reload}>
          Reload
        </button>
      </div>
      <div role="tablist" aria-label="Explore views" className="pills">
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
      {dataset.orderedPaths.length === 0 && load.kind !== 'loading' && (
        <p className="muted">No groups are included. Tick groups in the Data Manager, or choose them in Dataset.</p>
      )}
      {load.kind === 'loaded' && files.length > 0 && (
        <div key={viewKey}>
          {view === 'table' && (
            <DataTable files={files} deps={deps} autosave={autosave} onReload={dataset.reload} />
          )}
          {view !== 'table' && !savesSettled && <p role="status">Saving table edits…</p>}
          {savesSettled && view === 'missing' && <MissingView deps={deps} input={input} columns={plotColumns} revKey={revKey} />}
          {savesSettled && view === 'histogram' && (
            <HistogramView deps={deps} input={input} columns={plotColumns} revKey={revKey} />
          )}
          {savesSettled && view === 'crosstab' && (
            <CrosstabView deps={deps} input={input} data={data} defaultGroupColumn={defaultGroupColumn(combined)} revKey={revKey} />
          )}
          {savesSettled && view === 'profile' && (
            <ProfileView
              deps={deps}
              input={input}
              columns={plotColumns}
              revKey={revKey}
              groupColumns={combined.descriptiveColumns}
              defaultGroupColumn={files.length > 1 ? combined.groupColumn : ''}
            />
          )}
        </div>
      )}
    </section>
  );
}
