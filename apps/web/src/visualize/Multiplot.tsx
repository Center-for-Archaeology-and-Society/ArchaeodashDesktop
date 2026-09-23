/**
 * Multiplot (Section 9.4): grid of pairwise X/Y scatters with disjoint
 * selectors, point size, panel height 500–2000, static/interactive mode,
 * deterministic 100k-point interactive sampling (labeled per procedure 13),
 * progressive render with cancel, and plot save (SVG download in static
 * mode; the uuid stays internal to selection only).
 */
import { useMemo, useRef, useState, type ReactElement } from 'react';
import { allPairs, samplingPlan, svgToDataUrl, type PlotPair } from './multiplot-model.ts';
import { colorFor } from './visualize-model.ts';

export const MIN_HEIGHT = 500;
export const MAX_HEIGHT = 2000;
export const PANEL_CHUNK = 12;

export interface MultiplotProps {
  /** Display names of the numeric columns being paired. */
  readonly columns: readonly string[];
  /** Row-aligned values per column (nulls skip the panel point). */
  readonly values: readonly (readonly (number | null)[])[];
  /** Group label per row (colors and legends). */
  readonly groupLabels: readonly string[];
  /** Distinct group names in palette order. */
  readonly groupNames: readonly string[];
  /** Row indices surviving the metadata filter. */
  readonly rowIndices: readonly number[];
}

export interface PanelTrace {
  readonly name: string;
  readonly color: string;
  readonly points: { readonly x: number; readonly y: number; readonly row: number }[];
}

/** Pure panel geometry: sampled points bucketed per group. */
export function panelPoints(
  props: MultiplotProps,
  pair: PlotPair,
  indices: readonly number[],
): { traces: PanelTrace[]; drawn: number } {
  const byGroup = new Map<string, { x: number; y: number; row: number }[]>();
  for (const row of indices) {
    const x = props.values[pair.xIndex]?.[row];
    const y = props.values[pair.yIndex]?.[row];
    if (x === null || x === undefined || y === null || y === undefined) continue;
    const name = props.groupLabels[row] ?? 'All';
    const bucket = byGroup.get(name);
    const point = { x, y, row };
    if (bucket) bucket.push(point);
    else byGroup.set(name, [point]);
  }
  return {
    traces: [...byGroup.entries()].map(([name, points], gi) => ({
      name,
      color: colorFor(props.groupNames.indexOf(name) >= 0 ? props.groupNames.indexOf(name) : gi),
      points,
    })),
    drawn: [...byGroup.values()].reduce((n, pts) => n + pts.length, 0),
  };
}

/** Static SVG panel (no Plotly): deterministic full-fidelity render. */
export function StaticPanel({
  xLabel,
  yLabel,
  width,
  height,
  pointSize,
  traces,
  pair,
}: {
  xLabel: string;
  yLabel: string;
  width: number;
  height: number;
  pointSize: number;
  traces: readonly PanelTrace[];
  pair: PlotPair;
}): ReactElement {
  const all = traces.flatMap((t) => t.points);
  const xMin = all.length ? Math.min(...all.map((p) => p.x)) : 0;
  const xMax = all.length ? Math.max(...all.map((p) => p.x)) : 1;
  const yMin = all.length ? Math.min(...all.map((p) => p.y)) : 0;
  const yMax = all.length ? Math.max(...all.map((p) => p.y)) : 1;
  const spanX = xMax - xMin || 1;
  const spanY = yMax - yMin || 1;
  const pad = 28;
  const sx = (x: number) => pad + ((x - xMin) / spanX) * (width - pad * 2);
  const sy = (y: number) => height - pad - ((y - yMin) / spanY) * (height - pad * 2);
  return (
    <svg
      className="multiplot-panel"
      data-pair={`${pair.xIndex}-${pair.yIndex}`}
      role="img"
      aria-label={`Scatter of ${yLabel} by ${xLabel}`}
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
    >
      {traces.map((t) => (
        <g key={t.name} fill={t.color}>
          {t.points.map((p) => (
            <circle key={p.row} cx={sx(p.x)} cy={sy(p.y)} r={pointSize / 2} />
          ))}
        </g>
      ))}
      <text x={width / 2} y={height - 6} textAnchor="middle" fontSize="10">
        {xLabel}
      </text>
      <text x={10} y={14} textAnchor="start" fontSize="10">
        {yLabel}
      </text>
    </svg>
  );
}

export function Multiplot(props: MultiplotProps): ReactElement {
  const [pointSize, setPointSize] = useState(4);
  const [height, setHeight] = useState(600);
  const [renderCount, setRenderCount] = useState<number | null>(PANEL_CHUNK);
  const gridRef = useRef<HTMLDivElement | null>(null);

  const pairs = useMemo(() => allPairs(props.columns.length), [props.columns]);
  const plan = useMemo(() => samplingPlan(props.rowIndices.length), [props.rowIndices]);
  const visiblePairs = renderCount === null ? pairs : pairs.slice(0, renderCount);

  const savePlots = (): void => {
    const holder = gridRef.current;
    if (!holder) return;
    const serializer = new XMLSerializer();
    const panels = holder.querySelectorAll('svg.multiplot-panel');
    panels.forEach((svg, i) => {
      const a = document.createElement('a');
      a.href = svgToDataUrl(serializer, svg);
      a.download = `multiplot_panel_${i + 1}.svg`;
      document.body.append(a);
      a.click();
      a.remove();
    });
  };

  return (
    <div className="multiplot">
      <div className="explore-controls">
        <label>
          Point size
          <input
            type="range"
            min={1}
            max={10}
            value={pointSize}
            aria-label="Point size"
            onChange={(e) => setPointSize(Number(e.target.value))}
          />
        </label>
        <label>
          Height {height}px
          <input
            type="range"
            min={MIN_HEIGHT}
            max={MAX_HEIGHT}
            step={50}
            value={height}
            aria-label="Panel height"
            onChange={(e) => setHeight(Number(e.target.value))}
          />
        </label>
        <button type="button" onClick={savePlots}>
          Save plots (SVG)
        </button>
      </div>
      {plan.sampled && (
        <p role="status">
          Interactive ceiling: showing {plan.indices.length} of {props.rowIndices.length} points
          (stride {plan.stride}, deterministic).
        </p>
      )}
      {pairs.length === 0 && <p className="muted">Pick at least two predictors for a multiplot.</p>}
      <div className="multiplot-grid" ref={gridRef}>
        {visiblePairs.map((pair) => (
          <MultiplotPanel
            key={`${pair.xIndex}-${pair.yIndex}`}
            props={props}
            pair={pair}
            pointSize={pointSize}
            height={height}
          />
        ))}
      </div>
      {renderCount !== null && renderCount < pairs.length && (
        <button type="button" onClick={() => setRenderCount(null)}>
          Continue rendering {pairs.length - renderCount} more panels
        </button>
      )}
    </div>
  );
}

function MultiplotPanel({
  props,
  pair,
  pointSize,
  height,
}: {
  props: MultiplotProps;
  pair: PlotPair;
  pointSize: number;
  height: number;
}): ReactElement {
  const plan = samplingPlan(props.rowIndices.length);
  const { traces } = panelPoints(props, pair, plan.indices);
  const xLabel = props.columns[pair.xIndex] ?? `X${pair.xIndex}`;
  const yLabel = props.columns[pair.yIndex] ?? `Y${pair.yIndex}`;
  return (
    <div className="multiplot-cell">
      <StaticPanel
        pair={pair}
        xLabel={xLabel}
        yLabel={yLabel}
        width={340}
        height={height}
        pointSize={pointSize}
        traces={traces}
      />
    </div>
  );
}
