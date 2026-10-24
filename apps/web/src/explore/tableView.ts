/**
 * Search and sort for the Explore dataset table (legacy `DT::datatable`
 * global search and column sort), computed as a display order over the
 * combined rows. The order holds indexes into `combined.rows`, so editing
 * and autosave keep addressing the right row (by its hidden
 * `analytical_uuid`) however the table is sorted or filtered. The UUID is
 * never searched or shown.
 */
import type { CombinedRows } from '../visualize/combine.ts';

export type TableRow = CombinedRows['rows'][number];

export type SortKey =
  | { readonly kind: 'visible_id' | 'legacy_rowid' }
  | { readonly kind: 'descriptive' | 'elemental'; readonly index: number };

export interface SortState {
  readonly key: SortKey;
  readonly dir: 'asc' | 'desc';
}

/** Current text of a descriptive cell (saved edits included), or null. */
export type DescriptiveText = (row: TableRow, index: number) => string | null;

export function sameSortKey(a: SortKey, b: SortKey): boolean {
  if (a.kind !== b.kind) return false;
  return !('index' in a) || !('index' in b) || a.index === b.index;
}

/** Header clicks cycle ascending → descending → unsorted (file order). */
export function nextSort(current: SortState | null, key: SortKey): SortState | null {
  if (!current || !sameSortKey(current.key, key)) return { key, dir: 'asc' };
  return current.dir === 'asc' ? { key, dir: 'desc' } : null;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

function blankToNull(v: string | null | undefined): string | null {
  return v === null || v === undefined || v.trim() === '' ? null : v;
}

function sortValue(row: TableRow, key: SortKey, text: DescriptiveText): string | number | null {
  switch (key.kind) {
    case 'visible_id':
      return blankToNull(row.visible_id);
    case 'legacy_rowid':
      return blankToNull(row.legacy_rowid);
    case 'descriptive':
      return blankToNull(text(row, key.index));
    case 'elemental': {
      const v = row.elemental[key.index];
      return v === null || v === undefined || Number.isNaN(v) ? null : v;
    }
  }
}

function asNumber(text: string): number | null {
  const t = text.trim();
  if (t === '') return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

/** Numbers compare by value; text that reads as numbers does too; else natural order. */
export function compareCells(a: string | number, b: string | number): number {
  const na = typeof a === 'number' ? a : asNumber(a);
  const nb = typeof b === 'number' ? b : asNumber(b);
  if (na !== null && nb !== null) return na - nb;
  return collator.compare(String(a), String(b));
}

/** Cell texts as displayed (lower case), for the global search. */
function rowTexts(row: TableRow, descriptiveCount: number, text: DescriptiveText): string[] {
  const cells: string[] = [];
  if (row.visible_id) cells.push(row.visible_id);
  if (row.legacy_rowid) cells.push(row.legacy_rowid);
  for (let i = 0; i < descriptiveCount; i++) {
    const v = text(row, i);
    if (v) cells.push(v);
  }
  for (const v of row.elemental) cells.push(v === null || Number.isNaN(v) ? 'NA' : String(v));
  return cells.map((c) => c.toLowerCase());
}

/**
 * Display order: rows matching every word of `query` somewhere in their
 * visible cells (DT's smart search), then sorted by `sort` with missing
 * values last in either direction and ties kept in file order.
 */
export function tableOrder(
  rows: readonly TableRow[],
  descriptiveCount: number,
  query: string,
  sort: SortState | null,
  text: DescriptiveText,
): number[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  let order = rows.map((_, i) => i);
  if (terms.length > 0) {
    order = order.filter((i) => {
      const cells = rowTexts(rows[i]!, descriptiveCount, text);
      return terms.every((term) => cells.some((c) => c.includes(term)));
    });
  }
  if (sort) {
    const values = new Map(order.map((i) => [i, sortValue(rows[i]!, sort.key, text)]));
    const sign = sort.dir === 'asc' ? 1 : -1;
    order.sort((x, y) => {
      const a = values.get(x) ?? null;
      const b = values.get(y) ?? null;
      if (a === null || b === null) return a === b ? x - y : a === null ? 1 : -1;
      return sign * compareCells(a, b) || x - y;
    });
  }
  return order;
}
