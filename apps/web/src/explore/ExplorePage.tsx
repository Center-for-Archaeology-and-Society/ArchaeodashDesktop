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
 * Dataset table over the selected groups (descriptive edits only). Rows from
 * several group files are shown together; edits are saved back to each
 * row's own file. The synthetic group column, and columns a row's file does
 * not have, are read-only.
 */
export function DataTable({
  files,
  deps,
  onSaved,
}: {
  files: readonly LoadedGroupFile[];
  deps: ExploreDeps;
  onSaved: () => void;
}): ReactElement {
  const combined = useMemo(() => combineGroupFiles(files), [files]);
  // Drafts key on uuid+NUL+column so batched edits address hidden identity
  // per column; the UUID itself is never rendered (Section 3.2).
  const [drafts, setDrafts] = useState<Map<string, string>>(new Map());
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved' | { error: string }>(
    'idle',
  );
  const fileOf = useMemo(() => new Map(files.map((f) => [f.path, f])), [files]);

  const editCount = drafts.size;

  const saveEdits = useCallback(async () => {
    if (drafts.size === 0) return;
    setSaveState('saving');
    try {
      const pathOfUuid = new Map(combined.rows.map((r, i) => [r.analytical_uuid, combined.pathOf[i]!]));
      const byPath = new Map<string, { analytical_uuid: string; column: string; value: string }[]>();
      for (const [key, value] of drafts) {
        const sep = key.indexOf('\u0000');
        const uuid = key.slice(0, sep);
        const path = pathOfUuid.get(uuid);
        if (!path) continue;
        const list = byPath.get(path) ?? [];
        list.push({ analytical_uuid: uuid, column: key.slice(sep + 1), value });
        byPath.set(path, list);
      }
      for (const [path, edits] of byPath) {
        const file = fileOf.get(path);
        if (!file) continue;
        await deps.groups.patchDescriptiveValues({
          path,
          expected_revision: file.data.revision_id,
          edits,
        });
      }
      setDrafts(new Map());
      setSaveState('saved');
      onSaved();
    } catch (err: unknown) {
      setSaveState({ error: err instanceof Error ? err.message : String(err) });
      // Some files may have saved; reload so revisions stay current.
      onSaved();
    }
  }, [deps, drafts, combined, fileOf, onSaved]);

  return (
    <div>
      <HiddenIdNote />
      <div className="table-scroll">
        <table className="data-table">
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
            {combined.rows.map((row, rowIndex) => {
              const own = fileOf.get(combined.pathOf[rowIndex]!)?.data.descriptive_columns ?? [];
              return (
                <tr key={row.analytical_uuid}>
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
                    const key = `${row.analytical_uuid}\u0000${column}`;
                    const draft = drafts.get(key);
                    return (
                      <td key={column}>
                        <input
                          value={draft ?? value ?? ''}
                          aria-label={`${column} for ${row.visible_id ?? `row ${rowIndex + 1}`}`}
                          onChange={(e) => setDrafts((prev) => new Map(prev).set(key, e.target.value))}
                        />
                      </td>
                    );
                  })}
                  {row.elemental.map((value, i) => (
                    <td key={combined.elementalColumns[i] ?? i} className="locked-col">
                      {value === null ? 'NA' : value}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <div className="table-actions">
        <button
          type="button"
          disabled={editCount === 0 || saveState === 'saving'}
          onClick={() => void saveEdits()}
        >
          Save descriptive edits ({editCount})
        </button>
        <button
          type="button"
          onClick={() =>
            void deps.exports
              .measuredData({ ...inputPaths(files), raw_text: false })
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

/** Request input over the selected group files. */
type InputPaths = { path: string; extra_paths: string[] };

/** Missing-value bands (Good ≤5% / OK ≤40% / Bad ≤80% / Remove). */
function MissingView({
  deps,
  input,
  columns,
}: {
  deps: ExploreDeps;
  input: InputPaths;
  columns: string[];
}): ReactElement {
  const [state, setState] = useState<AsyncState<ExploreMissingProfileResponse>>({ kind: 'idle' });

  useEffect(() => {
    setState({ kind: 'loading' });
    deps.explore
      .missingProfile({ ...input, columns, transformation: null })
      .then((r) => setState({ kind: 'loaded', data: r }))
      .catch((err: unknown) =>
        setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) }),
      );
  }, [deps, input, columns]);

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
}: {
  deps: ExploreDeps;
  input: InputPaths;
  columns: string[];
}): ReactElement {
  const [column, setColumn] = useState(columns[0] ?? '');
  const [bins, setBins] = useState(30);
  const [state, setState] = useState<AsyncState<ExploreHistogramResponse>>({ kind: 'idle' });

  useEffect(() => {
    if (!column) return;
    setState({ kind: 'loading' });
    deps.explore
      .histogram({ ...input, column, bins, transformation: null })
      .then((r) => setState({ kind: 'loaded', data: r }))
      .catch((err: unknown) =>
        setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) }),
      );
  }, [deps, input, column, bins]);

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
}: {
  deps: ExploreDeps;
  input: InputPaths;
  data: GroupRowsResponse;
  defaultGroupColumn: string;
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
  const [state, setState] = useState<AsyncState<ExploreCrosstabResponse>>({ kind: 'idle' });

  useEffect(() => {
    if (!groupColumn || !valueColumn) return;
    setState({ kind: 'loading' });
    deps.explore
      .crosstab({
        ...input,
        group_column: groupColumn,
        value_column: valueColumn,
        summary_method: method,
      })
      .then((result) => setState({ kind: 'loaded', data: result }))
      .catch((err: unknown) =>
        setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) }),
      );
  }, [deps, input, groupColumn, valueColumn, method]);

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
}: {
  deps: ExploreDeps;
  input: InputPaths;
  columns: string[];
  groupColumns: string[];
  defaultGroupColumn: string;
}): ReactElement {
  const [groupColumn, setGroupColumn] = useState(defaultGroupColumn);
  const [logScale, setLogScale] = useState(true);
  const [state, setState] = useState<AsyncState<ExploreCompositionalProfileResponse>>({
    kind: 'idle',
  });

  useEffect(() => {
    let cancelled = false;
    setState({ kind: 'loading' });
    deps.explore
      .compositionalProfile({
        ...input,
        columns,
        group_column: groupColumn || null,
        transformation: null,
      })
      .then((r) => {
        if (!cancelled) setState({ kind: 'loaded', data: r });
      })
      .catch((err: unknown) => {
        if (!cancelled)
          setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [deps, input, columns, groupColumn]);

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
  const input = useMemo(() => inputPaths(files), [files]);
  // Remount views when the selection or a file revision changes.
  const viewKey = files.map((f) => `${f.path}@${f.data.revision_id}`).join('|');

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
        <p className="muted">Choose one or more groups in Dataset.</p>
      )}
      {load.kind === 'loaded' && files.length > 0 && (
        <div key={viewKey}>
          {view === 'table' && <DataTable files={files} deps={deps} onSaved={dataset.reload} />}
          {view === 'missing' && <MissingView deps={deps} input={input} columns={combined.elementalColumns} />}
          {view === 'histogram' && (
            <HistogramView deps={deps} input={input} columns={combined.elementalColumns} />
          )}
          {view === 'crosstab' && (
            <CrosstabView deps={deps} input={input} data={data} defaultGroupColumn={defaultGroupColumn(combined)} />
          )}
          {view === 'profile' && (
            <ProfileView
              deps={deps}
              input={input}
              columns={combined.elementalColumns}
              groupColumns={combined.descriptiveColumns}
              defaultGroupColumn={files.length > 1 ? combined.groupColumn : ''}
            />
          )}
        </div>
      )}
    </section>
  );
}
