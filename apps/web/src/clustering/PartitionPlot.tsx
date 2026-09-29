import { useState, type ReactElement } from 'react';
import type { ClusterFitResponse } from '@archaeodash/client';
const palette = ['#0072B2', '#D55E00', '#009E73', '#CC79A7', '#E69F00', '#56B4E9', '#7B3294', '#444444'];

/** Plot only finite coordinates, keeping identities out of the DOM. */
export function PartitionPlot({ result }: { result: ClusterFitResponse }): ReactElement {
  const [mode, setMode] = useState<'clusters' | 'groups'>('clusters');
  const groupsAvailable = result.plot_groups?.length === result.n_rows;
  const groups = mode === 'groups' && groupsAvailable;
  const coordinates = groups ? result.plot_coordinates : result.cluster_plot_coordinates ?? result.plot_coordinates;
  const names = groups ? result.plot_column_names : result.cluster_plot_column_names ?? result.plot_column_names;
  const labels = groups ? result.plot_groups! : result.cluster?.map(c => `Cluster ${c}`) ?? [];
  if (!coordinates?.length || !result.cluster || coordinates.length !== result.n_rows || labels.length !== result.n_rows) return <p>Plot coordinates are unavailable for this result.</p>;
  const points = coordinates.flatMap(([x, y], i) => Number.isFinite(x) && Number.isFinite(y) ? [{ x, y, row: i + 1, label: labels[i] || 'Unlabeled', cluster: result.cluster![i]! }] : []);
  const unique = [...new Set(points.map(p => p.label))];
  const xs = points.map(p => p.x), ys = points.map(p => p.y);
  const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
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
      {points.map(p => <g key={p.row} transform={`translate(${sx(p.x)} ${sy(p.y)})`} fill={palette[unique.indexOf(p.label) % palette.length]} stroke="currentColor" strokeWidth={0.4}>
        <title>{`Analytical unit ${p.row}; ${p.label}; cluster ${p.cluster}`}</title>
        {!groups || p.cluster % 3 === 1 ? <circle r={4} /> : p.cluster % 3 === 2 ? <rect x={-4} y={-4} width={8} height={8} /> : <path d="M0 -5L5 4L-5 4Z" />}
      </g>)}
    </svg>
    <ul aria-label="Plot legend">{unique.map((label, i) => <li key={label}><span aria-hidden="true" style={{ color: palette[i % palette.length] }}>● </span>{label}</li>)}</ul>
  </figure>;
}
