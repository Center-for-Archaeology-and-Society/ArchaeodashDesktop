/**
 * Result table with the controls the legacy DT tables offered: per-column
 * filters (number columns get an operator and value, e.g. at least 50 or
 * between 10 and 50), sorting by header or by two chosen columns, a
 * column-visibility menu, a compact toggle, how many rows to show, and an
 * optional checkbox column. Selection is a set of row keys held by the
 * caller, so checked rows stay checked through sort, filter and column
 * changes. Keys never reach the DOM (checkbox labels use visible cells).
 */
import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import type { TableCell } from '@archaeodash/client';
import {
  filterRows, formatCell, isNumericColumn, isVisible, loadView, nextSort, numberFilterText, parseNumberFilter, saveView, sortRows,
  type GridColumn, type GridView, type NumberFilterOp, type SortKey,
} from './grid-model.ts';

export interface GridSelection {
  readonly selected: ReadonlySet<string>;
  readonly onChange: (next: Set<string>) => void;
  readonly disabled?: boolean;
}

const PAGE = 100;
const SHOW_CHOICES = [100, 250, 500, 1000] as const;

const NUMBER_OPS: readonly { value: NumberFilterOp | ''; label: string }[] = [
  { value: '', label: 'Any' },
  { value: '>=', label: '≥' },
  { value: '<=', label: '≤' },
  { value: '>', label: '>' },
  { value: '<', label: '<' },
  { value: '=', label: '=' },
  { value: 'between', label: 'Between' },
];

/** Operator and value(s) for a number column's filter (stored as text like `>= 50` or `10..50`). */
function NumberFilterInput({ label, value, onChange }: { label: string; value: string; onChange: (next: string) => void }): ReactElement {
  const parsed = parseNumberFilter(value);
  // Typed values are kept while incomplete (e.g. the upper bound not yet entered).
  const [draft, setDraft] = useState(() => ({
    op: (parsed?.op ?? '') as NumberFilterOp | '',
    a: parsed ? String(parsed.a) : '',
    b: parsed?.b !== undefined ? String(parsed.b) : '',
  }));
  useEffect(() => {
    if (value === '') setDraft((d) => (d.op === '' && d.a === '' && d.b === '' ? d : { op: '', a: '', b: '' }));
  }, [value]);
  const update = (next: typeof draft) => {
    setDraft(next);
    onChange(numberFilterText(next.op, next.a, next.b));
  };
  return <div className="result-grid-number-filter">
    <select aria-label={`Filter ${label}: comparison`} value={draft.op} onChange={event => update({ ...draft, op: event.target.value as NumberFilterOp | '' })}>
      {NUMBER_OPS.map(op => <option key={op.value} value={op.value}>{op.label}</option>)}
    </select>
    {draft.op !== '' && <input type="number" step="any" aria-label={`Filter ${label}: ${draft.op === 'between' ? 'from' : 'value'}`} value={draft.a}
      onChange={event => update({ ...draft, a: event.target.value })} />}
    {draft.op === 'between' && <input type="number" step="any" aria-label={`Filter ${label}: to`} value={draft.b}
      onChange={event => update({ ...draft, b: event.target.value })} />}
  </div>;
}

export function ResultGrid({ tableId, columns, rows, keys, selection, rowLabel, initialView }: {
  /** Names the saved view (column visibility, compact mode). */
  tableId: string;
  columns: readonly GridColumn[];
  rows: readonly (readonly TableCell[])[];
  /** Selection key per row (null: the row cannot be selected). */
  keys?: readonly (string | null)[];
  selection?: GridSelection;
  /** Accessible name of a row's checkbox. */
  rowLabel?: (row: readonly TableCell[], index: number) => string;
  /** View used before the browser storage is read (tests). */
  initialView?: GridView;
}): ReactElement {
  const [view, setView] = useState<GridView>(() => initialView ?? loadView(tableId));
  // Filters by column label, so they survive the caller rebuilding its columns.
  const [filterByLabel, setFilterByLabel] = useState<Record<string, string>>({});
  // Sort keys, first first: a header click sorts by that column alone; the
  // toolbar adds a second key for ties (e.g. group, then probability).
  const [sort, setSort] = useState<readonly SortKey[]>([]);
  const [shown, setShown] = useState(PAGE);
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const outside = (event: MouseEvent) => { if (menu.current && !menu.current.contains(event.target as Node)) setMenuOpen(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') setMenuOpen(false); };
    document.addEventListener('mousedown', outside);
    document.addEventListener('keydown', escape);
    return () => { document.removeEventListener('mousedown', outside); document.removeEventListener('keydown', escape); };
  }, [menuOpen]);

  const updateView = (next: GridView) => { setView(next); saveView(tableId, next); };
  const visible = columns.map((column, i) => [i, column] as const).filter(([, column]) => isVisible(view, column));
  const filters = columns.map(column => filterByLabel[column.label] ?? '');
  const filterKey = JSON.stringify(filters);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- filterKey stands for filters
  const order = useMemo(() => sortRows(rows, filterRows(rows, filters, columns), sort), [rows, filterKey, sort]);
  const numeric = useMemo(() => columns.map((column, i) => isNumericColumn(rows, i, column)), [rows, columns]);
  const setFilter = (label: string, value: string) => { setFilterByLabel(current => ({ ...current, [label]: value })); setShown(PAGE); };
  const setSortKey = (level: number, key: SortKey | null) => setSort(current => {
    const next = current.slice(0, level);
    if (key) next.push(key, ...current.slice(level + 1));
    return next.filter((k, i) => i === 0 || k.column !== next[0]?.column).slice(0, 2);
  });
  const sortPicker = (level: number) => {
    const key = sort[level];
    return <span className="result-grid-sortby">
      <select aria-label={level === 0 ? 'Sort by' : 'Then by'} value={key ? String(key.column) : ''}
        onChange={event => {
          const column = Number(event.target.value);
          setSortKey(level, event.target.value === '' ? null : { column, direction: key?.direction ?? (numeric[column] ? 'desc' : 'asc') });
        }}>
        <option value="">{level === 0 ? 'Original order' : 'None'}</option>
        {visible.filter(([i]) => level === 0 || i !== sort[0]?.column).map(([i, column]) => <option key={i} value={i}>{column.label}</option>)}
      </select>
      {key && <select aria-label={level === 0 ? 'Sort direction' : 'Then-by direction'} value={key.direction}
        onChange={event => setSortKey(level, { column: key.column, direction: event.target.value as SortKey['direction'] })}>
        <option value="asc">{numeric[key.column] ? 'Low to high' : 'A to Z'}</option>
        <option value="desc">{numeric[key.column] ? 'High to low' : 'Z to A'}</option>
      </select>}
    </span>;
  };
  const selectable = selection && keys ? order.map(i => keys[i]).filter((k): k is string => !!k) : [];
  const allChecked = selectable.length > 0 && selectable.every(k => selection!.selected.has(k));
  const label = (i: number) => rowLabel?.(rows[i]!, i) ?? formatCell(rows[i]?.[0]);
  const toggle = (key: string, on: boolean) => {
    if (!selection) return;
    const next = new Set(selection.selected);
    if (on) next.add(key); else next.delete(key);
    selection.onChange(next);
  };
  const toggleAll = (on: boolean) => {
    if (!selection) return;
    const next = new Set(selection.selected);
    for (const key of selectable) { if (on) next.add(key); else next.delete(key); }
    selection.onChange(next);
  };
  const filtered = filters.some(f => f.trim() !== '');

  return <div className="result-grid">
    <div className="result-grid-toolbar">
      <div className="result-grid-columns" ref={menu}>
        <button type="button" className="btn btn-sm" aria-haspopup="true" aria-expanded={menuOpen} onClick={() => setMenuOpen(open => !open)}>Columns</button>
        {menuOpen && <div className="result-grid-menu" role="group" aria-label="Visible columns">
          {columns.map((column, i) => <label key={i} className="field-check">
            <input type="checkbox" checked={isVisible(view, column)} disabled={isVisible(view, column) && visible.length === 1}
              onChange={event => updateView({ ...view, visibility: { ...view.visibility, [column.label]: event.target.checked } })} />
            {column.label}
          </label>)}
        </div>}
      </div>
      <label className="field-check"><input type="checkbox" checked={view.compact} onChange={event => updateView({ ...view, compact: event.target.checked })} />Compact table</label>
      <span className="result-grid-sorting" role="group" aria-label="Sorting">
        <span className="muted">Sort</span>
        {sortPicker(0)}
        {sort[0] && <><span className="muted">then</span>{sortPicker(1)}</>}
      </span>
      <label className="result-grid-show">Show <select value={shown === Number.MAX_SAFE_INTEGER ? 'all' : String(shown)}
        onChange={event => setShown(event.target.value === 'all' ? Number.MAX_SAFE_INTEGER : Number(event.target.value))}>
        {[...new Set([...SHOW_CHOICES, ...(shown === Number.MAX_SAFE_INTEGER ? [] : [shown])])].sort((a, b) => a - b)
          .map(n => <option key={n} value={n}>{n}</option>)}
        <option value="all">All</option>
      </select> rows</label>
      {filtered && <span className="muted">{order.length} of {rows.length} rows match the filters.</span>}
      {filtered && <button type="button" className="btn btn-sm btn-ghost" onClick={() => setFilterByLabel({})}>Clear filters</button>}
    </div>
    <table className={`data-table result-grid-table${view.compact ? ' is-compact' : ''}`}>
      <thead>
        <tr>
          {selection && keys && <th className="result-grid-check">
            <input type="checkbox" aria-label={`Select all ${selectable.length} ${filtered ? 'matching ' : ''}rows`} checked={allChecked}
              disabled={selection.disabled || selectable.length === 0} onChange={event => toggleAll(event.target.checked)} />
          </th>}
          {visible.map(([i, column]) => {
            const direction = sort[0]?.column === i ? sort[0].direction : null;
            return <th key={i} aria-sort={direction === 'asc' ? 'ascending' : direction === 'desc' ? 'descending' : undefined}>
              <button type="button" className="result-grid-sort" onClick={() => setSort(current => { const next = nextSort(current[0] ?? null, i); return next ? [next] : []; })}>
                {column.label}<span aria-hidden="true" className="result-grid-arrow">{direction === 'asc' ? ' ▲' : direction === 'desc' ? ' ▼' : ''}</span>
              </button>
            </th>;
          })}
        </tr>
        <tr className="result-grid-filters">
          {selection && keys && <th />}
          {visible.map(([i, column]) => <th key={i}>
            {numeric[i]
              ? <NumberFilterInput label={column.label} value={filters[i] ?? ''} onChange={value => setFilter(column.label, value)} />
              : <input type="search" aria-label={`Filter ${column.label}`} placeholder="Filter" value={filters[i] ?? ''}
                onChange={event => setFilter(column.label, event.target.value)} />}
          </th>)}
        </tr>
      </thead>
      <tbody>
        {order.slice(0, shown).map(index => {
          const key = keys?.[index] ?? null;
          return <tr key={index} className={key && selection?.selected.has(key) ? 'is-selected' : undefined}>
            {selection && keys && <td className="result-grid-check">
              {key && <input type="checkbox" aria-label={`Select ${label(index)}`} checked={selection.selected.has(key)}
                disabled={selection.disabled} onChange={event => toggle(key, event.target.checked)} />}
            </td>}
            {visible.map(([i, column]) => <td key={i} className={typeof rows[index]?.[i] === 'number' ? 'num' : undefined}>{formatCell(rows[index]?.[i], column.digits)}</td>)}
          </tr>;
        })}
      </tbody>
    </table>
    {order.length === 0 && rows.length > 0 && <p className="empty-note">No rows match the filters.</p>}
    {shown < order.length && <button type="button" className="btn btn-sm" onClick={() => setShown(n => n + PAGE)}>Show {PAGE} more rows</button>}
  </div>;
}
