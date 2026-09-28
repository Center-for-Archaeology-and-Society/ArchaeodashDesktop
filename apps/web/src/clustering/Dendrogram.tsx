import { useId, type ReactElement } from 'react';
import type { ClusterFitResponse } from '@archaeodash/client';
import { layoutDendrogram } from './dendrogram-model.ts';

const colors = [
  'var(--chart-1, #2563eb)', 'var(--chart-2, #c2410c)', 'var(--chart-3, #15803d)',
  'var(--chart-4, #a21caf)', 'var(--chart-5, #a16207)', 'var(--chart-6, #0e7490)',
  'var(--chart-7, #be123c)', 'var(--chart-8, #4f46e5)',
];

/** Accessible horizontal hclust dendrogram for Ward.D2 and DIANA fits. */
export function Dendrogram({ result, cutK = 2, leafSize = 12 }: {
  result: ClusterFitResponse;
  cutK?: number;
  leafSize?: number;
}): ReactElement {
  const id = useId();
  const layout = layoutDendrogram(result, cutK);
  if (!layout) return <figure aria-label="Dendrogram unavailable"><figcaption>Dendrogram unavailable for this result.</figcaption></figure>;
  const { width, height, plotLeft, plotRight, plotTop, plotBottom } = layout;
  const mid = (layout.minHeight + layout.maxHeight) / 2;
  const ticks = [...new Set([layout.minHeight, mid, layout.maxHeight])];
  const scaleX = (v: number) => layout.maxHeight === layout.minHeight
    ? plotRight
    : plotRight - ((v - layout.minHeight) / (layout.maxHeight - layout.minHeight)) * (plotRight - plotLeft);
  const size = Number.isFinite(leafSize) ? Math.max(8, Math.min(20, leafSize)) : 12;
  return <figure aria-label={`${result.method === 'diana' ? 'DIANA' : 'Ward.D2'} dendrogram`}>
    <figcaption>{result.method === 'diana' ? 'DIANA' : 'Ward.D2'} hierarchical clustering; cut into {cutK} clusters</figcaption>
      <svg role="img" aria-labelledby={`${id}-title ${id}-desc`} viewBox={`0 0 ${width} ${height}`} width={width} height={height} style={{ display: 'block', width: '100%', height: 'auto', minWidth: width, maxWidth: 'none', font: 'inherit', color: 'inherit' }}>
        <title id={`${id}-title`}>Hierarchical clustering dendrogram</title>
        <desc id={`${id}-desc`}>Horizontal dendrogram with merge height increasing from right to left. Leaves are analytical units in dendrogram order.</desc>
        <line x1={plotLeft} y1={plotBottom} x2={plotRight} y2={plotBottom} stroke="currentColor" />
        {ticks.map(tick => <g key={tick}>
          <line x1={scaleX(tick)} y1={plotBottom} x2={scaleX(tick)} y2={plotBottom + 5} stroke="currentColor" />
          <text x={scaleX(tick)} y={plotBottom + 20} textAnchor="middle" fill="currentColor" fontSize={11}>{Number.isFinite(Number(tick.toPrecision(4))) ? Number(tick.toPrecision(4)) : tick.toPrecision(4)}</text>
        </g>)}
        <text x={(plotLeft + plotRight) / 2} y={height - 16} textAnchor="middle" fill="currentColor" fontSize={12}>Merge height</text>
        <text x={16} y={(plotTop + plotBottom) / 2} textAnchor="middle" transform={`rotate(-90 16 ${(plotTop + plotBottom) / 2})`} fill="currentColor" fontSize={12}>Leaf order</text>
        {layout.segments.map((s, i) => <line key={i} x1={s.x1} y1={s.y1} x2={s.x2} y2={s.y2} stroke={s.group == null ? 'currentColor' : colors[s.group % colors.length]} strokeWidth={1.4} />)}
        {layout.leaves.map(({ row, y, group }) => <text key={row} x={plotRight + 8} y={y} dominantBaseline="middle" fill="currentColor" fontSize={size}>
          <tspan fill={colors[group % colors.length]}>● </tspan>Analytical unit {row} · Cluster {group + 1}
        </text>)}
      </svg>
  </figure>;
}
