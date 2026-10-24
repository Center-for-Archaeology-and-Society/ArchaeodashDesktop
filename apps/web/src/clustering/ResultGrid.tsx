/**
 * Result table with the controls the legacy DT tables offered: per-column
 * filters, sorting, a column-visibility menu, a compact toggle, and an
 * optional checkbox column. Selection is a set of row keys held by the
 * caller, so checked rows stay checked through sort, filter and column
 * changes. Keys never reach the DOM (checkbox labels use visible cells).
 */
import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import type { TableCell } from '@archaeodash/client';
import {
  filterRows, formatCell, isVisible, loadView, nextSort, saveView, sortRows,
  type GridColumn, type GridView, type SortState,
} from './grid-model.ts';

export interface GridSelection {
  readonly selected: ReadonlySet<string>;
  readonly onChange: (next: Set<string>) => void;
  readonly disabled?: boolean;
}

const PAGE = 100;

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
  const [sort, setSort] = useState<SortState>(null);
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
            const direction = sort?.column === i ? sort.direction : null;
            return <th key={i} aria-sort={direction === 'asc' ? 'ascending' : direction === 'desc' ? 'descending' : undefined}>
              <button type="button" className="result-grid-sort" onClick={() => setSort(current => nextSort(current, i))}>
                {column.label}<span aria-hidden="true" className="result-grid-arrow">{direction === 'asc' ? ' ▲' : direction === 'desc' ? ' ▼' : ''}</span>
              </button>
            </th>;
          })}
        </tr>
        <tr className="result-grid-filters">
          {selection && keys && <th />}
          {visible.map(([i, column]) => <th key={i}>
            <input type="search" aria-label={`Filter ${column.label}`} placeholder="Filter" value={filters[i] ?? ''}
              onChange={event => { const value = event.target.value; setFilterByLabel(current => ({ ...current, [column.label]: value })); setShown(PAGE); }} />
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
