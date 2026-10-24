/**
 * Pure helpers for the Visualize & Assign route (Section 9.4).
 *
 * Selection is keyed internally by the hidden `analytical_uuid`; visible
 * surfaces (tables, labels, exports) only ever show ANID / metadata /
 * predictors. The symbol map is the conservative repeating ten-symbol set,
 * and the data ellipse follows the legacy ggplot2 `stat_ellipse()` default:
 * a robust multivariate-t fit (`MASS::cov.trob`, nu = 5) scaled by
 * `sqrt(2·qf(level, 2, n - 1))`.
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

/**
 * Distinct colors for `count` groups: viridis anchors spread evenly across
 * the palette so a handful of groups never land on neighbouring shades;
 * more than ten groups cycle the anchors.
 */
export function groupColors(count: number): string[] {
  if (count <= 0) return [];
  if (count === 1) return [VIRIDIS[3] as string];
  if (count > VIRIDIS.length) return Array.from({ length: count }, (_, i) => colorFor(i));
  const step = (VIRIDIS.length - 1) / (count - 1);
  return Array.from({ length: count }, (_, i) => VIRIDIS[Math.round(i * step)] as string);
}

export type PaletteTheme = 'viridis' | 'default';

export const PALETTE_THEMES: readonly [PaletteTheme, string][] = [
  ['viridis', 'Viridis'],
  ['default', 'Default (Dark 3)'],
];

/** sRGB hex for an HCL (polar CIE-LUV, D65) color, clipped like R's `hcl(fixup = TRUE)`. */
export function hclToHex(h: number, c: number, l: number): string {
  if (l <= 0) return '#000000';
  const xn = 95.047;
  const yn = 100;
  const zn = 108.883;
  const rad = (h * Math.PI) / 180;
  const u = c * Math.cos(rad);
  const v = c * Math.sin(rad);
  const y = yn * (l > 7.999592 ? ((l + 16) / 116) ** 3 : l / 903.3);
  const t = xn + yn + zn;
  const x0 = xn / t;
  const y0 = yn / t;
  const u0 = (2 * x0) / (6 * y0 - x0 + 1.5);
  const v0 = (4.5 * y0) / (6 * y0 - x0 + 1.5);
  const uu = u / (13 * l) + u0;
  const vv = v / (13 * l) + v0;
  const x = (9 * y * uu) / (4 * vv);
  const z = -x / 3 - 5 * y + (3 * y) / vv;
  const gamma = (value: number) => {
    const lin = value / 100;
    const g = lin <= 0.0031308 ? 12.92 * lin : 1.055 * lin ** (1 / 2.4) - 0.055;
    return Math.round(Math.min(1, Math.max(0, g)) * 255);
  };
  const r = gamma(3.240479 * x - 1.53715 * y - 0.498535 * z);
  const g = gamma(-0.969256 * x + 1.875992 * y + 0.041556 * z);
  const b = gamma(0.055648 * x - 0.204043 * y + 1.057311 * z);
  return `#${[r, g, b].map((n) => n.toString(16).padStart(2, '0')).join('')}`;
}

/** R `hcl.colors(n, "Dark 3")`: qualitative, C = 80, L = 60, hues from 0°. */
export function dark3Colors(count: number): string[] {
  if (count <= 0) return [];
  const h2 = (360 * (count - 1)) / count;
  return Array.from({ length: count }, (_, i) => hclToHex(count === 1 ? 0 : (h2 * i) / (count - 1), 80, 60));
}

/** Group colors for the chosen plot theme (legacy `plot_theme` input). */
export function paletteColors(theme: PaletteTheme, count: number): string[] {
  return theme === 'default' ? dark3Colors(count) : groupColors(count);
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

/**
 * Multi-value metadata filter (legacy `%in%` match): a row passes when its
 * normalized cell is one of `values`; an empty set passes every row.
 */
export function rowPassesFilterValues(
  row: FilterableRow,
  columnIndex: number,
  values: ReadonlySet<string>,
): boolean {
  if (values.size === 0) return true;
  return values.has(normalizeFilterValue(row.descriptive[columnIndex] ?? null));
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
 * Upper quantile of F(2, d2): closed form of `qf(p, 2, d2)`, since the
 * F(2, d2) CDF is `1 - (1 + 2f/d2)^(-d2/2)`.
 */
export function fQuantile2(p: number, d2: number): number {
  if (p <= 0 || p >= 1 || d2 <= 0) throw new Error(`invalid F quantile arguments p=${p}, d2=${d2}`);
  return (d2 / 2) * ((1 - p) ** (-2 / d2) - 1);
}

/**
 * Robust bivariate location/scatter by `MASS::cov.trob` defaults
 * (multivariate t, nu = 5, maxit = 25, tol = 0.01, unit weights).
 * Returns null when the scatter is singular.
 */
export function covTrob(
  x: readonly number[],
  y: readonly number[],
): { center: [number, number]; cov: [number, number, number] } | null {
  const n = Math.min(x.length, y.length);
  if (n < 2) return null;
  const nu = 5;
  const p = 2;
  let lx = 0;
  let ly = 0;
  for (let i = 0; i < n; i++) {
    lx += x[i]!;
    ly += y[i]!;
  }
  lx /= n;
  ly /= n;
  let w = new Array<number>(n).fill(1);
  const dx = new Array<number>(n).fill(0);
  const dy = new Array<number>(n).fill(0);
  for (let iter = 0; iter < 25; iter++) {
    const w0 = w;
    let sw = 0;
    for (let i = 0; i < n; i++) sw += w[i]!;
    let a = 0;
    let b = 0;
    let c = 0;
    for (let i = 0; i < n; i++) {
      dx[i] = x[i]! - lx;
      dy[i] = y[i]! - ly;
      const wi = w[i]! / sw;
      a += wi * dx[i]! * dx[i]!;
      b += wi * dx[i]! * dy[i]!;
      c += wi * dy[i]! * dy[i]!;
    }
    const det = a * c - b * b;
    if (!(det > 1e-12 * Math.max(a * c, Number.MIN_VALUE)) || !Number.isFinite(det)) return null;
    const next = new Array<number>(n);
    for (let i = 0; i < n; i++) {
      const q = (c * dx[i]! * dx[i]! - 2 * b * dx[i]! * dy[i]! + a * dy[i]! * dy[i]!) / det;
      next[i] = (nu + p) / (nu + q);
    }
    w = next;
    let sx = 0;
    let sy = 0;
    let sw2 = 0;
    for (let i = 0; i < n; i++) {
      sx += w[i]! * x[i]!;
      sy += w[i]! * y[i]!;
      sw2 += w[i]!;
    }
    lx = sx / sw2;
    ly = sy / sw2;
    if (w.every((wi, i) => Math.abs(wi - w0[i]!) < 0.01)) break;
  }
  // cov = crossprod(sqrt(w) * X) / sum(wt), X centred at the last iteration's location.
  let a = 0;
  let b = 0;
  let c = 0;
  for (let i = 0; i < n; i++) {
    a += w[i]! * dx[i]! * dx[i]!;
    b += w[i]! * dx[i]! * dy[i]!;
    c += w[i]! * dy[i]! * dy[i]!;
  }
  a /= n;
  b /= n;
  c /= n;
  if (!(a > 0 && c > 0 && a * c - b * b > 0)) return null;
  return { center: [lx, ly], cov: [a, b, c] };
}

/**
 * Data-ellipse outline (Section 9.4, levels 0.50–0.99) as ggplot2
 * `stat_ellipse(type = "t")` draws it: needs at least three points with
 * distinct x and y values, robust t centre/scatter, radius
 * `sqrt(2·qf(level, 2, n - 1))`, and 52 closed-path points (51 segments).
 * Degenerate input yields no points.
 */
export function ellipsePoints(
  x: readonly number[],
  y: readonly number[],
  level: number,
): { px: number[]; py: number[] } {
  const n = Math.min(x.length, y.length);
  if (n < 3 || new Set(x.slice(0, n)).size < 2 || new Set(y.slice(0, n)).size < 2) return { px: [], py: [] };
  const fit = covTrob(x, y);
  if (!fit) return { px: [], py: [] };
  const [a, b, c] = fit.cov;
  // Upper Cholesky factor U of [[a, b], [b, c]]: shape = U'U.
  const u11 = Math.sqrt(a);
  const u12 = b / u11;
  const u22 = Math.sqrt(Math.max(c - u12 * u12, 0));
  const radius = Math.sqrt(2 * fQuantile2(level, n - 1));
  const px: number[] = [];
  const py: number[] = [];
  const segments = 51;
  for (let k = 0; k <= segments; k++) {
    const angle = (k * 2 * Math.PI) / segments;
    const cx = Math.cos(angle);
    const sy = Math.sin(angle);
    // unit.circle %*% chol: row vector [cos, sin] times U.
    px.push(fit.center[0] + radius * cx * u11);
    py.push(fit.center[1] + radius * (cx * u12 + sy * u22));
  }
  return { px, py };
}
