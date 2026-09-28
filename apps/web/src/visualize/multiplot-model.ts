/**
 * Multiplot model (Section 9.4): deterministic sampling for the interactive
 * 100,000-point ceiling, disjoint X/Y pair planning, and plot-save helpers.
 *
 * Parity class E (procedure 13): when a plot exceeds the ceiling the sampled
 * index set must be deterministic — a fixed stride from index 0, so the same
 * dataset always renders the same subset and the UI labels that sampling
 * happened (`sampled: true` with the stride).
 */

/** Legacy interactive ceiling: at most 100,000 points per interactive panel. */
export const INTERACTIVE_POINT_CEILING = 100_000;

export interface SamplingPlan {
  /** Row indices to draw, ascending and deterministic. */
  readonly indices: number[];
  /** True when the ceiling forced sampling (the UI must label this). */
  readonly sampled: boolean;
  /** Stride between kept rows (1 when unsampled). */
  readonly stride: number;
}

export function samplingPlan(n: number, ceiling = INTERACTIVE_POINT_CEILING): SamplingPlan {
  if (!Number.isFinite(n) || n <= 0) return { indices: [], sampled: false, stride: 1 };
  if (n <= ceiling) {
    return { indices: Array.from({ length: n }, (_, i) => i), sampled: false, stride: 1 };
  }
  const stride = Math.ceil(n / ceiling);
  const indices: number[] = [];
  for (let i = 0; i < n; i += stride) indices.push(i);
  return { indices, sampled: true, stride };
}

/** One scatter panel of a multiplot grid. */
export interface PlotPair {
  readonly xIndex: number;
  readonly yIndex: number;
}

/**
 * All disjoint X/Y pairs across the given column count, in column order
 * (legacy multiplot: for x < y panels). An empty column set yields no pairs.
 */
export function allPairs(columnCount: number): PlotPair[] {
  const pairs: PlotPair[] = [];
  for (let y = 0; y < columnCount; y++) {
    for (let x = 0; x < columnCount; x++) {
      if (x !== y) pairs.push({ xIndex: x, yIndex: y });
    }
  }
  return pairs;
}

/**
 * Human-readable sampling status for the interactive ceiling (Section 3.2:
 * preserve the 100k ceiling but display the deterministic sampling
 * status/count). Returns null when nothing was dropped.
 */
export function samplingStatusText(plan: SamplingPlan, total: number): string | null {
  if (!plan.sampled) return null;
  return `Sampled ${plan.indices.length} of ${total} points (stride ${plan.stride}, deterministic)`;
}

/**
 * Extract the selection uuids from a `plotly_selected` event: each point
 * carries `[analytical_uuid]` in `customdata` and the uuid is never rendered.
 */
export function uuidsFromSelectedEvent(eventData: unknown): string[] {
  const points = (eventData as { points?: { customdata?: unknown[] }[] } | null)?.points ?? [];
  return points
    .map((p) => (Array.isArray(p.customdata) ? String(p.customdata[0]) : ''))
    .filter((u) => u !== '');
}

/**
 * Static SVG scatter serialization for plot save: the browser stringifies
 * the live panel SVG; tests pass the element through `XMLSerializer`-shaped
 * `{ serializeToString }`. Returns a data URL safe to anchor-download.
 */
export function svgToDataUrl(
  svg: { serializeToString: (el: Element) => string },
  element: Element,
): string {
  const xml = svg.serializeToString(element);
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(xml)}`;
}
