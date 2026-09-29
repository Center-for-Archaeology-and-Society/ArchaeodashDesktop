/**
 * Multiplot model (Section 9.4): legacy R group/facet slice-head sampling for
 * the interactive 100,000-point ceiling, disjoint pair planning, and save
 * helpers.
 */

/** Legacy interactive ceiling: at most 100,000 points across the rendered grid. */
export const INTERACTIVE_POINT_CEILING = 100_000;

export interface SamplingPlan {
  /** Source-array row indices to draw, in source order. */
  readonly indices: number[];
  /** True when the interactive ceiling dropped candidate points. */
  readonly sampled: boolean;
  readonly candidateCount: number;
  readonly selectedCount: number;
  readonly perGroupFacet: number;
  readonly errorText: string | null;
}

export function samplingPlan(
  rowIndices: readonly number[],
  groupLabels: readonly string[],
  renderedFacetCount: number,
  facetGridBudget: number,
  ceiling = INTERACTIVE_POINT_CEILING,
): SamplingPlan {
  if (!Number.isSafeInteger(renderedFacetCount) || renderedFacetCount < 0 ||
      !Number.isSafeInteger(facetGridBudget) || facetGridBudget < renderedFacetCount ||
      !Number.isSafeInteger(ceiling) || ceiling < 1) {
    throw new RangeError('Invalid multiplot sampling dimensions');
  }
  if (renderedFacetCount === 0 || rowIndices.length === 0) {
    return {
      indices: [],
      sampled: false,
      candidateCount: 0,
      selectedCount: 0,
      perGroupFacet: 0,
      errorText: null,
    };
  }
  const counts = new Map<string, number>();
  for (const row of rowIndices) {
    const group = groupLabels[row] ?? 'All';
    counts.set(group, (counts.get(group) ?? 0) + 1);
  }
  const groupCount = Math.max(1, counts.size);
  const rawBudget = Math.floor(ceiling / (facetGridBudget * groupCount));
  const aggregateBound = Math.floor(ceiling / (renderedFacetCount * groupCount));
  if (rawBudget < 1 || aggregateBound < 1) {
    return {
      indices: [],
      sampled: true,
      candidateCount: rowIndices.length * renderedFacetCount,
      selectedCount: 0,
      perGroupFacet: 0,
      errorText: 'Too many groups or facets for the 100,000-point interactive limit. Reduce the selected groups or predictors.',
    };
  }
  // R uses max(25, rawBudget). Clamp that floor to the stricter rendered-grid
  // bound so unusually large facet/group grids can never exceed 100,000.
  const perGroupFacet = Math.min(Math.max(25, rawBudget), aggregateBound);
  const kept = new Map<string, number>();
  const indices: number[] = [];
  for (const row of rowIndices) {
    const group = groupLabels[row] ?? 'All';
    const count = kept.get(group) ?? 0;
    if (count < perGroupFacet) {
      indices.push(row);
      kept.set(group, count + 1);
    }
  }
  const selectedRows = [...kept.values()].reduce((n, count) => n + count, 0);
  const candidateCount = rowIndices.length * renderedFacetCount;
  const selectedCount = selectedRows * renderedFacetCount;
  return {
    indices,
    sampled: selectedCount < candidateCount,
    candidateCount,
    selectedCount,
    perGroupFacet,
    errorText: null,
  };
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
export function samplingStatusText(plan: SamplingPlan): string | null {
  if (plan.errorText !== null) return plan.errorText;
  if (!plan.sampled) return null;
  return `Sampled ${plan.selectedCount.toLocaleString()} of ${plan.candidateCount.toLocaleString()} interactive points (up to ${plan.perGroupFacet.toLocaleString()} per group and facet)`;
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
