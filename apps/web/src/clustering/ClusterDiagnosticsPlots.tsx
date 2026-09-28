import type { ReactElement } from 'react';
import type { ClusterDiagnosticsResponse } from '@archaeodash/client';

type PlotPoint = { k: number; value: number };
const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);

function pointsFor(values: unknown, firstK: number): PlotPoint[] {
  if (!Array.isArray(values)) return [];
  return values.flatMap((value, index) => finite(value) ? [{ k: firstK + index, value }] : []);
}

function segmentsFor(values: unknown, firstK: number): PlotPoint[][] {
  if (!Array.isArray(values)) return [];
  const segments: PlotPoint[][] = [];
  let segment: PlotPoint[] = [];
  values.forEach((value, index) => {
    if (finite(value)) segment.push({ k: firstK + index, value });
    else if (segment.length) { segments.push(segment); segment = []; }
  });
  if (segment.length) segments.push(segment);
  return segments;
}

function format(value: number): string {
  const precise = value.toPrecision(4);
  const compact = Number(precise);
  return Number.isFinite(compact) ? compact.toString() : precise;
}

function DiagnosticPlot({
  title, label, points, segments, unavailable,
}: {
  title: string;
  label: string;
  points: PlotPoint[];
  segments: PlotPoint[][];
  unavailable: string;
}): ReactElement {
  const width = 440;
  const height = 260;
  const left = 72;
  const right = 18;
  const top = 20;
  const bottom = 44;
  const plotWidth = width - left - right;
  const plotHeight = height - top - bottom;
  const xs = points.map(point => point.k);
  const ys = points.map(point => point.value);
  const minX = Math.min(...xs);
  const maxX = Math.max(...xs);
  const magnitude = Math.max(...ys.map(value => Math.abs(value))) || 1;
  const normalized = ys.map(value => value / magnitude);
  const minY = Math.min(...normalized);
  const maxY = Math.max(...normalized);
  const xSpan = maxX - minX || 1;
  const rawYSpan = maxY - minY;
  const yPadding = rawYSpan === 0 ? Math.max(Math.abs(minY) * 0.2, 0.2) : 0;
  const domainMinY = minY - yPadding;
  const domainMaxY = maxY + yPadding;
  const ySpan = domainMaxY - domainMinY;
  const x = (k: number) => left + ((k - minX) / xSpan) * plotWidth;
  const y = (value: number) => top + ((domainMaxY - value / magnitude) / ySpan) * plotHeight;
  const ticks = minX === maxX ? [minX] : [minX, Math.round((minX + maxX) / 2), maxX]
    .filter((value, index, all) => all.indexOf(value) === index);
  const middleRawY = Math.min(...ys) / 2 + Math.max(...ys) / 2;
  const yTicks = [Math.min(...ys), middleRawY, Math.max(...ys)]
    .filter((value, index, all) => all.indexOf(value) === index);

  return <figure className="cluster-diagnostic-plot">
    <figcaption>{title}</figcaption>
    {points.length === 0 ? <p className="muted" role="status">{unavailable}</p> : <svg
      viewBox={`0 0 ${width} ${height}`} role="img" aria-label={title} className="cluster-diagnostic-svg"
    >
      <line x1={left} y1={top} x2={left} y2={top + plotHeight} stroke="currentColor" />
      <line x1={left} y1={top + plotHeight} x2={width - right} y2={top + plotHeight} stroke="currentColor" />
      <text x={left + plotWidth / 2} y={height - 7} textAnchor="middle" fill="currentColor" fontSize="12">Number of clusters (k)</text>
      <text transform={`translate(18 ${top + plotHeight / 2}) rotate(-90)`} textAnchor="middle" fill="currentColor" fontSize="12">{label === 'Within-cluster sum of squares (WSS)' ? 'WSS' : label}</text>
      {yTicks.map(tick => {
        const normalizedTick = tick / magnitude;
        const tickY = top + ((domainMaxY - normalizedTick) / ySpan) * plotHeight;
        return <g key={tick}>
          <line x1={left - 4} y1={tickY} x2={left} y2={tickY} stroke="currentColor" />
          <text x={left - 8} y={tickY + 4} textAnchor="end" fill="currentColor" fontSize="12">{format(tick)}</text>
        </g>;
      })}
      {ticks.map(tick => <g key={tick}>
        <line x1={x(tick)} y1={top + plotHeight} x2={x(tick)} y2={top + plotHeight + 4} stroke="currentColor" />
        <text x={x(tick)} y={top + plotHeight + 18} textAnchor="middle" fill="currentColor" fontSize="12">{tick}</text>
      </g>)}
      {segments.map((segment, index) => segment.length > 1 && <path
        key={index}
        d={segment.map((point, pointIndex) => `${pointIndex ? 'L' : 'M'} ${x(point.k)} ${y(point.value)}`).join(' ')}
        fill="none" stroke="var(--cluster-diagnostic-line, currentColor)" strokeWidth="2"
      />)}
      {points.map(point => <circle
        key={point.k} data-k={point.k} cx={x(point.k)} cy={y(point.value)} r="4"
        fill="var(--cluster-diagnostic-point, currentColor)"
      ><title>{`k=${point.k}, ${label.toLowerCase()}=${format(point.value)}`}</title></circle>)}
    </svg>}
  </figure>;
}

export function ClusterDiagnosticsPlots({ result }: { result: ClusterDiagnosticsResponse }): ReactElement {
  const wss = pointsFor(result?.wss, 1);
  const silhouette = pointsFor(result?.silhouette, 2);
  return <div className="cluster-diagnostics-plots" aria-label="Cluster diagnostic plots">
    <DiagnosticPlot title="Elbow plot" label="Within-cluster sum of squares (WSS)" points={wss}
      segments={segmentsFor(result?.wss, 1)} unavailable="WSS values are unavailable for plotting." />
    <DiagnosticPlot title="Mean silhouette plot" label="Mean silhouette width" points={silhouette}
      segments={segmentsFor(result?.silhouette, 2)} unavailable="Mean silhouette values are unavailable for plotting." />
  </div>;
}
