/**
 * Pure helpers for the result grid: per-column filters, sorting, display
 * formatting, and the per-table view settings (column visibility, compact
 * mode) kept in localStorage.
 */
import type { TableCell } from '@archaeodash/client';

export interface GridColumn {
  readonly label: string;
  /** Hidden until the user shows it (column menu). */
  readonly hiddenByDefault?: boolean;
  /** Fixed decimals for numeric cells (display only; exports keep full values). */
  readonly digits?: number;
}

export type SortKey = { readonly column: number; readonly direction: 'asc' | 'desc' };
export type SortState = SortKey | null;

/** Display text of a cell. */
export function formatCell(value: TableCell | undefined, digits?: number): string {
  if (value === null || value === undefined) return 'Unavailable';
  if (typeof value === 'number' && digits !== undefined && Number.isFinite(value)) return value.toFixed(digits);
  return String(value);
}

const NUMBER = String.raw`(-?(?:\d+(?:\.\d*)?|\.\d+)(?:e[-+]?\d+)?)`;
const COMPARATOR = new RegExp(String.raw`^(<=|>=|<|>|=|≤|≥)\s*` + NUMBER + '$', 'i');
const RANGE = new RegExp('^' + NUMBER + String.raw`\s*\.\.\s*` + NUMBER + '$', 'i');

/** Operators of a number filter; `between` is inclusive. */
export type NumberFilterOp = '>=' | '<=' | '>' | '<' | '=' | 'between';

export interface NumberFilter {
  readonly op: NumberFilterOp;
  readonly a: number;
  /** Upper bound of `between`. */
  readonly b?: number;
}

/** A number filter typed as `>= 50`, `< 10`, `= 3` or `10..50`; null otherwise. */
export function parseNumberFilter(filter: string): NumberFilter | null {
  const query = filter.trim();
  const range = RANGE.exec(query);
  if (range) {
    const a = Number(range[1]);
    const b = Number(range[2]);
    return { op: 'between', a: Math.min(a, b), b: Math.max(a, b) };
  }
  const comparison = COMPARATOR.exec(query);
  if (!comparison) return null;
  const symbol = comparison[1]!;
  const op = (symbol === '≥' ? '>=' : symbol === '≤' ? '<=' : symbol) as NumberFilterOp;
  return { op, a: Number(comparison[2]) };
}

/** The filter text for a number filter (inverse of `parseNumberFilter`). */
export function numberFilterText(op: NumberFilterOp | '', a: string, b = ''): string {
  if (!op || a.trim() === '' || !Number.isFinite(Number(a))) return '';
  if (op === 'between') return b.trim() !== '' && Number.isFinite(Number(b)) ? `${a.trim()}..${b.trim()}` : '';
  return `${op} ${a.trim()}`;
}

/** Whether a number passes a number filter. */
export function numberPasses(value: number, filter: NumberFilter): boolean {
  switch (filter.op) {
    case '<': return value < filter.a;
    case '<=': return value <= filter.a;
    case '>': return value > filter.a;
    case '>=': return value >= filter.a;
    case 'between': return value >= filter.a && value <= (filter.b ?? filter.a);
    default: return value === filter.a;
  }
}

/**
 * Whether one cell passes one filter. Text matches case-insensitively as a
 * substring of the displayed value; on numeric cells `>`, `<`, `>=`, `<=`,
 * `=` and ranges `a..b` compare numerically (e.g. `>= 50`, `10..50`).
 */
export function cellMatches(value: TableCell | undefined, filter: string, digits?: number): boolean {
  const query = filter.trim();
  if (!query) return true;
  const number = parseNumberFilter(query);
  if (number) return typeof value === 'number' && Number.isFinite(value) && numberPasses(value, number);
  return formatCell(value, digits).toLowerCase().includes(query.toLowerCase());
}

/**
 * Whether a column holds numbers (fixed decimals, or every present cell a
 * number), so it gets a number filter instead of a text one.
 */
export function isNumericColumn(rows: readonly (readonly TableCell[])[], column: number, def?: GridColumn): boolean {
  if (def?.digits !== undefined) return true;
  let seen = false;
  for (const row of rows) {
    const cell = row[column];
    if (cell === null || cell === undefined) continue;
    if (typeof cell !== 'number') return false;
    seen = true;
  }
  return seen;
}

/** Row indices passing every column filter. */
export function filterRows(
  rows: readonly (readonly TableCell[])[],
  filters: readonly string[],
  columns: readonly GridColumn[] = [],
): number[] {
  const active = filters.map((f, i) => [i, f] as const).filter(([, f]) => f.trim() !== '');
  const out: number[] = [];
  rows.forEach((row, index) => {
    if (active.every(([column, filter]) => cellMatches(row[column], filter, columns[column]?.digits))) out.push(index);
  });
  return out;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Compare two cells: numbers numerically, text naturally; missing values last. */
export function compareCells(a: TableCell | undefined, b: TableCell | undefined): number {
  const missingA = a === null || a === undefined || (typeof a === 'number' && !Number.isFinite(a));
  const missingB = b === null || b === undefined || (typeof b === 'number' && !Number.isFinite(b));
  if (missingA || missingB) return missingA === missingB ? 0 : missingA ? 1 : -1;
  if (typeof a === 'number' && typeof b === 'number') return a - b;
  return collator.compare(String(a), String(b));
}

/**
 * Sort row indices (stable) by one key or several (first key first, ties by
 * the next); missing values stay last in both directions.
 */
export function sortRows(rows: readonly (readonly TableCell[])[], indices: readonly number[], sort: SortState | readonly SortKey[]): number[] {
  const keys: readonly SortKey[] = sort === null ? [] : 'column' in sort ? [sort] : sort;
  if (keys.length === 0) return [...indices];
  return [...indices].sort((i, j) => {
    for (const key of keys) {
      const a = rows[i]?.[key.column];
      const b = rows[j]?.[key.column];
      const missingA = a === null || a === undefined || (typeof a === 'number' && !Number.isFinite(a));
      const missingB = b === null || b === undefined || (typeof b === 'number' && !Number.isFinite(b));
      if (missingA || missingB) {
        if (missingA !== missingB) return missingA ? 1 : -1;
        continue;
      }
      const order = (key.direction === 'asc' ? 1 : -1) * compareCells(a, b);
      if (order) return order;
    }
    return i - j;
  });
}

/** Next sort when a header is clicked: ascending, descending, then off. */
export function nextSort(current: SortState, column: number): SortState {
  if (!current || current.column !== column) return { column, direction: 'asc' };
  return current.direction === 'asc' ? { column, direction: 'desc' } : null;
}

// --- Persisted view settings ---

export interface GridView {
  /** Explicit visibility choices by column label; others use the default. */
  readonly visibility: Readonly<Record<string, boolean>>;
  readonly compact: boolean;
}

export const DEFAULT_VIEW: GridView = { visibility: {}, compact: true };

type Storage = Pick<globalThis.Storage, 'getItem' | 'setItem'>;

function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

export const viewKey = (tableId: string): string => `archaeodash.resultTable.${tableId}`;

/** The saved view of one table; the default when absent or unreadable. */
export function loadView(tableId: string, store: Storage | null = storage()): GridView {
  try {
    const raw = store?.getItem(viewKey(tableId));
    if (!raw) return DEFAULT_VIEW;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return DEFAULT_VIEW;
    const { visibility, compact } = parsed as { visibility?: unknown; compact?: unknown };
    const clean: Record<string, boolean> = {};
    if (visibility && typeof visibility === 'object') {
      for (const [label, shown] of Object.entries(visibility)) if (typeof shown === 'boolean') clean[label] = shown;
    }
    return { visibility: clean, compact: typeof compact === 'boolean' ? compact : DEFAULT_VIEW.compact };
  } catch {
    return DEFAULT_VIEW;
  }
}

export function saveView(tableId: string, view: GridView, store: Storage | null = storage()): void {
  try {
    store?.setItem(viewKey(tableId), JSON.stringify(view));
  } catch {
    // Private windows or blocked storage: the view simply is not remembered.
  }
}

export function isVisible(view: GridView, column: GridColumn): boolean {
  return view.visibility[column.label] ?? !column.hiddenByDefault;
}
