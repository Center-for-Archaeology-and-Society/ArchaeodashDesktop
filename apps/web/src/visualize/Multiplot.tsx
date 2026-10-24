/**
 * Multiplot (Section 9.4): the legacy facet grid of X × Y scatters — rows
 * are the chosen Y columns, columns the chosen X columns, X = Y skipped —
 * with point size 0.1–10, whole-figure height 500–2000, static/interactive
 * mode, legacy per-group/per-facet 100k-point interactive sampling (labeled
 * per procedure 13), progressive drawing with stop/continue, and plot save
 * (SVG download in static mode; Plotly toImage SVG in interactive mode; the
 * uuid stays internal to selection only).
 */
import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import type { Data, Layout } from 'plotly.js-dist-min';
import {
  crossPairs,
  extent,
  niceTicks,
  samplingPlan,
  samplingStatusText,
  uuidsFromSelectedEvent,
  type PlotPair,
  type SamplingPlan,
} from './multiplot-model.ts';
import { colorFor, groupColors } from './visualize-model.ts';
import { plotTheme } from '../charts/PlotlyChart.tsx';
import { PlotSaveButtons, combineSvgParts, saveSvgPart, serializeSvg, svgFromDataUrl, type ImageFormat, type SvgPart } from '../ui/PlotSave.tsx';

export const MIN_HEIGHT = 500;
export const MAX_HEIGHT = 2000;
/** Legacy default figure height in pixels. */
export const DEFAULT_HEIGHT = 900;
export const PANEL_CHUNK = 12;
/** Panel width shared by the static SVG and interactive Plotly renders. */
export const PANEL_WIDTH = 340;
/** Smallest panel height when many Y rows share the figure height. */
export const MIN_PANEL_HEIGHT = 160;

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
  /** Color per group name (same order); defaults to the viridis spread. */
  readonly palette?: readonly string[];
  /** Row indices surviving the metadata filter. */
  readonly rowIndices: readonly number[];
  /** analytical_uuid per row (customdata for selection; never rendered). */
  readonly rowUuids?: readonly string[];
  /** Selection callback shared with the single-plot path (optional). */
  readonly onSelect?: (uuids: readonly string[]) => void;
  /** Initial render mode; tests use 'interactive' to prove SSR safety. */
  readonly initialMode?: MultiplotMode;
  /** Initial X / Y column names (default: every column on both axes). */
  readonly initialX?: readonly string[];
  readonly initialY?: readonly string[];
}

export interface PanelTrace {
  readonly name: string;
  readonly color: string;
  readonly points: { readonly x: number; readonly y: number; readonly row: number }[];
}

/** Pure panel geometry: sampled points bucketed per group. */
export function panelPoints(
  props: Pick<MultiplotProps, 'values' | 'groupLabels' | 'groupNames' | 'palette'>,
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
  // Same palette as the single plot, so a group keeps its color.
  const palette = props.palette ?? groupColors(props.groupNames.length);
  let drawn = 0;
  for (const pts of byGroup.values()) drawn += pts.length;
  return {
    traces: [...byGroup.entries()].map(([name, points], gi) => ({
      name,
      color: palette[props.groupNames.indexOf(name)] ?? colorFor(gi),
      points,
    })),
    drawn,
  };
}

function tickText(v: number): string {
  const a = Math.abs(v);
  if (a !== 0 && (a >= 1e5 || a < 1e-3)) return v.toExponential(1);
  return String(Number(v.toPrecision(4)));
}

/** Static SVG panel (no Plotly): deterministic full-fidelity render with axes. */
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
  function* coords(axis: 'x' | 'y') {
    for (const t of traces) for (const p of t.points) yield p[axis];
  }
  const [xMin, xMax] = extent(coords('x')) ?? [0, 1];
  const [yMin, yMax] = extent(coords('y')) ?? [0, 1];
  const spanX = xMax - xMin || 1;
  const spanY = yMax - yMin || 1;
  const left = 46;
  const right = 10;
  const top = 18;
  const bottom = 34;
  const sx = (x: number) => left + ((x - xMin) / spanX) * (width - left - right);
  const sy = (y: number) => height - bottom - ((y - yMin) / spanY) * (height - top - bottom);
  const xTicks = niceTicks(xMin, xMax, 4);
  const yTicks = niceTicks(yMin, yMax, 4);
  return (
    <svg
      className="multiplot-panel"
      data-pair={`${pair.xIndex}-${pair.yIndex}`}
      role="img"
      aria-label={`Scatter of ${yLabel} by ${xLabel}`}
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
      fontSize="10"
    >
      <rect x={left} y={top} width={width - left - right} height={height - top - bottom} fill="none" stroke="#999" strokeWidth="0.75" />
      {xTicks.map((t) => (
        <g key={`x${t}`}>
          <line x1={sx(t)} x2={sx(t)} y1={height - bottom} y2={height - bottom + 4} stroke="#777" />
          <text x={sx(t)} y={height - bottom + 14} textAnchor="middle" fill="currentColor">{tickText(t)}</text>
        </g>
      ))}
      {yTicks.map((t) => (
        <g key={`y${t}`}>
          <line x1={left - 4} x2={left} y1={sy(t)} y2={sy(t)} stroke="#777" />
          <text x={left - 6} y={sy(t) + 3} textAnchor="end" fill="currentColor">{tickText(t)}</text>
        </g>
      ))}
      {traces.map((t) => (
        <g key={t.name} fill={t.color} fillOpacity="0.66">
          {t.points.map((p) => (
            <circle key={p.row} cx={sx(p.x)} cy={sy(p.y)} r={pointSize} />
          ))}
        </g>
      ))}
      <text x={left + (width - left - right) / 2} y={height - 4} textAnchor="middle" fill="currentColor">
        {xLabel}
      </text>
      <text x={left} y={12} textAnchor="start" fill="currentColor">
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
  // The latest callback, so a new function identity never redraws the panel.
  const selectRef = useRef(onSelect);
  useEffect(() => {
    selectRef.current = onSelect;
  }, [onSelect]);

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
        marker: { color: t.color, size: pointSize * 2, opacity: 0.66 },
      } as Data));
      const theme = plotTheme();
      const layout = {
        ...theme,
        dragmode: 'lasso',
        width,
        height,
        xaxis: { ...(theme.xaxis as object), title: { text: xLabel } },
        yaxis: { ...(theme.yaxis as object), title: { text: yLabel } },
        showlegend: false,
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
          selectRef.current?.(uuidsFromSelectedEvent(eventData));
        });
        gd.on('plotly_doubleclick', () => selectRef.current?.([]));
      });
    })().catch(() => {
      if (!disposed && holder.current) holder.current.dataset.renderState = 'error';
    });
    return () => {
      disposed = true;
    };
  }, [traces, xLabel, yLabel, width, height, pointSize, rowUuids]);

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

/** Multi-select list of column names (native select, keyboard friendly). */
function ColumnPicker({
  label,
  columns,
  selected,
  onChange,
}: {
  label: string;
  columns: readonly string[];
  selected: readonly string[];
  onChange: (next: string[]) => void;
}): ReactElement {
  return (
    <label className="multiplot-picker">
      {label} ({selected.length})
      <select
        multiple
        aria-label={`${label} columns`}
        size={Math.min(6, Math.max(3, columns.length))}
        value={[...selected]}
        onChange={(e) => onChange(columns.filter((c) => [...e.target.selectedOptions].some((o) => o.value === c)))}
      >
        {columns.map((c) => (
          <option key={c} value={c}>
            {c}
          </option>
        ))}
      </select>
      <span className="multiplot-picker-actions">
        <button type="button" className="btn btn-sm" onClick={() => onChange([...columns])}>All</button>
        <button type="button" className="btn btn-sm" onClick={() => onChange([])}>None</button>
      </span>
    </label>
  );
}

export function Multiplot(props: MultiplotProps): ReactElement {
  const [pointSize, setPointSize] = useState(2);
  const [height, setHeight] = useState(DEFAULT_HEIGHT);
  const [mode, setMode] = useState<MultiplotMode>(props.initialMode ?? 'static');
  const [xChoice, setXChoice] = useState<readonly string[] | null>(props.initialX ?? null);
  const [yChoice, setYChoice] = useState<readonly string[] | null>(props.initialY ?? null);
  const [renderCount, setRenderCount] = useState(PANEL_CHUNK);
  const [drawing, setDrawing] = useState(true);
  const gridRef = useRef<HTMLDivElement | null>(null);

  // Unset choices follow the available columns (all on both axes).
  const xCols = useMemo(() => (xChoice ?? props.columns).filter((c) => props.columns.includes(c)), [xChoice, props.columns]);
  const yCols = useMemo(() => (yChoice ?? props.columns).filter((c) => props.columns.includes(c)), [yChoice, props.columns]);
  const xIdx = useMemo(() => xCols.map((c) => props.columns.indexOf(c)), [xCols, props.columns]);
  const yIdx = useMemo(() => yCols.map((c) => props.columns.indexOf(c)), [yCols, props.columns]);
  const pairs = useMemo(() => crossPairs(xIdx, yIdx), [xIdx, yIdx]);
  const plan: SamplingPlan = useMemo(
    () => samplingPlan(props.rowIndices, props.groupLabels, pairs.length, pairs.length),
    [props.rowIndices, props.groupLabels, pairs.length],
  );
  const panelHeight = Math.max(MIN_PANEL_HEIGHT, Math.floor(height / Math.max(1, yCols.length)));

  // Draw panels a chunk at a time so a large grid never freezes the page;
  // the user can stop and continue.
  useEffect(() => {
    setRenderCount(PANEL_CHUNK);
    setDrawing(true);
  }, [pairs, mode]);
  useEffect(() => {
    if (!drawing || renderCount >= pairs.length) return;
    const timer = setTimeout(() => setRenderCount((n) => n + PANEL_CHUNK), 30);
    return () => clearTimeout(timer);
  }, [drawing, renderCount, pairs.length]);
  const shown = Math.min(renderCount, pairs.length);
  const visible = new Set(pairs.slice(0, shown).map((p) => `${p.xIndex}-${p.yIndex}`));

  const palette = props.palette ?? groupColors(props.groupNames.length);
  const legendGroups = useMemo(() => {
    const present = new Set<string>();
    for (const row of props.rowIndices) present.add(props.groupLabels[row] ?? 'All');
    return props.groupNames.filter((g) => present.has(g));
  }, [props.rowIndices, props.groupLabels, props.groupNames]);

  /** Saves every drawn panel as one image (grid layout). */
  const savePlots = async (format: ImageFormat): Promise<unknown> => {
    const holder = gridRef.current;
    if (!holder) throw new Error('There is no plot to save.');
    const parts: SvgPart[] = [];
    if (mode === 'static') {
      holder.querySelectorAll<SVGSVGElement>('svg.multiplot-panel').forEach((svg) => parts.push(serializeSvg(svg)));
    } else {
      const Plotly = (await import('plotly.js-dist-min')) as PlotlyModule;
      for (const fig of holder.querySelectorAll('div.multiplot-panel-plotly')) {
        const url = await Plotly.toImage(fig as unknown as Parameters<typeof Plotly.toImage>[0], {
          format: 'svg',
          width: PANEL_WIDTH,
          height: panelHeight,
        });
        parts.push({ text: svgFromDataUrl(url), width: PANEL_WIDTH, height: panelHeight });
      }
    }
    if (parts.length === 0) throw new Error('There is no plot to save.');
    const perRow = Math.max(1, xCols.length - (xCols.some((c) => yCols.includes(c)) ? 1 : 0));
    return saveSvgPart(combineSvgParts(parts, Math.min(perRow, parts.length)), 'Multiplot', format);
  };

  const samplingStatus = mode === 'interactive' ? samplingStatusText(plan) : null;

  return (
    <div className="multiplot">
      <div className="explore-controls">
        <ColumnPicker label="X" columns={props.columns} selected={xCols} onChange={setXChoice} />
        <ColumnPicker label="Y" columns={props.columns} selected={yCols} onChange={setYChoice} />
      </div>
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
          Point size {pointSize.toFixed(1)}
          <input
            type="range"
            min={0.1}
            max={10}
            step={0.1}
            value={pointSize}
            aria-label="Point size"
            onChange={(e) => setPointSize(Number(e.target.value))}
          />
        </label>
        <label>
          Plot height {height}px
          <input
            type="range"
            min={MIN_HEIGHT}
            max={MAX_HEIGHT}
            step={50}
            value={height}
            aria-label="Plot height"
            onChange={(e) => setHeight(Number(e.target.value))}
          />
        </label>
        <PlotSaveButtons name="Multiplot" save={savePlots} />
      </div>
      {samplingStatus !== null && <p role={plan.errorText === null ? 'status' : 'alert'}>{samplingStatus}</p>}
      {pairs.length === 0 && <p className="muted">Pick X and Y columns (at least one pair of different columns) for a multiplot.</p>}
      {legendGroups.length > 0 && pairs.length > 0 && (
        <ul className="multiplot-legend" aria-label="Groups">
          {legendGroups.map((g) => (
            <li key={g}>
              <span className="multiplot-swatch" style={{ background: palette[props.groupNames.indexOf(g)] ?? '#888' }} />
              {g}
            </li>
          ))}
        </ul>
      )}
      {shown < pairs.length && (
        <p className="multiplot-progress" role="status">
          {drawing ? `Drawing panels: ${shown} of ${pairs.length}` : `Stopped after ${shown} of ${pairs.length} panels.`}{' '}
          {drawing ? (
            <button type="button" className="btn btn-sm" onClick={() => setDrawing(false)}>Stop</button>
          ) : (
            <button type="button" className="btn btn-sm" onClick={() => setDrawing(true)}>
              Continue rendering {pairs.length - shown} more panels
            </button>
          )}
        </p>
      )}
      <div
        className="multiplot-grid"
        ref={gridRef}
        style={{ gridTemplateColumns: `repeat(${Math.max(1, xCols.length)}, max-content)` }}
      >
        {yIdx.flatMap((y) =>
          xIdx.map((x) => {
            const key = `${x}-${y}`;
            if (x === y) return <div key={key} className="multiplot-cell multiplot-cell-empty" aria-hidden="true" />;
            if (!visible.has(key)) return <div key={key} className="multiplot-cell multiplot-cell-pending" style={{ width: PANEL_WIDTH, height: panelHeight }} />;
            return (
              <MultiplotPanel
                key={key}
                props={props}
                pair={{ xIndex: x, yIndex: y }}
                plan={plan}
                pointSize={pointSize}
                height={panelHeight}
                mode={mode}
              />
            );
          }),
        )}
      </div>
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
  const indices = mode === 'interactive' ? plan.indices : props.rowIndices;
  const { values, groupLabels, groupNames, palette } = props;
  const traces = useMemo(
    () => panelPoints({ values, groupLabels, groupNames, palette }, pair, indices).traces,
    // pair is rebuilt each render; its two indices identify it.
    [values, groupLabels, groupNames, palette, pair.xIndex, pair.yIndex, indices],
  );
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
          rowUuids={props.rowUuids ?? NO_UUIDS}
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

const NO_UUIDS: readonly string[] = [];
