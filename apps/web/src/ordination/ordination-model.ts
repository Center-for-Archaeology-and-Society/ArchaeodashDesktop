/**
 * Pure ordination computations for the Ordination route (Section 8.5/8.7),
 * following the factoextra definitions the legacy app plotted:
 *
 * - individual cos² (`fviz_pca_ind(col.ind = "cos2")`): squared score on the
 *   displayed axes over the squared distance to the centre across every
 *   component;
 * - variable coordinates (`get_pca_var`): `rotation[j][k] * sdev[k]`;
 * - variable contribution (`fviz_pca_var(col.var = "contrib")`): per-axis
 *   `100 * cos²_jk / Σ_j cos²_jk`, combined over the displayed axes weighted
 *   by their eigenvalues;
 * - contribution table: per-component contributions summed over the first
 *   `min(4, component_count)` components (IMPLEMENTATION.md §3.2 correction of
 *   the legacy `contrib[, 1:4]`, which failed with fewer than four PCs).
 */

/** Components summed by the contribution table (legacy `contrib[, 1:4]`). */
export const CONTRIBUTION_COMPONENTS = 4;

/** factoextra gradient used by the legacy cos²/contrib plots. */
export const GRADIENT_COLORS = ['#00AFBB', '#E7B800', '#FC4E07'] as const;

/** The gradient as a Plotly colorscale. */
export const GRADIENT_SCALE: [number, string][] = [
  [0, GRADIENT_COLORS[0]],
  [0.5, GRADIENT_COLORS[1]],
  [1, GRADIENT_COLORS[2]],
];

/** Number of components in a rotation (variable-major) matrix. */
export function componentCount(rotation: readonly (readonly number[])[]): number {
  return rotation.reduce((n, row) => Math.min(n, row.length), rotation[0]?.length ?? 0);
}

/**
 * Quality of representation of each row on axes `a` and `b`:
 * `(s_a² + s_b²) / Σ_k s_k²`. A row at the centre (zero distance) gets 0.
 */
export function individualCos2(scores: readonly (readonly number[])[], a = 0, b = 1): number[] {
  return scores.map((row) => {
    const total = row.reduce((sum, v) => sum + (Number.isFinite(v) ? v * v : 0), 0);
    if (!(total > 0)) return 0;
    const sa = row[a] ?? 0;
    const sb = row[b] ?? 0;
    return (sa * sa + sb * sb) / total;
  });
}

/** Variable coordinates on one component: `rotation[j][k] * sdev[k]`. */
export function variableCoords(rotation: readonly (readonly number[])[], sdev: readonly number[], k: number): number[] {
  const s = sdev[k] ?? 0;
  return rotation.map((row) => (row[k] ?? 0) * s);
}

/**
 * Per-variable percentage contribution to component `k`:
 * `100 * cos²_jk / Σ_j cos²_jk` with `cos² = coord²` (equal to
 * `100 * rotation²` for unit-norm loadings). All zero when the component is.
 */
export function componentContrib(rotation: readonly (readonly number[])[], k: number): number[] {
  const sq = rotation.map((row) => (row[k] ?? 0) ** 2);
  const total = sq.reduce((s, v) => s + v, 0);
  return sq.map((v) => (total > 0 ? (100 * v) / total : 0));
}

/**
 * Contribution of each variable to axes `a` and `b` together, as factoextra
 * colours `fviz_pca_var(col.var = "contrib")`: the per-axis contributions
 * weighted by eigenvalue (`sdev²`), `(c_a·eig_a + c_b·eig_b) / (eig_a + eig_b)`.
 */
export function variableContrib(rotation: readonly (readonly number[])[], sdev: readonly number[], a = 0, b = 1): number[] {
  const ea = (sdev[a] ?? 0) ** 2;
  const eb = (sdev[b] ?? 0) ** 2;
  const ca = componentContrib(rotation, a);
  const cb = componentContrib(rotation, b);
  const total = ea + eb;
  return ca.map((v, j) => (total > 0 ? (v * ea + (cb[j] ?? 0) * eb) / total : 0));
}

export interface ContributionTable {
  /** Components summed: `min(4, component_count)`. */
  readonly components: number;
  /** Variable name and summed contribution (%), largest first. */
  readonly rows: readonly (readonly [string, number])[];
}

/** Contributions summed over the first `min(4, component_count)` components, sorted descending. */
export function contributionTable(rotation: readonly (readonly number[])[], names: readonly string[]): ContributionTable {
  const components = Math.min(CONTRIBUTION_COMPONENTS, componentCount(rotation));
  const sums = rotation.map(() => 0);
  for (let k = 0; k < components; k++) {
    componentContrib(rotation, k).forEach((v, j) => {
      sums[j] = (sums[j] ?? 0) + v;
    });
  }
  const rows = sums
    .map((v, j) => [names[j] ?? `Column ${j + 1}`, v] as const)
    .sort((x, y) => y[1] - x[1]);
  return { components, rows };
}

/** Human-readable statement of the contribution-table rule. */
export function contributionRule(components: number, scoreNames: readonly string[]): string {
  if (components === 0) return 'No components to summarize.';
  const first = scoreNames[0] ?? 'PC1';
  const last = scoreNames[components - 1] ?? `PC${components}`;
  const span = components === 1 ? first : `${first}–${last}`;
  const shortfall = components < CONTRIBUTION_COMPONENTS ? ` (only ${components} component${components === 1 ? '' : 's'} available)` : '';
  return `Sum of each variable's percentage contribution to ${span}: the first min(4, number of components) = ${components}${shortfall}. Largest first.`;
}

/** Linearly interpolated colour on the gradient for `t` in [0, 1]. */
export function gradientColor(t: number): string {
  const clamped = Number.isFinite(t) ? Math.min(1, Math.max(0, t)) : 0;
  const stops = GRADIENT_COLORS.map((hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)));
  const pos = clamped * (stops.length - 1);
  const i = Math.min(stops.length - 2, Math.floor(pos));
  const f = pos - i;
  const from = stops[i] ?? [0, 0, 0];
  const to = stops[i + 1] ?? from;
  const rgb = from.map((c, n) => Math.round(c + ((to[n] ?? c) - c) * f));
  return `#${rgb.map((c) => c.toString(16).padStart(2, '0')).join('')}`;
}

/** Maps values to [0, 1] over their own range (a constant set maps to 0.5). */
export function normalize(values: readonly number[]): number[] {
  const finite = values.filter(Number.isFinite);
  const lo = Math.min(...finite);
  const hi = Math.max(...finite);
  return values.map((v) => (hi > lo ? (v - lo) / (hi - lo) : 0.5));
}

/** How the LDA vector view can be drawn for a response. */
export type LdaLayout =
  | { readonly kind: 'none' }
  | { readonly kind: 'one'; readonly name: string }
  | { readonly kind: 'two'; readonly names: readonly [string, string] };

/**
 * Number of usable discriminants: the smallest of the score names, every
 * score row, and every scaling row (a malformed response never indexes a
 * missing column).
 */
export function ldaDimensions(lda: {
  readonly score_names: readonly string[];
  readonly scores: readonly (readonly number[])[];
  readonly scaling: readonly (readonly number[])[];
}): number {
  let n = lda.score_names.length;
  for (const row of lda.scores) n = Math.min(n, row.length);
  for (const row of lda.scaling) n = Math.min(n, row.length);
  return Math.max(0, n);
}

/** LD1/LD2 vector view when two discriminants exist, else a one-dimensional alternative. */
export function ldaLayout(lda: Parameters<typeof ldaDimensions>[0]): LdaLayout {
  const n = ldaDimensions(lda);
  if (n === 0) return { kind: 'none' };
  const name = (i: number) => lda.score_names[i] ?? `LD${i + 1}`;
  if (n === 1) return { kind: 'one', name: name(0) };
  return { kind: 'two', names: [name(0), name(1)] };
}

/** One variable's discriminant coefficients on the displayed axes. */
export interface LdaVector {
  readonly variable: string;
  readonly x: number;
  readonly y: number;
}

/** Arrow end points `(scaling[v][0], scaling[v][1])` per variable (legacy `plotLDAvectors`). */
export function ldaVectors(scaling: readonly (readonly number[])[], names: readonly string[]): LdaVector[] {
  return scaling.map((row, v) => ({ variable: names[v] ?? `Column ${v + 1}`, x: row[0] ?? 0, y: row[1] ?? 0 }));
}

/**
 * Row indices per group, in order of first appearance, for the
 * one-dimensional view. Missing or mismatched groups give one "All rows" group.
 */
export function indicesByGroup(count: number, groups: readonly string[] | undefined): [string, number[]][] {
  const out = new Map<string, number[]>();
  for (let i = 0; i < count; i++) {
    const g = groups && groups.length === count ? groups[i] ?? '(Missing)' : 'All rows';
    const list = out.get(g);
    if (list) list.push(i);
    else out.set(g, [i]);
  }
  return [...out];
}
