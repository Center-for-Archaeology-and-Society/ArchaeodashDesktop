/**
 * Pure helpers for the Visualize & Assign route (Section 9.4).
 *
 * Selection is keyed internally by the hidden `analytical_uuid`; visible
 * surfaces (tables, labels, exports) only ever show ANID / metadata /
 * predictors. The symbol map is the conservative repeating ten-symbol set,
 * and the data ellipse follows the legacy two-degree-of-freedom Gaussian
 * confidence region (chi-square quantile r² = -2·ln(1-level)).
 */

export const TEN_SYMBOLS: readonly string[] = [
  'circle',
  'square',
  'triangle-up',
  'diamond',
  'cross',
  'x',
  'star',
  'pentagon',
  'hexagram',
  'hourglass',
] as const;

/** Legacy viridis palette, sampled at ten evenly spaced anchor colors. */
export const VIRIDIS: readonly string[] = [
  '#440154',
  '#482878',
  '#3e4989',
  '#31688e',
  '#26828e',
  '#1f9e89',
  '#35b779',
  '#6ece58',
  '#b5de2b',
  '#fde725',
] as const;

/** Repeating ten-symbol map: `SYMBOLS[index % 10]` (Section 9.4). */
export function symbolFor(index: number): string {
  return TEN_SYMBOLS[((index % TEN_SYMBOLS.length) + TEN_SYMBOLS.length) % TEN_SYMBOLS.length] as string;
}

/** Group color: viridis anchor cycled by group index. */
export function colorFor(index: number): string {
  return VIRIDIS[((index % VIRIDIS.length) + VIRIDIS.length) % VIRIDIS.length] as string;
}

/** Metadata filter display: missing cells normalize to the literal `(Missing)`. */
export function normalizeFilterValue(value: string | null | undefined): string {
  if (value === null || value === undefined || value.trim() === '') return '(Missing)';
  return value;
}

export interface FilterableRow {
  analytical_uuid: string;
  descriptive: (string | null)[];
}

/**
 * Metadata field/value filter (Section 9.4): a row passes when the
 * descriptive cell at `columnIndex` equals `rawValue` with `(Missing)`
 * normalization on both sides.
 */
export function rowPassesFilter(
  row: FilterableRow,
  columnIndex: number,
  rawValue: string | null,
): boolean {
  if (rawValue === null || rawValue === '') return true;
  const cell = row.descriptive[columnIndex] ?? null;
  return normalizeFilterValue(cell) === normalizeFilterValue(rawValue);
}

export type LabelMode = 'anid' | 'sampleId' | 'rowNumber';

export interface LabelRow {
  analytical_uuid: string;
  visible_id?: string | null;
}

/**
 * Optional point labels with the legacy fallback chain: ANID / sample ID /
 * displayed row number (Section 9.4). Row numbers are 1-based.
 */
export function labelFor(row: LabelRow, mode: LabelMode, rowNumber: number): string {
  switch (mode) {
    case 'anid':
      return row.analytical_uuid;
    case 'sampleId':
      return row.visible_id ?? `Row ${rowNumber}`;
    case 'rowNumber':
      return String(rowNumber);
  }
}

export interface Selection {
  readonly uuids: ReadonlySet<string>;
}

export function emptySelection(): Selection {
  return { uuids: new Set<string>() };
}

/** Lasso/box selection replaces the previous set (Plotly `selectedpoints`). */
export function replaceSelection(uuids: readonly string[]): Selection {
  return { uuids: new Set(uuids) };
}

export function toggleUuid(selection: Selection, uuid: string): Selection {
  const next = new Set(selection.uuids);
  if (next.has(uuid)) {
    next.delete(uuid);
  } else {
    next.add(uuid);
  }
  return { uuids: next };
}

/** Double-click clears the selection (Section 9.4). */
export function clearSelection(_: Selection): Selection {
  return emptySelection();
}

/**
 * Chi-square quantile for two degrees of freedom: `r² = -2·ln(1 - level)`.
 * 0.50 → 1.3863, 0.95 → 5.9915, 0.99 → 9.2103.
 */
export function chiSquare2(level: number): number {
  if (level <= 0 || level >= 1) {
    throw new Error(`ellipse level must be in (0, 1), got ${level}`);
  }
  return -2 * Math.log(1 - level);
}

/**
 * Data-ellipse outline (Section 9.4, levels 0.50–0.99) for the selected or
 * grouped points: mean and 2×2 covariance of (x, y), eigen-decomposed
 * analytically, scaled by the chi-square quantile of the confidence level.
 * Returns 37 closed-path points; empty input yields no points.
 */
export function ellipsePoints(
  x: readonly number[],
  y: readonly number[],
  level: number,
): { px: number[]; py: number[] } {
  const n = Math.min(x.length, y.length);
  if (n < 2) return { px: [], py: [] };
  let mx = 0;
  let my = 0;
  for (let i = 0; i < n; i++) {
    mx += x[i]!;
    my += y[i]!;
  }
  mx /= n;
  my /= n;
  let a = 0;
  let b = 0;
  let c = 0;
  for (let i = 0; i < n; i++) {
    const dx = x[i]! - mx;
    const dy = y[i]! - my;
    a += dx * dx;
    b += dx * dy;
    c += dy * dy;
  }
  a /= n - 1;
  b /= n - 1;
  c /= n - 1;
  const t = a + c;
  const d = Math.sqrt(Math.max(((a - c) / 2) ** 2 + b * b, 0));
  const l1 = t / 2 + d;
  const l2 = Math.max(t / 2 - d, 0);
  if (l1 <= 0) return { px: [], py: [] };
  const theta = 0.5 * Math.atan2(2 * b, a - c);
  const r = Math.sqrt(chiSquare2(level));
  const s1 = Math.sqrt(l1) * r;
  const s2 = Math.sqrt(l2) * r;
  const cos = Math.cos(theta);
  const sin = Math.sin(theta);
  const px: number[] = [];
  const py: number[] = [];
  for (let k = 0; k <= 36; k++) {
    const phi = (k / 36) * 2 * Math.PI;
    const ex = s1 * Math.cos(phi);
    const ey = s2 * Math.sin(phi);
    px.push(mx + ex * cos - ey * sin);
    py.push(my + ex * sin + ey * cos);
  }
  return { px, py };
}
