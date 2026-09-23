/**
 * Explore route (Section 9.4): dataset table with elemental columns locked,
 * plus the missing-value, histogram, crosstab, and compositional-profile
 * views over one loaded group file. Legacy tab set: Dataset, Crosstabs,
 * Univariate Plots, Compositional Profile Plot.
 */
import { useCallback, useEffect, useMemo, useState, type ReactElement } from 'react';
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
import { downloadExportResult } from '../exports.ts';
import { HiddenIdNote } from './HiddenIdNote.tsx';
export interface ExploreDeps {
  readonly groups: GroupsService;
  readonly explore: ExploreService;
  readonly exports: ExportsService;
  /** Legacy `lastOpenedDataset` preference (Section 10.1/9.4). */
  readonly onDatasetOpened?: (path: string) => void;
  /** Restored `lastOpenedDataset` preference, or empty string. */
  readonly initialDataset?: string;
}

type LoadState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'loaded'; data: GroupRowsResponse }
  | { kind: 'error'; message: string };

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

/** One selected group's editable dataset table (descriptive edits only). */
export function DataTable({
  data,
  deps,
  onSaved,
}: {
  data: GroupRowsResponse;
  deps: ExploreDeps;
  onSaved: () => void;
}): ReactElement {
  // Drafts key on uuid+NUL+column so batched edits address hidden identity
  // per column; the UUID itself is never rendered (Section 3.2).
  const [drafts, setDrafts] = useState<Map<string, string>>(new Map());
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved' | { error: string }>(
    'idle',
  );

  const editList = useMemo(
    () =>
      [...drafts.entries()].map(([key, value]) => {
        const sep = key.indexOf('\u0000');
        return {
          analytical_uuid: key.slice(0, sep),
          column: key.slice(sep + 1),
          value,
        };
      }),
    [drafts],
  );

  const saveEdits = useCallback(async () => {
    if (editList.length === 0) return;
    setSaveState('saving');
    try {
      await deps.groups.patchDescriptiveValues({
        path: data.path,
        expected_revision: data.revision_id,
        edits: editList,
      });
      setDrafts(new Map());
      setSaveState('saved');
      onSaved();
    } catch (err: unknown) {
      setSaveState({ error: err instanceof Error ? err.message : String(err) });
    }
  }, [deps, data, editList, onSaved]);

  return (
    <div>
      <HiddenIdNote />
      <table className="data-table">
        <thead>
          <tr>
            <th>{data.visible_id_column}</th>
            <th>{data.legacy_rowid_column}</th>
            {data.descriptive_columns.map((c) => (
              <th key={c}>{c}</th>
            ))}
            {data.elemental_columns.map((c) => (
              <th key={c} className="locked-col" title="Measured elemental values are read-only">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.rows.map((row) => (
            <tr key={row.analytical_uuid}>
              <td>{row.visible_id ?? ''}</td>
              <td>{row.legacy_rowid ?? ''}</td>
              {row.descriptive.map((value, i) => {
                const column = data.descriptive_columns[i] ?? '';
                const key = `${row.analytical_uuid}\u0000${column}`;
                const draft = drafts.get(key);
                return (
                  <td key={column}>
                    <input
                      value={draft ?? value ?? ''}
                      aria-label={`${column} for ${row.visible_id ?? row.analytical_uuid}`}
                      onChange={(e) => setDrafts((prev) => new Map(prev).set(key, e.target.value))}
                    />
                  </td>
                );
              })}
              {row.elemental.map((value, i) => (
                <td key={data.elemental_columns[i] ?? i} className="locked-col">
                  {value === null ? 'NA' : value}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <div className="table-actions">
        <button
          type="button"
          disabled={editList.length === 0 || saveState === 'saving'}
          onClick={() => void saveEdits()}
        >
          Save descriptive edits ({editList.length})
        </button>
        <button
          type="button"
          onClick={() =>
            void deps.exports
              .measuredData({ path: data.path, raw_text: false })
              .then(downloadExportResult)
              .catch((err: unknown) => setSaveState({ error: err instanceof Error ? err.message : String(err) }))
          }
        >
          Export measured data (CSV)
        </button>
        {saveState === 'saved' && <span role="status">Saved.</span>}
        {typeof saveState === 'object' && 'error' in saveState && (
          <span role="alert">Save failed: {saveState.error}</span>
        )}
      </div>
    </div>
  );
}

/** Missing-value bands (Good ≤5% / OK ≤40% / Bad ≤80% / Remove). */
function MissingView({
  deps,
  path,
  columns,
}: {
  deps: ExploreDeps;
  path: string;
  columns: string[];
}): ReactElement {
  const [state, setState] = useState<AsyncState<ExploreMissingProfileResponse>>({ kind: 'idle' });

  useEffect(() => {
    setState({ kind: 'loading' });
    deps.explore
      .missingProfile({ path, columns, transformation: null })
      .then((r) => setState({ kind: 'loaded', data: r }))
      .catch((err: unknown) =>
        setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) }),
      );
  }, [deps, path, columns]);

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
  path,
  columns,
}: {
  deps: ExploreDeps;
  path: string;
  columns: string[];
}): ReactElement {
  const [column, setColumn] = useState(columns[0] ?? '');
  const [bins, setBins] = useState(30);
  const [state, setState] = useState<AsyncState<ExploreHistogramResponse>>({ kind: 'idle' });

  useEffect(() => {
    if (!column) return;
    setState({ kind: 'loading' });
    deps.explore
      .histogram({ path, column, bins, transformation: null })
      .then((r) => setState({ kind: 'loaded', data: r }))
      .catch((err: unknown) =>
        setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) }),
      );
  }, [deps, path, column, bins]);

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
  path,
  data,
}: {
  deps: ExploreDeps;
  path: string;
  data: GroupRowsResponse;
}): ReactElement {
  const [groupColumn, setGroupColumn] = useState(data.descriptive_columns[0] ?? '');
  const [valueColumn, setValueColumn] = useState(
    data.descriptive_columns[1] ?? data.elemental_columns[0] ?? '',
  );
  const [method, setMethod] = useState<'count' | 'mean' | 'median' | 'sd'>('count');
  const [state, setState] = useState<AsyncState<ExploreCrosstabResponse>>({ kind: 'idle' });

  useEffect(() => {
    if (!groupColumn || !valueColumn) return;
    setState({ kind: 'loading' });
    deps.explore
      .crosstab({
        path,
        group_column: groupColumn,
        value_column: valueColumn,
        summary_method: method,
      })
      .then((result) => setState({ kind: 'loaded', data: result }))
      .catch((err: unknown) =>
        setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) }),
      );
  }, [deps, path, groupColumn, valueColumn, method]);

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
      {state.kind === 'loaded' && state.data.rows.kind === 'count' && (
        <table className="data-table">
          <thead>
            <tr>
              <th>Group</th>
              <th>Value</th>
              <th>Count</th>
            </tr>
          </thead>
          <tbody>
            {state.data.rows.rows.map((row, i) => (
              <tr key={i}>
                <td>{row.group ?? '(Missing)'}</td>
                <td>{row.value ?? '(Missing)'}</td>
                <td>{row.count}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {state.kind === 'loaded' && state.data.rows.kind === 'summary' && (
        <table className="data-table">
          <thead>
            <tr>
              <th>Group</th>
              <th>{state.data.rows.result_column}</th>
            </tr>
          </thead>
          <tbody>
            {state.data.rows.rows.map((row, i) => (
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

/** Compositional profile: one polyline per group across ordered predictors. */
function ProfileView({
  deps,
  path,
  columns,
}: {
  deps: ExploreDeps;
  path: string;
  columns: string[];
}): ReactElement {
  const [groupColumn, setGroupColumn] = useState('');
  const [state, setState] = useState<AsyncState<ExploreCompositionalProfileResponse>>({
    kind: 'idle',
  });

  useEffect(() => {
    setState({ kind: 'loading' });
    deps.explore
      .compositionalProfile({
        path,
        columns,
        group_column: groupColumn || null,
        transformation: null,
      })
      .then((r) => setState({ kind: 'loaded', data: r }))
      .catch((err: unknown) =>
        setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) }),
      );
  }, [deps, path, columns, groupColumn]);

  const elements = useMemo(
    () => [...new Set(state.kind === 'loaded' ? state.data.rows.map((r) => r.element) : [])],
    [state],
  );
  const groups = useMemo(
    () =>
      [
        ...new Set(
          state.kind === 'loaded' ? state.data.rows.map((r) => r.group_label ?? '(Missing)') : [],
        ),
      ],
    [state],
  );

  return (
    <div>
      <label>
        Color by group
        <select value={groupColumn} onChange={(e) => setGroupColumn(e.target.value)}>
          <option value="">(none)</option>
          {columns.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
      {state.kind === 'loading' && <p role="status">Computing profile…</p>}
      {state.kind === 'error' && <p role="alert">Error: {state.message}</p>}
      {state.kind === 'loaded' && elements.length > 0 && (
        <svg
          className="profile-plot"
          role="img"
          aria-label="Compositional profile"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
        >
          {groups.map((group, gi) => (
            <polyline
              key={group}
              className="profile-line"
              fill="none"
              stroke={`var(--chart-${gi % 8})`}
              strokeWidth="0.5"
              points={elements
                .map((element, xi) => {
                  const row = state.data.rows.find(
                    (r) => r.element === element && (r.group_label ?? '(Missing)') === group,
                  );
                  if (!row || row.value === null) return null;
                  const x = (xi / Math.max(elements.length - 1, 1)) * 100;
                  const y = 100 - row.value * 10;
                  return `${x},${Math.max(0, Math.min(100, y))}`;
                })
                .filter((p): p is string => p !== null)
                .join(' ')}
            />
          ))}
        </svg>
      )}
      <p className="muted">Compositional profile across ordered predictors, one line per group.</p>
    </div>
  );
}

export function ExplorePage({ deps, initialDataset = '' }: { deps: ExploreDeps; initialDataset?: string }): ReactElement {
  const [candidates, setCandidates] = useState<Awaited<ReturnType<GroupsService['scan']>>>([]);
  const [selectedPath, setSelectedPath] = useState(initialDataset);
  const [loadState, setLoadState] = useState<LoadState>({ kind: 'idle' });
  const [view, setView] = useState<ExploreView>('table');

  const ready = useMemo(
    () => candidates.filter((c) => c.ready && c.group !== null && c.group !== undefined),
    [candidates],
  );

  const loadRows = useCallback(
    (path: string) => {
      setLoadState({ kind: 'loading' });
      deps.groups
        .rows(path)
        .then((data) => setLoadState({ kind: 'loaded', data }))
        .catch((err: unknown) =>
          setLoadState({ kind: 'error', message: err instanceof Error ? err.message : String(err) }),
        );
    },
    [deps],
  );

  useEffect(() => {
    let cancelled = false;
    deps.groups
      .scan()
      .then((list) => {
        if (cancelled) return;
        setCandidates(list);
        const preferred =
          list.find((c) => c.ready && c.path === initialDataset)?.path ??
          list.find((c) => c.ready && c.group)?.path ??
          '';
        setSelectedPath((current) => current || preferred);
      })
      .catch(() => {
        if (!cancelled) setCandidates([]);
      });
    return () => {
      cancelled = true;
    };
  }, [deps, initialDataset]);
  useEffect(() => {
    if (selectedPath) {
      loadRows(selectedPath);
      deps.onDatasetOpened?.(selectedPath);
    }
  }, [selectedPath, loadRows, deps]);

  return (
    <section aria-labelledby="explore-heading">
      <h1 id="explore-heading">Explore</h1>
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
        <button type="button" onClick={() => selectedPath && loadRows(selectedPath)}>
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
      {loadState.kind === 'loading' && <p role="status">Loading dataset…</p>}
      {loadState.kind === 'error' && <p role="alert">Load failed: {loadState.message}</p>}
      {loadState.kind === 'loaded' && view === 'table' && (
        <DataTable
          key={loadState.data.revision_id}
          data={loadState.data}
          deps={deps}
          onSaved={() => loadRows(loadState.data.path)}
        />
      )}
      {loadState.kind === 'loaded' && view === 'missing' && (
        <MissingView deps={deps} path={loadState.data.path} columns={loadState.data.elemental_columns} />
      )}
      {loadState.kind === 'loaded' && view === 'histogram' && (
        <HistogramView deps={deps} path={loadState.data.path} columns={loadState.data.elemental_columns} />
      )}
      {loadState.kind === 'loaded' && view === 'crosstab' && (
        <CrosstabView deps={deps} path={loadState.data.path} data={loadState.data} />
      )}
      {loadState.kind === 'loaded' && view === 'profile' && (
        <ProfileView deps={deps} path={loadState.data.path} columns={loadState.data.elemental_columns} />
      )}
    </section>
  );
}
