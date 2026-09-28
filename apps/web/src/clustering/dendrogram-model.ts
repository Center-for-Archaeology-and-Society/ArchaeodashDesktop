import type { ClusterFitResponse } from '@archaeodash/client';

export interface DendrogramSegment { x1: number; y1: number; x2: number; y2: number; group: number | null }
export interface DendrogramLeaf { row: number; y: number; group: number }
export interface DendrogramLayout {
  width: number;
  height: number;
  plotTop: number;
  plotBottom: number;
  plotLeft: number;
  plotRight: number;
  segments: DendrogramSegment[];
  leaves: DendrogramLeaf[];
  minHeight: number;
  maxHeight: number;
}

/** Build an hclust-compatible tree. Leaves are negative row ordinals; positive
 * references point to an earlier merge, numbered from one. */
export function layoutDendrogram(result: Pick<ClusterFitResponse, 'n_rows' | 'merge' | 'height' | 'order'>, cutK = 2): DendrogramLayout | null {
  const n = result.n_rows;
  const merge = result.merge;
  const heights = result.height;
  const order = result.order;
  if (!Number.isInteger(n) || n < 2 || n > 1000 || !Number.isInteger(cutK) || cutK < 1 || cutK > n || !Array.isArray(merge) || !Array.isArray(heights) || !Array.isArray(order) ||
      merge.length !== n - 1 || heights.length !== n - 1 || order.length !== n ||
      heights.some(h => !Number.isFinite(h) || h < 0) || merge.some(pair => !Array.isArray(pair) || pair.length !== 2 || pair.some(ref => !Number.isInteger(ref) || ref === 0))) return null;
  const seenOrder = new Set(order);
  if (seenOrder.size !== n || order.some(i => !Number.isInteger(i) || i < 1 || i > n)) return null;

  const positions = new Map<number, number>();
  order.forEach((row, i) => positions.set(row, i));
  const nodeHeight = new Array<number>(2 * n).fill(Number.NaN);
  const nodeY = new Array<number>(2 * n).fill(Number.NaN);
  const used = new Set<number>();
  for (let row = 1; row <= n; row++) {
    nodeHeight[row] = 0;
    nodeY[row] = positions.get(row)!;
  }
  const segments: DendrogramSegment[] = [];
  const left = 48, right = 400, top = 16, bottom = 72;
  const width = 1000, height = Math.max(280, n * 24 + top + bottom);
  const plotLeft = left, plotRight = width - right, plotTop = top, plotBottom = height - bottom;
  const minHeight = Math.min(0, ...heights), maxHeight = Math.max(0, ...heights);
  const x = (v: number) => maxHeight === minHeight
    ? plotRight
    : plotRight - ((v - minHeight) / (maxHeight - minHeight)) * (plotRight - plotLeft);
  const y = (i: number) => plotTop + (i + 0.5) * (plotBottom - plotTop) / n;
  // Validate the full tree before replaying the cut; malformed parent links
  // must never reach the union/find traversal below.
  const topologyUsed = new Set<number>();
  const available = new Set<number>(Array.from({ length: n }, (_, i) => i + 1));
  const mergeLeaves: number[][] = new Array(2 * n);
  for (let row = 1; row <= n; row++) mergeLeaves[row] = [row];
  for (let i = 0; i < merge.length; i++) {
    const id = n + i + 1;
    for (const ref of merge[i]!) {
      const child = ref < 0 ? -ref : n + ref;
      if ((ref < 0 && -ref > n) || (ref > 0 && ref >= i + 1) || !available.has(child) || topologyUsed.has(child)) return null;
      topologyUsed.add(child);
    }
    if (merge[i]![0] === merge[i]![1]) return null;
    const children = merge[i]!.map(ref => ref < 0 ? [-ref] : mergeLeaves[n + ref]);
    if (!children[0] || !children[1]) return null;
    const span = [...children[0], ...children[1]].map(row => positions.get(row)!);
    if (Math.max(...span) - Math.min(...span) + 1 !== span.length) return null;
    mergeLeaves[id] = [...children[0], ...children[1]];
    available.add(id);
  }
  if (topologyUsed.size !== 2 * n - 2 || !available.has(2 * n - 1)) return null;
  // Replay just the first n-k merges to find the cut membership for each leaf.
  const parent = Array.from({ length: 2 * n }, (_, i) => i);
  const find = (v: number): number => parent[v]! === v ? v : (parent[v] = find(parent[v]!));
  for (let i = 0; i < n - cutK; i++) {
    const [l, r] = merge[i]!;
    const a = find(l < 0 ? -l : n + l), b = find(r < 0 ? -r : n + r);
    parent[a] = n + i + 1; parent[b] = n + i + 1; parent[n + i + 1] = n + i + 1;
  }
  const groupByRoot = new Map<number, number>();
  const leafGroups = new Map<number, number>();
  let nextGroup = 0;
  for (let row = 1; row <= n; row++) {
    const root = find(row);
    if (!groupByRoot.has(root)) groupByRoot.set(root, nextGroup++);
    leafGroups.set(row, groupByRoot.get(root)!);
  }
  const nodeGroup = new Array<number | null>(2 * n).fill(null);
  for (let row = 1; row <= n; row++) nodeGroup[row] = leafGroups.get(row)!;
  for (let i = 0; i < merge.length; i++) {
    const id = n + i + 1;
    const pair = merge[i]!;
    if (!Array.isArray(pair) || pair.length !== 2) return null;
    const childIds: number[] = [];
    for (const ref of pair) {
      if (!Number.isInteger(ref) || ref === 0) return null;
      const child = ref < 0 ? -ref : n + ref;
      if (ref < 0 && -ref > n || ref > 0 && ref >= id - n || ref > merge.length) return null;
      if (!Number.isFinite(nodeHeight[child]) || used.has(child)) return null;
      childIds.push(child);
      used.add(child);
    }
    if (childIds[0] === childIds[1]) return null;
    const [a, b] = childIds as [number, number];
    const mergeHeight = heights[i]!;
    const ay = nodeY[a]!, by = nodeY[b]!, my = (ay + by) / 2;
    const ga = nodeGroup[a]!, gb = nodeGroup[b]!;
    const mergeGroup = ga === gb ? ga : null;
    segments.push(
      { x1: x(nodeHeight[a]!), y1: y(ay), x2: x(mergeHeight), y2: y(ay), group: ga },
      { x1: x(nodeHeight[b]!), y1: y(by), x2: x(mergeHeight), y2: y(by), group: gb },
      { x1: x(mergeHeight), y1: y(ay), x2: x(mergeHeight), y2: y(by), group: mergeGroup },
    );
    nodeHeight[id] = mergeHeight;
    nodeY[id] = my;
    nodeGroup[id] = mergeGroup;
  }
  const root = n + merge.length;
  if (used.size !== 2 * n - 2 || !Number.isFinite(nodeHeight[root])) return null;
  return {
    width, height, plotTop, plotBottom, plotLeft, plotRight,
    segments, leaves: order.map(row => ({ row, y: y(positions.get(row)!), group: leafGroups.get(row)! })), minHeight, maxHeight,
  };
}
