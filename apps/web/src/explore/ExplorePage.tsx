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
  ExploreCompositionalProfileResponse,
  ExportFormat,
  ExploreService,
  ExportsService,
  GroupRowsResponse,
  GroupsService,
} from '@archaeodash/client';
import type { Data } from 'plotly.js-dist-min';
import { downloadExportResult } from '../exports.ts';
import { PlotlyChart } from '../charts/PlotlyChart.tsx';
import { plotlyText } from '../charts/plotly-text.ts';
import { groupColors } from '../visualize/visualize-model.ts';
import {
  asRowsResponse,
  combineGroupFiles,
  defaultGroupColumn,
  inputPaths,
  type CombinedRows,
  type LoadedGroupFile,
} from '../visualize/combine.ts';
import { GroupMultiSelect } from '../datasets/GroupMultiSelect.tsx';
import { useGroupFiles } from '../datasets/useGroupFiles.ts';
import { HiddenIdNote } from './HiddenIdNote.tsx';
import { CellAutosave, cellKey } from './autosave.ts';
import { useAnalysisSetup } from '../analysis-setup/AnalysisSetupContext.tsx';
import { useCachedResult, useDataCache } from '../datasets/DataCache.tsx';
import { analysisInput } from '../analysis-setup/setup.ts';
import { missingProfile } from './missing.ts';
import { nextSort, sameSortKey, tableOrder, type SortKey, type SortState } from './tableView.ts';
import { BusyLabel, BusyNote } from '../ui/Busy.tsx';
import { TableCard, num, type TableSheet } from '../ui/TableExport.tsx';
import { PageHeader } from '../ui/PageHeader.tsx';
import { TabPanels } from '../ui/TabPanels.tsx';
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
  // Global search and column sort (legacy DT table). The order lists
  // indexes into combined.rows, so edits still reach the right row.
  const [query, setQuery] = useState('');
  const [sort, setSort] = useState<SortState | null>(null);
  const order = useMemo(
    () =>
      tableOrder(combined.rows, combined.descriptiveColumns.length, query, sort, (row, i) => {
        const key = cellKey(row.analytical_uuid, combined.descriptiveColumns[i] ?? '');
        // Saved edits count; the order is recomputed when the search or sort
        // changes, not on every save, so an edited row stays in place.
        return autosave.saved.has(key) ? (autosave.saved.get(key) ?? null) : (row.descriptive[i] ?? null);
      }),
    [combined, query, sort, autosave],
  );
  // Large tables render only the rows in view (plus a margin), so scrolling
  // and typing stay fast however many rows are loaded.
  const total = order.length;
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
  // A new search or sort starts from the top of the table.
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
    setWindowStart(0);
  }, [query, sort]);
  const first = virtual ? Math.min(windowStart, Math.max(0, total - windowSize)) : 0;
  const last = virtual ? Math.min(total, first + windowSize) : total;
  const visibleRows = virtual ? order.slice(first, last) : order;
  const columnCount = 2 + combined.descriptiveColumns.length + combined.elementalColumns.length;
  // Drafts key on uuid+NUL+column (the UUID itself is never rendered).
  const [drafts, setDrafts] = useState<Map<string, string>>(new Map());
  const [, setTick] = useState(0);
  const [exportError, setExportError] = useState('');
  const [exporting, setExporting] = useState<ExportFormat | 'setup' | null>(null);
  const runExport = (format: ExportFormat) => {
    setExportError('');
    setExporting(format);
    void autosave
      .idle()
      .then(() => deps.exports.measuredData({ ...inputPaths(files), raw_text: false, format }))
      .then(downloadExportResult)
      .catch((err: unknown) => setExportError(err instanceof Error ? err.message : String(err)))
      .finally(() => setExporting(null));
  };
  // The legacy "chemical" export: values after the Analysis setup (selected
  // elements, imputation, ratios, transform), computed now, never stored.
  const { setup } = useAnalysisSetup();
  const setupDefinition = useMemo(() => {
    if (!setup) return null;
    const input = analysisInput(setup, combined.elementalColumns);
    if (input.columns.length === 0) return null;
    return input.transformation ?? { ...setup, elemental_columns: input.elements, ratios: [] };
  }, [setup, combined.elementalColumns]);
  const runSetupExport = (format: ExportFormat) => {
    if (!setupDefinition) return;
    setExportError('');
    setExporting('setup');
    void autosave
      .idle()
      .then(() => deps.exports.transformed({ ...inputPaths(files), definition: setupDefinition, raw_text: false, format }))
      .then(downloadExportResult)
      .catch((err: unknown) => setExportError(err instanceof Error ? err.message : String(err)))
      .finally(() => setExporting(null));
  };
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
          disabled={exporting !== null}
          onClick={() => runExport('xlsx')}
          title={files.length > 1 ? 'One sheet with every row, plus one sheet per group' : undefined}
        >
          <BusyLabel busy={exporting === 'xlsx'} busyText="Exporting…">Export to Excel</BusyLabel>
        </button>
        <button type="button" disabled={exporting !== null} onClick={() => runExport('csv')}>
          <BusyLabel busy={exporting === 'csv'} busyText="Exporting…">Export CSV</BusyLabel>
        </button>
        {setupDefinition && (
          <button
            type="button"
            disabled={exporting !== null}
            onClick={() => runSetupExport('xlsx')}
            title="Values after the Analysis setup (elements, imputation, ratios, transform), as an Excel workbook"
          >
            <BusyLabel busy={exporting === 'setup'} busyText="Exporting…">Export with Analysis setup</BusyLabel>
          </button>
        )}
      </div>
      {autosave.error && autosave.failedCount > 0 && (
        <p role="alert">
          Save failed: {autosave.error}
          {autosave.conflicts.size > 0 && ' Reload to see the current values; unsaved edits are lost.'}
        </p>
      )}
      {exportError && <p role="alert">Export failed: {exportError}</p>}
      <div className="toolbar-row table-search">
        <label className="field field-inline">
          Search
          <input
            type="search"
            value={query}
            placeholder="Filter rows"
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        {query.trim() !== '' && (
          <span role="status" className="muted">
            {`${total.toLocaleString()} of ${combined.rows.length.toLocaleString()} rows match`}
          </span>
        )}
        {sort && (
          <button type="button" className="btn btn-sm" onClick={() => setSort(null)}>
            Clear sort
          </button>
        )}
      </div>
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
              <SortHeader label={combined.visibleIdColumn} sortKey={{ kind: 'visible_id' }} sort={sort} onSort={setSort} />
              <SortHeader label={combined.legacyRowidColumn} sortKey={{ kind: 'legacy_rowid' }} sort={sort} onSort={setSort} />
              {combined.descriptiveColumns.map((c, index) => (
                <SortHeader key={c} label={c} sortKey={{ kind: 'descriptive', index }} sort={sort} onSort={setSort} />
              ))}
              {combined.elementalColumns.map((c, index) => (
                <SortHeader
                  key={c}
                  label={c}
                  sortKey={{ kind: 'elemental', index }}
                  sort={sort}
                  onSort={setSort}
                  className="locked-col"
                  title="Measured elemental values are read-only"
                />
              ))}
            </tr>
          </thead>
          <tbody>
            {virtual && first > 0 && (
              <tr aria-hidden="true" className="table-spacer" style={{ height: first * rowHeight }}>
                <td colSpan={columnCount} />
              </tr>
            )}
            {visibleRows.map((rowIndex, offset) => {
              const row = combined.rows[rowIndex]!;
              const position = first + offset;
              const path = combined.pathOf[rowIndex]!;
              const own = fileOf.get(path)?.data.descriptive_columns ?? [];
              const rowLabel = row.visible_id ?? `row ${rowIndex + 1}`;
              return (
                <tr key={row.analytical_uuid} data-row aria-rowindex={position + 2}>
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
      {total === 0 && combined.rows.length > 0 && <p className="muted">No rows match the search.</p>}
    </div>
  );
}

/** Column header that sorts the table: ascending, descending, then file order. */
function SortHeader({
  label,
  sortKey,
  sort,
  onSort,
  className,
  title,
}: {
  label: string;
  sortKey: SortKey;
  sort: SortState | null;
  onSort: (next: SortState | null) => void;
  className?: string;
  title?: string;
}): ReactElement {
  const active = sort !== null && sameSortKey(sort.key, sortKey) ? sort.dir : null;
  return (
    <th
      className={className}
      title={title}
      aria-sort={active === 'asc' ? 'ascending' : active === 'desc' ? 'descending' : undefined}
    >
      <button type="button" className="sort-header" onClick={() => onSort(nextSort(sort, sortKey))}>
        {label}
        <span className="sort-indicator" aria-hidden="true">
          {active === 'asc' ? '▲' : active === 'desc' ? '▼' : ''}
        </span>
      </button>
    </th>
  );
}

/**
 * Keeps a select's value among its options: when the options change (for
 * example the Analysis setup drops an element), a value no longer offered
 * falls back to `fallback` if offered, else the first option.
 */
export function validChoice(value: string, options: readonly string[], fallback = ''): string {
  if (options.includes(value)) return value;
  if (options.includes(fallback)) return fallback;
  return options[0] ?? '';
}

function useValidChoice(
  options: readonly string[],
  initial: string | (() => string),
  fallback = '',
): [string, (v: string) => void] {
  const [value, setValue] = useState(initial);
  const effective = validChoice(value, options, fallback);
  useEffect(() => {
    if (effective !== value) setValue(effective);
  }, [effective, value]);
  return [effective, setValue];
}

/** Request input over the selected group files. */
type InputPaths = { path: string; extra_paths: string[] };

/** Missing-value bands (Good ≤5% / OK ≤40% / Bad ≤80% / Remove). */
/** Cache key for an Explore result: view, file revisions, and request. */
export function exploreKey(kind: string, revKey: string, request: unknown): string {
  return `explore:${kind}:${revKey}:${JSON.stringify(request)}`;
}

/**
 * Missing-value profile, computed from the rows already loaded (no backend
 * round trip), so the tab opens instantly however large the dataset is.
 */
export function MissingView({ combined, columns }: { combined: CombinedRows; columns: string[] }): ReactElement {
  const rows = useMemo(() => missingProfile(combined, columns), [combined, columns]);
  const traces = useMemo(() => missingBarTraces(rows), [rows]);
  const layout = useMemo(
    () => ({
      barmode: 'stack' as const,
      xaxis: { title: { text: 'Missing rows' }, rangemode: 'tozero' as const },
      yaxis: {
        title: { text: 'Features' },
        categoryorder: 'array' as const,
        categoryarray: rows.map((r) => r.feature),
        automargin: true,
      },
      legend: { orientation: 'h' as const, y: 1.08, x: 0 },
      showlegend: true,
    }),
    [rows],
  );
  if (rows.length === 0) return <p>Choose a dataset first.</p>;
  const sheet: TableSheet = {
    name: 'Missing values',
    columns: ['Feature', 'Missing', 'Missing (%)', 'Band'],
    rows: rows.map((r) => [r.feature, r.num_missing, num(r.pct_missing * 100), r.band]),
  };
  return (
    <div className="results">
    <section className="card">
      <PlotlyChart
        data={traces}
        layout={layout}
        ariaLabel="Missing rows by feature"
        saveName="Missing values"
        height={Math.max(260, 22 * rows.length + 110)}
      />
    </section>
    <TableCard title="Missing values" fileName="Missing values" sheets={() => [sheet]} note={`${combined.rows.length} rows. Bands: Good ≤ 5%, OK ≤ 40%, Bad ≤ 80%, else Remove.`}>
    <table className="data-table missing-table">
      <thead>
        <tr>
          <th>Feature</th>
          <th>Missing</th>
          <th>%</th>
          <th>Band</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.feature} data-band={row.band}>
            <td>{row.feature}</td>
            <td>{row.num_missing}</td>
            <td>{Number.isNaN(row.pct_missing) ? 'NA' : (row.pct_missing * 100).toFixed(1)}</td>
            <td><span className={`band band-${row.band.toLowerCase()}`}>{row.band}</span></td>
          </tr>
        ))}
      </tbody>
    </table>
    </TableCard>
    </div>
  );
}

/** Band fill colors for the missing-value chart (Good, OK, Bad, Remove). */
export const BAND_COLORS: Readonly<Record<string, string>> = {
  Good: '#1f8a63',
  OK: '#3b6fd8',
  Bad: '#d9822b',
  Remove: '#c8423b',
};

/**
 * Legacy `plot_missing`: horizontal bars of missing rows per feature, filled
 * by band, one trace per band present (in band order) so the legend lists
 * the bands. Features keep the profile order (most missing first, drawn at
 * the bottom as with `coord_flip`).
 */
export function missingBarTraces(
  rows: readonly { feature: string; num_missing: number; pct_missing: number; band: string }[],
): Data[] {
  return Object.keys(BAND_COLORS)
    .map((band) => ({ band, rows: rows.filter((r) => r.band === band) }))
    .filter((b) => b.rows.length > 0)
    .map(
      ({ band, rows: bandRows }) =>
        ({
          type: 'bar',
          orientation: 'h',
          name: band,
          y: bandRows.map((r) => r.feature),
          x: bandRows.map((r) => r.num_missing),
          text: bandRows.map((r) => (Number.isNaN(r.pct_missing) ? 'NA' : `${(r.pct_missing * 100).toFixed(1)}%`)),
          textposition: 'none',
          hovertemplate: `%{y}: %{x} missing (%{text}, ${band})<extra></extra>`,
          marker: { color: BAND_COLORS[band] },
        }) as Data,
    );
}

const DEFAULT_BINS = 30;
const MIN_BINS = 2;
const MAX_BINS = 100;

/** Bin count typed in the histogram field: a whole number 2–100, else null. */
export function parseBins(text: string): number | null {
  const t = text.trim();
  if (!/^\d+$/.test(t)) return null;
  const n = Number(t);
  return n >= MIN_BINS && n <= MAX_BINS ? n : null;
}

/** Short label for a bin edge (hover text). */
function edgeLabel(v: number | undefined): string {
  return v === undefined ? '' : String(Number(v.toPrecision(6)));
}
/** Largest profile (rows × elements) warmed in the background. */
const PROFILE_WARMUP_CELLS = 250_000;

/** Equal-width bins (legacy ggplot `geom_histogram(bins = n)`): exactly n bars. */
function histogramRequest(input: InputPaths, column: string, bins: number) {
  return { ...input, column, bins, binning: 'equal_width' as const, transformation: null };
}

function profileRequest(input: InputPaths, columns: string[], groupColumn: string) {
  return { ...input, columns, group_column: groupColumn || null, transformation: null };
}

/**
 * Background warm-up of the Explore results: one request at a time, through
 * the shared result cache (same keys as the views), cancelled when the data
 * or columns change.
 */
function useExploreWarmup(
  deps: ExploreDeps,
  ready: boolean,
  input: InputPaths,
  columns: string[],
  profileGroupColumn: string,
  revKey: string,
  rowCount: number,
): void {
  const cache = useDataCache();
  useEffect(() => {
    if (!cache || !ready || columns.length === 0) return;
    let cancelled = false;
    void (async () => {
      for (const column of columns) {
        if (cancelled) return;
        const request = histogramRequest(input, column, DEFAULT_BINS);
        await cache.result(exploreKey('histogram', revKey, request), () => deps.explore.histogram(request)).catch(() => undefined);
      }
      // The profile response has one entry per row and element; only warm it
      // when that stays modest.
      if (cancelled || rowCount * columns.length > PROFILE_WARMUP_CELLS) return;
      const request = profileRequest(input, columns, profileGroupColumn);
      await cache.result(exploreKey('profile', revKey, request), () => deps.explore.compositionalProfile(request)).catch(() => undefined);
    })();
    return () => {
      cancelled = true;
    };
  }, [cache, ready, input, columns, profileGroupColumn, revKey, rowCount, deps]);
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
  const [column, setColumn] = useValidChoice(columns, columns[0] ?? '');
  // The typed text is kept as is (it may be empty or out of range while
  // typing); the plot uses the last valid bin count.
  const [binsText, setBinsText] = useState(String(DEFAULT_BINS));
  const [bins, setBins] = useState(DEFAULT_BINS);
  const [binsTouched, setBinsTouched] = useState(false);
  const binsInvalid = parseBins(binsText) === null;
  const showBinsError = binsInvalid && (binsTouched || binsText.trim() !== '');
  const request = useMemo(() => histogramRequest(input, column, bins), [input, column, bins]);
  const state: AsyncState<ExploreHistogramResponse> = useCachedResult(
    column ? exploreKey('histogram', revKey, request) : null,
    () => deps.explore.histogram(request),
  );

  const bars = useMemo<Data[]>(() => {
    if (state.kind !== 'loaded') return [];
    const { breaks, counts } = state.data;
    return [{
      type: 'bar',
      x: counts.map((_, i) => ((breaks[i] ?? 0) + (breaks[i + 1] ?? 0)) / 2),
      y: counts,
      width: counts.map((_, i) => (breaks[i + 1] ?? 0) - (breaks[i] ?? 0)),
      text: counts.map((c, i) => `${edgeLabel(breaks[i])}–${edgeLabel(breaks[i + 1])}: ${c}`),
      textposition: 'none',
      hovertemplate: '%{text}<extra></extra>',
      marker: { color: '#1f8a63', line: { color: 'rgba(255,255,255,0.6)', width: 1 } },
    } as Data];
  }, [state]);
  const histLayout = useMemo(() => ({ bargap: 0, xaxis: { title: { text: column } }, yaxis: { title: { text: 'Count' } }, showlegend: false }), [column]);
  const binSheet = (): TableSheet[] => state.kind !== 'loaded' ? [] : [{
    name: `Histogram ${column}`,
    columns: ['From', 'To', 'Count'],
    rows: state.data.counts.map((c, i) => [num(state.data.breaks[i]), num(state.data.breaks[i + 1]), c]),
  }];
  return (
    <div className="results">
      <div className="toolbar-row">
      <label className="field field-inline">
        Field
        <select value={column} onChange={(e) => setColumn(e.target.value)}>
          {columns.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      <label className="field field-inline">
        Bins (2–100)
        <input
          type="number"
          min={MIN_BINS}
          max={MAX_BINS}
          step={1}
          value={binsText}
          aria-invalid={showBinsError ? true : undefined}
          aria-describedby={showBinsError ? 'explore-bins-error' : undefined}
          onChange={(e) => {
            const text = e.target.value;
            setBinsText(text);
            const n = parseBins(text);
            if (n !== null) setBins(n);
          }}
          onBlur={() => setBinsTouched(true)}
        />
      </label>
      {showBinsError && (
        <span id="explore-bins-error" role="alert" className="explore-field-error">
          {`Enter a whole number from ${MIN_BINS} to ${MAX_BINS}; showing ${bins} bins.`}
        </span>
      )}
      </div>
      {state.kind === 'loading' && <BusyNote>Computing histogram…</BusyNote>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && (
        <TableCard bare title={`Histogram of ${column}`} fileName={`Histogram ${column}`} sheets={binSheet}>
          <PlotlyChart data={bars} layout={histLayout} ariaLabel={`Histogram of ${column}`} saveName={`Histogram ${column}`} height={340} />
        </TableCard>
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
  // Any column can group (legacy crosstab selects listed every column).
  const allColumns = useMemo(
    () => data.descriptive_columns.concat(data.elemental_columns),
    [data.descriptive_columns, data.elemental_columns],
  );
  const [groupColumn, setGroupColumn] = useValidChoice(allColumns, defaultGroupColumn, defaultGroupColumn);
  // Default to the first other descriptive field with two or more distinct
  // non-missing values; an all-missing column makes an empty-looking table.
  const [valueColumn, setValueColumn] = useValidChoice(allColumns, () => {
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

  const crossSheet = (): TableSheet[] => {
    if (state.kind !== 'loaded') return [];
    return state.data.kind === 'count'
      ? [{ name: 'Crosstab', columns: [groupColumn, valueColumn, 'count'], rows: state.data.rows.map((r) => [r.group ?? '(Missing)', r.value ?? '(Missing)', r.count]) }]
      : [{ name: 'Crosstab', columns: [groupColumn, state.data.result_column], rows: state.data.rows.map((r) => [r.group ?? '(Missing)', num(r.result)]) }];
  };
  return (
    <div className="results">
      <div className="toolbar-row">
      <label className="field field-inline">
        Group field
        <select value={groupColumn} onChange={(e) => setGroupColumn(e.target.value)}>
          {allColumns.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      <label className="field field-inline">
        Second field
        <select value={valueColumn} onChange={(e) => setValueColumn(e.target.value)}>
          {allColumns.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      <label className="field field-inline">
        Summary
        <select value={method} onChange={(e) => setMethod(e.target.value as typeof method)}>
          <option value="count">count</option>
          <option value="mean">mean</option>
          <option value="median">median</option>
          <option value="sd">sd</option>
        </select>
      </label>
      </div>
      {state.kind === 'loading' && <BusyNote>Computing crosstab…</BusyNote>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && (
        <TableCard title={`${valueColumn} by ${groupColumn}`} fileName={`Crosstab ${valueColumn} by ${groupColumn}`} sheets={crossSheet}>
      {state.data.kind === 'count' && (
        <CrosstabCountTable rows={state.data.rows} groupLabel={groupColumn} valueLabel={valueColumn} />
      )}
      {state.data.kind === 'summary' && (
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
                <td>{row.result === null ? '(Missing)' : row.result.toFixed(2)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
        </TableCard>
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
      name: plotlyText(name),
      x: series.x,
      y: series.y,
      connectgaps: false,
      opacity: grouped ? 0.45 : 0.25,
      line: { color: grouped ? colors[i] : '#d62728', width: 1 },
      hovertemplate: `${plotlyText(name)}<br>%{x}: %{y}<extra></extra>`,
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
  const groupOptions = useMemo(() => [''].concat(groupColumns), [groupColumns]);
  const [groupColumn, setGroupColumn] = useValidChoice(groupOptions, defaultGroupColumn, defaultGroupColumn);
  const [logScale, setLogScale] = useState(true);
  const request = useMemo(() => profileRequest(input, columns, groupColumn), [input, columns, groupColumn]);
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
      <div className="toolbar-row">
        <label className="field field-inline">
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
        <label className="field-check">
          <input type="checkbox" checked={logScale} onChange={(e) => setLogScale(e.target.checked)} /> Log scale
        </label>
      </div>
      {state.kind === 'loading' && <BusyNote>Computing profile…</BusyNote>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && traces.length > 0 && (
        <section className="card"><PlotlyChart data={traces} layout={layout} ariaLabel="Compositional profile" saveName="Compositional profile" height={480} /></section>
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
  const plotColumns = useMemo(() => analysisInput(setup, combined.elementalColumns).elements, [setup, combined.elementalColumns]);
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

  // Saved table edits change revisions, so results recompute after them.
  const revKey = autosave.revisionsKey;
  const profileGroupColumn = files.length > 1 ? combined.groupColumn : '';
  // Warm the other tabs in the background once the data is in: every
  // element's default histogram, then the default profile plot. Results are
  // cached with the file revisions, so tabs and field changes open at once.
  useExploreWarmup(deps, load.kind === 'loaded' && files.length > 0 && savesSettled, input, plotColumns, profileGroupColumn, revKey, combined.rows.length);

  return (
    <section className="page" aria-labelledby="explore-heading">
      <PageHeader id="explore-heading" title="Explore" description="Browse and edit the samples, check missing values, and look at distributions before analysis.">
        <label className="field field-inline">
          Dataset
          <GroupMultiSelect
            options={dataset.options}
            selected={dataset.selectedPaths}
            onChange={dataset.setSelectedPaths}
          />
        </label>
        <button type="button" className="btn" onClick={dataset.reload}>
          Reload
        </button>
      </PageHeader>
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
      {load.kind === 'loading' && <BusyNote>Loading dataset…</BusyNote>}
      {load.kind === 'error' && <p role="alert">Load failed: {load.message}</p>}
      {dataset.orderedPaths.length === 0 && load.kind !== 'loading' && (
        <p className="muted">No groups are included. Tick groups in the Data Manager, or choose them in Dataset.</p>
      )}
      {load.kind === 'loaded' && files.length > 0 && (
        <div key={viewKey}>
          {view !== 'table' && view !== 'missing' && !savesSettled && <BusyNote>Saving table edits…</BusyNote>}
          <TabPanels active={view} panels={{
            table: () => <DataTable files={files} deps={deps} autosave={autosave} onReload={dataset.reload} />,
            missing: () => <MissingView combined={combined} columns={plotColumns} />,
            histogram: () => <HistogramView deps={deps} input={input} columns={plotColumns} revKey={revKey} />,
            crosstab: () => <CrosstabView deps={deps} input={input} data={data} defaultGroupColumn={defaultGroupColumn(combined)} revKey={revKey} />,
            profile: () => (
              <ProfileView
                deps={deps}
                input={input}
                columns={plotColumns}
                revKey={revKey}
                groupColumns={combined.descriptiveColumns}
                defaultGroupColumn={profileGroupColumn}
              />
            ),
          }} />
        </div>
      )}
    </section>
  );
}
