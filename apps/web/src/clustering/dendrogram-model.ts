import type { ClusterFitResponse } from '@archaeodash/client';

export interface DendrogramSegment {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  group: number | null;
  /** Stands for a collapsed branch (many samples drawn as one leaf). */
  collapsed?: boolean;
}
/** One drawn leaf: a sample, or a collapsed branch of `size` samples. */
export interface DendrogramLeaf {
  /** The sample (1-based input row), or the branch's first sample in leaf order. */
  row: number;
  y: number;
  /** Cut cluster (0-based), or null when a collapsed branch spans several. */
  group: number;
  size: number;
}
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
  /** Cut cluster (0-based) of every sample, indexed by input row - 1. */
  clusterOfRow: number[];
  /** True when the drawing collapses lower branches (see `FULL_LEAVES`). */
  truncated: boolean;
}

/** Trees up to this many samples draw every sample as a leaf. */
export const FULL_LEAVES = 1000;
/** Larger trees draw their top branches: at least this many leaves (more
 *  when the cut asks for more clusters, up to `FULL_LEAVES`). */
export const SHOWN_BRANCHES = 200;

/** Build an hclust-compatible tree. Leaves are negative row ordinals; positive
 * references point to an earlier merge, numbered from one. Linear in the
 * number of samples, so trees of 100,000 samples lay out quickly. */
export function layoutDendrogram(result: Pick<ClusterFitResponse, 'n_rows' | 'merge' | 'height' | 'order'>, cutK = 2): DendrogramLayout | null {
  const n = result.n_rows;
  const merge = result.merge;
  const heights = result.height;
  const order = result.order;
  if (!Number.isInteger(n) || n < 2 || !Number.isInteger(cutK) || cutK < 1 || cutK > n || !Array.isArray(merge) || !Array.isArray(heights) || !Array.isArray(order) ||
      merge.length !== n - 1 || heights.length !== n - 1 || order.length !== n) return null;
  let minHeight = 0;
  let maxHeight = 0;
  for (const h of heights) {
    if (typeof h !== 'number' || !Number.isFinite(h) || h < 0) return null;
    if (h < minHeight) minHeight = h;
    if (h > maxHeight) maxHeight = h;
  }
  // Leaf position of each sample; `order` must be a permutation of 1..n.
  const positions = new Int32Array(n + 1).fill(-1);
  for (let i = 0; i < n; i++) {
    const row = order[i];
    if (typeof row !== 'number' || !Number.isInteger(row) || row < 1 || row > n || positions[row] !== -1) return null;
    positions[row] = i;
  }

  // Nodes: samples 1..n, merge i (0-based) is node n + i + 1. Validate the
  // topology (each node used once, children merged earlier, and every
  // branch's samples contiguous in leaf order) with per-node spans.
  const nodes = 2 * n;
  const lo = new Int32Array(nodes);
  const hi = new Int32Array(nodes);
  const count = new Int32Array(nodes);
  const used = new Uint8Array(nodes);
  for (let row = 1; row <= n; row++) {
    lo[row] = positions[row]!;
    hi[row] = positions[row]!;
    count[row] = 1;
  }
  const children = new Int32Array(2 * (n - 1));
  for (let i = 0; i < n - 1; i++) {
    const pair = merge[i];
    if (!Array.isArray(pair) || pair.length !== 2) return null;
    const id = n + i + 1;
    let first = 0;
    for (let side = 0; side < 2; side++) {
      const ref = pair[side];
      if (typeof ref !== 'number' || !Number.isInteger(ref) || ref === 0 || ref < -n || ref > i) return null;
      const child = ref < 0 ? -ref : n + ref;
      if (used[child]) return null;
      used[child] = 1;
      children[2 * i + side] = child;
      if (side === 0) first = child;
    }
    const second = children[2 * i + 1]!;
    if (first === second) return null;
    lo[id] = Math.min(lo[first]!, lo[second]!);
    hi[id] = Math.max(hi[first]!, hi[second]!);
    count[id] = count[first]! + count[second]!;
    if (hi[id]! - lo[id]! + 1 !== count[id]) return null;
  }
  if (count[nodes - 1] !== n) return null;

  // Replay merges with union-find: `roots(k)` gives each sample's branch
  // after all but the last k - 1 merges.
  const parent = new Int32Array(nodes);
  const find = (v: number): number => {
    let root = v;
    while (parent[root] !== root) root = parent[root]!;
    while (parent[v] !== root) {
      const next = parent[v]!;
      parent[v] = root;
      v = next;
    }
    return root;
  };
  const replay = (clusters: number) => {
    for (let v = 0; v < nodes; v++) parent[v] = v;
    for (let i = 0; i < n - clusters; i++) {
      const id = n + i + 1;
      parent[find(children[2 * i]!)] = id;
      parent[find(children[2 * i + 1]!)] = id;
    }
  };

  // Cut membership, numbered by first appearance in input row order.
  replay(cutK);
  const clusterOfRow = new Array<number>(n);
  const groupByRoot = new Map<number, number>();
  for (let row = 1; row <= n; row++) {
    const root = find(row);
    let group = groupByRoot.get(root);
    if (group === undefined) {
      group = groupByRoot.size;
      groupByRoot.set(root, group);
    }
    clusterOfRow[row - 1] = group;
  }
  // Group of every node: its samples' cluster when they share one.
  const nodeGroup = new Int32Array(nodes).fill(-1);
  for (let row = 1; row <= n; row++) nodeGroup[row] = clusterOfRow[row - 1]!;
  for (let i = 0; i < n - 1; i++) {
    const a = nodeGroup[children[2 * i]!]!, b = nodeGroup[children[2 * i + 1]!]!;
    nodeGroup[n + i + 1] = a === b ? a : -1;
  }
  const groupOf = (node: number) => (nodeGroup[node]! < 0 ? null : nodeGroup[node]!);

  // Drawn leaves: every sample, or the branches left after all but the last
  // shown - 1 merges, in leaf order.
  const shown = n <= FULL_LEAVES ? n : Math.min(n, Math.max(SHOWN_BRANCHES, Math.min(cutK, FULL_LEAVES)));
  const truncated = shown < n;
  replay(shown);
  const units: number[] = [];
  for (let i = 0; i < n; i++) {
    const root = find(order[i]!);
    if (lo[root] === i) units.push(root);
  }
  const left = 48, right = 400, top = 16, bottom = 72;
  const width = 1000, height = Math.max(280, shown * 24 + top + bottom);
  const plotLeft = left, plotRight = width - right, plotTop = top, plotBottom = height - bottom;
  const x = (v: number) => maxHeight === minHeight
    ? plotRight
    : plotRight - ((v - minHeight) / (maxHeight - minHeight)) * (plotRight - plotLeft);
  const y = (i: number) => plotTop + (i + 0.5) * (plotBottom - plotTop) / shown;
  const nodeY = new Float64Array(nodes).fill(Number.NaN);
  const nodeHeight = new Float64Array(nodes).fill(Number.NaN);
  const segments: DendrogramSegment[] = [];
  const leaves: DendrogramLeaf[] = units.map((unit, rank) => {
    nodeY[unit] = rank;
    // A collapsed branch starts at its own top merge height.
    nodeHeight[unit] = unit <= n ? 0 : heights[unit - n - 1]!;
    if (unit > n) {
      segments.push({ x1: x(0), y1: y(rank), x2: x(nodeHeight[unit]!), y2: y(rank), group: groupOf(unit), collapsed: true });
    }
    return { row: order[lo[unit]!]!, y: y(rank), group: groupOf(unit) ?? -1, size: count[unit]! };
  });
  for (let i = n - shown; i < n - 1; i++) {
    const id = n + i + 1;
    const a = children[2 * i]!, b = children[2 * i + 1]!;
    const mergeHeight = heights[i]!;
    const ay = nodeY[a]!, by = nodeY[b]!;
    if (Number.isNaN(ay) || Number.isNaN(by)) return null;
    segments.push(
      { x1: x(nodeHeight[a]!), y1: y(ay), x2: x(mergeHeight), y2: y(ay), group: groupOf(a) },
      { x1: x(nodeHeight[b]!), y1: y(by), x2: x(mergeHeight), y2: y(by), group: groupOf(b) },
      { x1: x(mergeHeight), y1: y(ay), x2: x(mergeHeight), y2: y(by), group: groupOf(id) },
    );
    nodeHeight[id] = mergeHeight;
    nodeY[id] = (ay + by) / 2;
  }
  return {
    width, height, plotTop, plotBottom, plotLeft, plotRight,
    segments, leaves, minHeight, maxHeight, clusterOfRow, truncated,
  };
}
