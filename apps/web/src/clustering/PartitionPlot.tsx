import { useState, type ReactElement } from 'react';
import type { ClusterFitResponse } from '@archaeodash/client';
import { groupColors } from '../visualize/visualize-model.ts';
import { extent } from '../visualize/multiplot-model.ts';

/** Ten marker shapes (circle, square, triangles, diamond, plus, cross, star, pentagon, hourglass). */
const SHAPES = 10;
export function Marker({ shape }: { shape: number }): ReactElement {
  switch (((shape % SHAPES) + SHAPES) % SHAPES) {
    case 0: return <circle r={4} />;
    case 1: return <rect x={-3.6} y={-3.6} width={7.2} height={7.2} />;
    case 2: return <path d="M0 -5L4.6 3.5L-4.6 3.5Z" />;
    case 3: return <path d="M0 -5L5 0L0 5L-5 0Z" />;
    case 4: return <path d="M-1.6 -5H1.6V-1.6H5V1.6H1.6V5H-1.6V1.6H-5V-1.6H-1.6Z" />;
    case 5: return <path d="M-4.5 -2.6L-2.6 -4.5L0 -1.9L2.6 -4.5L4.5 -2.6L1.9 0L4.5 2.6L2.6 4.5L0 1.9L-2.6 4.5L-4.5 2.6L-1.9 0Z" />;
    case 6: return <path d="M0 -5.2L1.5 -1.6L5 -1.6L2.2 0.7L3.2 4.4L0 2.3L-3.2 4.4L-2.2 0.7L-5 -1.6L-1.5 -1.6Z" />;
    case 7: return <path d="M0 -5L4.8 -1.5L2.9 4L-2.9 4L-4.8 -1.5Z" />;
    case 8: return <path d="M0 5L4.6 -3.5L-4.6 -3.5Z" />;
    default: return <path d="M-4.5 -4.5H4.5L-4.5 4.5H4.5Z" />;
  }
}

/** Plot only finite coordinates, keeping identities out of the DOM. */
export function PartitionPlot({ result, labelOf = (row: number) => `Analytical unit ${row}` }: {
  result: ClusterFitResponse;
  /** Sample label (ANID) for a 1-based input row. */
  labelOf?: (row: number) => string;
}): ReactElement {
  const [mode, setMode] = useState<'clusters' | 'groups'>('clusters');
  const groupsAvailable = result.plot_groups?.length === result.n_rows;
  const groups = mode === 'groups' && groupsAvailable;
  const coordinates = groups ? result.plot_coordinates : result.cluster_plot_coordinates ?? result.plot_coordinates;
  const names = groups ? result.plot_column_names : result.cluster_plot_column_names ?? result.plot_column_names;
  const labels = groups ? result.plot_groups! : result.cluster?.map(c => `Cluster ${c}`) ?? [];
  if (!coordinates?.length || !result.cluster || coordinates.length !== result.n_rows || labels.length !== result.n_rows) return <p>Plot coordinates are unavailable for this result.</p>;
  const points = coordinates.flatMap(([x, y], i) => Number.isFinite(x) && Number.isFinite(y) ? [{ x, y, row: i + 1, label: labels[i] || 'Unlabeled', cluster: result.cluster![i]! }] : []);
  const unique = [...new Set(points.map(p => p.label))];
  // Viridis colors spread over the labels; past ten labels the shape also
  // changes, so up to 100 labels stay distinguishable. With existing groups
  // as colors, the shape marks the cluster instead.
  const colors = groupColors(unique.length);
  const colorOf = (label: string) => colors[unique.indexOf(label)] ?? '#444444';
  const shapeOf = (label: string, cluster: number) => (groups ? cluster - 1 : Math.floor(unique.indexOf(label) / SHAPES));
  // No spread into Math.min/max: WebKit caps arguments near 65k rows.
  const [minX, maxX] = extent(points.map(p => p.x)) ?? [0, 0];
  const [minY, maxY] = extent(points.map(p => p.y)) ?? [0, 0];
  const sx = (x: number) => 65 + (maxX === minX ? 0.5 : (x - minX) / (maxX - minX)) * 540;
  const sy = (y: number) => 355 - (maxY === minY ? 0.5 : (y - minY) / (maxY - minY)) * 305;
  return <figure aria-label="Partition cluster plot">
    <label>Color points by <select value={mode} onChange={e => setMode(e.target.value as 'clusters' | 'groups')}><option value="clusters">New clusters</option><option value="groups" disabled={!groupsAvailable}>Existing groups</option></select></label>
    {!groups && result.plot_warning && <p role="status">{result.plot_warning}</p>}
    <figcaption>{result.method === 'pam' ? 'k-medoids' : 'k-means'}: {groups ? 'color by existing group, symbol by cluster' : 'color by new cluster'}. {names?.join(' / ')}</figcaption>
    <svg viewBox="0 0 660 410" role="img" aria-label="Analytical units plotted by two analysis dimensions" style={{ width: '100%', maxWidth: 850 }}>
      <line x1={65} y1={355} x2={605} y2={355} stroke="currentColor" /><line x1={65} y1={50} x2={65} y2={355} stroke="currentColor" />
      <text x={335} y={398} textAnchor="middle" fill="currentColor">{names?.[0] ?? 'Dimension 1'}</text>
      <text transform="translate(15 202) rotate(-90)" textAnchor="middle" fill="currentColor">{names?.[1] ?? 'Dimension 2'}</text>
      {[0, 0.5, 1].map(t => <g key={t} fill="currentColor" fontSize={11}><text x={65 + 540 * t} y={374} textAnchor="middle">{(minX + (maxX - minX) * t).toPrecision(4)}</text><text x={58} y={359 - 305 * t} textAnchor="end">{(minY + (maxY - minY) * t).toPrecision(4)}</text></g>)}
      {points.map(p => <g key={p.row} transform={`translate(${sx(p.x)} ${sy(p.y)})`} fill={colorOf(p.label)} stroke="currentColor" strokeWidth={0.4}>
        <title>{`${labelOf(p.row)}; ${p.label}; cluster ${p.cluster}`}</title>
        <Marker shape={shapeOf(p.label, p.cluster)} />
      </g>)}
    </svg>
    <ul aria-label="Plot legend" className="partition-legend">{unique.map((label, i) => <li key={label}>
      <svg viewBox="-6 -6 12 12" width={12} height={12} aria-hidden="true"><g fill={colors[i]} stroke="currentColor" strokeWidth={0.4}><Marker shape={groups ? 0 : Math.floor(i / SHAPES)} /></g></svg> {label}
    </li>)}</ul>
    {groups && <ul aria-label="Cluster symbols" className="partition-legend">{[...new Set(points.map(p => p.cluster))].sort((a, b) => a - b).map(c => <li key={c}>
      <svg viewBox="-6 -6 12 12" width={12} height={12} aria-hidden="true"><g fill="none" stroke="currentColor" strokeWidth={0.8}><Marker shape={c - 1} /></g></svg> Cluster {c}
    </li>)}</ul>}
  </figure>;
}
