/**
 * Multiplot (Section 9.4): grid of pairwise X/Y scatters with disjoint
 * selectors, point size, panel height 500–2000, static/interactive mode,
 * legacy per-group/per-facet 100k-point interactive sampling (labeled per procedure 13),
 * progressive render with cancel, and plot save (SVG download in static
 * mode; Plotly toImage SVG export in interactive mode; the uuid stays
 * internal to selection only).
 */
import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import type { Data, Layout } from 'plotly.js-dist-min';
import {
  allPairs,
  samplingPlan,
  samplingStatusText,
  svgToDataUrl,
  uuidsFromSelectedEvent,
  type PlotPair,
  type SamplingPlan,
} from './multiplot-model.ts';
import { colorFor } from './visualize-model.ts';

export const MIN_HEIGHT = 500;
export const MAX_HEIGHT = 2000;
export const PANEL_CHUNK = 12;
/** Panel width shared by the static SVG and interactive Plotly renders. */
export const PANEL_WIDTH = 340;

export type MultiplotMode = 'static' | 'interactive';

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
  /** analytical_uuid per row (customdata for selection; never rendered). */
  readonly rowUuids?: readonly string[];
  /** Selection callback shared with the single-plot path (optional). */
  readonly onSelect?: (uuids: readonly string[]) => void;
  /** Initial render mode; tests use 'interactive' to prove SSR safety. */
  readonly initialMode?: MultiplotMode;
}

export interface PanelTrace {
  readonly name: string;
  readonly color: string;
  readonly points: { readonly x: number; readonly y: number; readonly row: number }[];
}

/** Pure panel geometry: sampled points bucketed per group. */
export function panelPoints(
  props: Pick<MultiplotProps, 'values' | 'groupLabels' | 'groupNames'>,
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

function downloadAnchor(name: string, href: string): void {
  const a = document.createElement('a');
  a.href = href;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
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

type PlotlyModule = typeof import('plotly.js-dist-min');

/**
 * Interactive Plotly panel (scattergl per group): `plotly.js-dist-min` is
 * dynamically imported inside the layout effect so `renderToString` never
 * touches `window`. Selection is keyed internally by `analytical_uuid`
 * carried in `customdata`; hover shows the group only, never the uuid.
 */
export function InteractivePanel({
  xLabel,
  yLabel,
  width,
  height,
  pointSize,
  traces,
  pair,
  rowUuids,
  onSelect,
}: {
  xLabel: string;
  yLabel: string;
  width: number;
  height: number;
  pointSize: number;
  traces: readonly PanelTrace[];
  pair: PlotPair;
  rowUuids: readonly string[];
  onSelect?: (uuids: readonly string[]) => void;
}): ReactElement {
  const holder = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!holder.current) return;
    let disposed = false;
    void (async () => {
      const Plotly = (await import('plotly.js-dist-min')) as PlotlyModule;
      if (disposed || !holder.current) return;
      const data: Data[] = traces.map((t) => ({
        type: 'scattergl',
        mode: 'markers',
        name: t.name,
        x: t.points.map((p) => p.x),
        y: t.points.map((p) => p.y),
        // customdata[0] is the analytical_uuid for selection; never rendered.
        customdata: t.points.map((p) => [rowUuids[p.row] ?? '', t.name]),
        // Hover shows the group only, never the uuid.
        hovertemplate: `${t.name}<extra></extra>`,
        marker: { color: t.color, size: pointSize },
      } as Data));
      const layout = {
        dragmode: 'lasso',
        width,
        height,
        xaxis: { title: { text: xLabel } },
        yaxis: { title: { text: yLabel } },
        showlegend: traces.length > 1,
      } as Layout;
      const config = { responsive: false, displayModeBar: true } as const;
      void Plotly.react(holder.current, data, layout, config).then(() => {
        if (disposed || !holder.current) return;
        holder.current.dataset.renderState = 'complete';
        const gd = holder.current as unknown as {
          on: (event: string, cb: (eventData: unknown) => void) => void;
          removeAllListeners?: (event: string) => void;
        };
        gd.removeAllListeners?.('plotly_selected');
        gd.removeAllListeners?.('plotly_doubleclick');
        gd.on('plotly_selected', (eventData: unknown) => {
          onSelect?.(uuidsFromSelectedEvent(eventData));
        });
        gd.on('plotly_doubleclick', () => onSelect?.([]));
      });
    })().catch(() => {
      if (!disposed && holder.current) holder.current.dataset.renderState = 'error';
    });
    return () => {
      disposed = true;
    };
  }, [traces, xLabel, yLabel, width, height, pointSize, rowUuids, onSelect]);

  return (
    <div
      ref={holder}
      className="multiplot-panel-plotly"
      data-render-state="loading"
      data-pair={`${pair.xIndex}-${pair.yIndex}`}
      role="img"
      aria-label={`Scatter of ${yLabel} by ${xLabel}`}
    />
  );
}

export function Multiplot(props: MultiplotProps): ReactElement {
  const [pointSize, setPointSize] = useState(4);
  const [height, setHeight] = useState(600);
  const [mode, setMode] = useState<MultiplotMode>(props.initialMode ?? 'static');
  const [renderCount, setRenderCount] = useState<number | null>(PANEL_CHUNK);
  const gridRef = useRef<HTMLDivElement | null>(null);

  const pairs = useMemo(() => allPairs(props.columns.length), [props.columns]);
  const plan: SamplingPlan = useMemo(
    () => samplingPlan(props.rowIndices, props.groupLabels, pairs.length, pairs.length),
    [props.rowIndices, props.groupLabels, pairs.length],
  );
  const visiblePairs = renderCount === null ? pairs : pairs.slice(0, renderCount);

  const savePlots = (): void => {
    const holder = gridRef.current;
    if (!holder) return;
    if (mode === 'static') {
      const serializer = new XMLSerializer();
      const panels = holder.querySelectorAll('svg.multiplot-panel');
      panels.forEach((svg, i) => {
        downloadAnchor(`multiplot_panel_${i + 1}.svg`, svgToDataUrl(serializer, svg));
      });
      return;
    }
    void (async () => {
      const Plotly = (await import('plotly.js-dist-min')) as PlotlyModule;
      const figures = holder.querySelectorAll('div.multiplot-panel-plotly');
      let i = 0;
      for (const fig of figures) {
        i += 1;
        const url = await Plotly.toImage(fig as unknown as Parameters<typeof Plotly.toImage>[0], {
          format: 'svg',
          width: PANEL_WIDTH,
          height,
        });
        downloadAnchor(`multiplot_panel_${i}.svg`, url);
      }
    })().catch(() => {
      /* plotly load failure leaves the existing plots untouched */
    });
  };

  const samplingStatus = mode === 'interactive' ? samplingStatusText(plan) : null;

  return (
    <div className="multiplot">
      <div className="explore-controls">
        <label>
          Render mode
          <select
            aria-label="Render mode"
            value={mode}
            onChange={(e) => setMode(e.target.value as MultiplotMode)}
          >
            <option value="static">Static (SVG)</option>
            <option value="interactive">Interactive (Plotly)</option>
          </select>
        </label>
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
      {samplingStatus !== null && <p role={plan.errorText === null ? 'status' : 'alert'}>{samplingStatus}</p>}
      {pairs.length === 0 && <p className="muted">Pick at least two predictors for a multiplot.</p>}
      <div className="multiplot-grid" ref={gridRef}>
        {visiblePairs.map((pair) => (
          <MultiplotPanel
            key={`${pair.xIndex}-${pair.yIndex}`}
            props={props}
            pair={pair}
            plan={plan}
            pointSize={pointSize}
            height={height}
            mode={mode}
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
  plan,
  pointSize,
  height,
  mode,
}: {
  props: MultiplotProps;
  pair: PlotPair;
  plan: SamplingPlan;
  pointSize: number;
  height: number;
  mode: MultiplotMode;
}): ReactElement {
  const { traces } = panelPoints(props, pair, mode === 'interactive' ? plan.indices : props.rowIndices);
  const xLabel = props.columns[pair.xIndex] ?? `X${pair.xIndex}`;
  const yLabel = props.columns[pair.yIndex] ?? `Y${pair.yIndex}`;
  if (mode === 'interactive') {
    return (
      <div className="multiplot-cell">
        <InteractivePanel
          pair={pair}
          xLabel={xLabel}
          yLabel={yLabel}
          width={PANEL_WIDTH}
          height={height}
          pointSize={pointSize}
          traces={traces}
          rowUuids={props.rowUuids ?? []}
          onSelect={props.onSelect}
        />
      </div>
    );
  }
  return (
    <div className="multiplot-cell">
      <StaticPanel
        pair={pair}
        xLabel={xLabel}
        yLabel={yLabel}
        width={PANEL_WIDTH}
        height={height}
        pointSize={pointSize}
        traces={traces}
      />
    </div>
  );
}
