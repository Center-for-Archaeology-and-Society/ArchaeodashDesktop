/**
 * Multiplot (Section 9.4): the legacy facet grid of X × Y scatters — rows
 * are the chosen Y columns, columns the chosen X columns, X = Y skipped —
 * with point size 0.1–10, whole-figure height 500–2000, static/interactive
 * mode, legacy per-group/per-facet 100k-point interactive sampling (labeled
 * per procedure 13), progressive drawing with stop/continue, and plot save
 * (SVG download in static mode; Plotly toImage SVG in interactive mode; the
 * uuid stays internal to selection only).
 *
 * Changing the X/Y pickers stops drawing at once; the grid follows the new
 * choice once the pickers settle. Drawn panels are memoized, so a picker
 * click never redraws them, and each drawing step is a React transition,
 * so input is handled between steps. The plots can open in new windows
 * (all in one, or one per plot); see multiplot-window.ts.
 */
import { memo, startTransition, useCallback, useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import type { Data, Layout } from 'plotly.js-dist-min';
import {
  crossPairs,
  extent,
  gridShape,
  niceTicks,
  samplingPlan,
  samplingStatusText,
  uuidsFromSelectedEvent,
  type PlotPair,
  type SamplingPlan,
} from './multiplot-model.ts';
import { colorFor, groupColors } from './visualize-model.ts';
import { plotTheme } from '../charts/PlotlyChart.tsx';
import { plotlyText } from '../charts/plotly-text.ts';
import {
  MAX_PLOT_WINDOWS,
  buildSnapshot,
  newSnapshotKey,
  onPopoutSelection,
  openPopouts,
  tileWindows,
  type PopoutGroup,
  type PopoutLayout,
  type PopoutTarget,
} from './multiplot-window.ts';
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
  readonly initialPointSize?: number;
  readonly initialHeight?: number;
  /** Show the "open in new window" controls (off inside a pop-out window). */
  readonly windowControls?: boolean;
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
  const plotlyRef = useRef<PlotlyModule | null>(null);
  // The latest callback, so a new function identity never redraws the panel.
  const selectRef = useRef(onSelect);
  useEffect(() => {
    selectRef.current = onSelect;
  }, [onSelect]);
  // Removing a panel frees its WebGL context (browsers allow only a few).
  useEffect(() => {
    const el = holder.current;
    return () => {
      if (el && plotlyRef.current) plotlyRef.current.purge(el);
    };
  }, []);

  useEffect(() => {
    if (!holder.current) return;
    let disposed = false;
    void (async () => {
      const Plotly = (await import('plotly.js-dist-min')) as PlotlyModule;
      if (disposed || !holder.current) return;
      plotlyRef.current = Plotly;
      const data: Data[] = traces.map((t) => ({
        type: 'scattergl',
        mode: 'markers',
        name: t.name,
        x: t.points.map((p) => p.x),
        y: t.points.map((p) => p.y),
        // customdata[0] is the analytical_uuid for selection; never rendered.
        customdata: t.points.map((p) => [rowUuids[p.row] ?? '', t.name]),
        // Hover shows the group only, never the uuid.
        hovertemplate: `${plotlyText(t.name)}<extra></extra>`,
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

/** Points drawn per progressive step, so one step never blocks for long. */
export const POINTS_PER_STEP = 60_000;
/** Quiet time after the last picker change before the grid follows it. */
export const PICKER_SETTLE_MS = 500;

/** Panels per drawing step: PANEL_CHUNK, fewer when each panel has many points. */
export function panelsPerStep(pointsPerPanel: number): number {
  return Math.max(1, Math.min(PANEL_CHUNK, Math.floor(POINTS_PER_STEP / Math.max(1, pointsPerPanel))));
}

interface AxisChoice {
  readonly x: readonly string[] | null;
  readonly y: readonly string[] | null;
}

type PopoutState = { kind: 'idle' } | { kind: 'busy' } | { kind: 'error'; text: string };

/** Unset choices follow the available columns (all on both axes). */
function chosenColumns(choice: readonly string[] | null, columns: readonly string[]): string[] {
  return (choice ?? columns).filter((c) => columns.includes(c));
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

export function Multiplot(props: MultiplotProps): ReactElement {
  const [pointSize, setPointSize] = useState(props.initialPointSize ?? 2);
  const [height, setHeight] = useState(props.initialHeight ?? DEFAULT_HEIGHT);
  const [mode, setMode] = useState<MultiplotMode>(props.initialMode ?? 'static');
  // `draft` is what the pickers show (changes at once); `applied` is what
  // the grid draws (follows the pickers once they settle).
  const [draft, setDraft] = useState<AxisChoice>(() => ({ x: props.initialX ?? null, y: props.initialY ?? null }));
  const [applied, setApplied] = useState<AxisChoice>(draft);
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const [drawing, setDrawing] = useState(true);
  const gridRef = useRef<HTMLDivElement | null>(null);

  const draftX = useMemo(() => chosenColumns(draft.x, props.columns), [draft.x, props.columns]);
  const draftY = useMemo(() => chosenColumns(draft.y, props.columns), [draft.y, props.columns]);
  const xCols = useMemo(() => chosenColumns(applied.x, props.columns), [applied.x, props.columns]);
  const yCols = useMemo(() => chosenColumns(applied.y, props.columns), [applied.y, props.columns]);
  const xIdx = useMemo(() => xCols.map((c) => props.columns.indexOf(c)), [xCols, props.columns]);
  const yIdx = useMemo(() => yCols.map((c) => props.columns.indexOf(c)), [yCols, props.columns]);
  const pairs = useMemo(() => crossPairs(xIdx, yIdx), [xIdx, yIdx]);
  const draftPairs = useMemo(
    () => crossPairs(draftX.map((c) => props.columns.indexOf(c)), draftY.map((c) => props.columns.indexOf(c))),
    [draftX, draftY, props.columns],
  );
  const plan: SamplingPlan = useMemo(
    () => samplingPlan(props.rowIndices, props.groupLabels, pairs.length, pairs.length),
    [props.rowIndices, props.groupLabels, pairs.length],
  );
  const indices = mode === 'interactive' ? plan.indices : props.rowIndices;
  // SVG panels cost one DOM node per point; Plotly panels draw with WebGL
  // after the step, so only SVG steps shrink with the point count.
  const step = mode === 'interactive' ? PANEL_CHUNK : panelsPerStep(indices.length);
  const [renderCount, setRenderCount] = useState(step);
  const shape = useMemo(() => gridShape(xIdx.length, yIdx.length, pairs.length), [xIdx.length, yIdx.length, pairs.length]);
  const panelHeight = Math.max(MIN_PANEL_HEIGHT, Math.floor(height / shape.rows));

  // Draw panels a step at a time so a large grid never freezes the page;
  // the user can stop and continue. Steps are transitions, so React stops
  // a step's render to handle a click or key press.
  useEffect(() => {
    setRenderCount(step);
    setDrawing(true);
  }, [pairs, mode, step]);
  useEffect(() => {
    if (!drawing || renderCount >= pairs.length) return;
    const timer = setTimeout(() => startTransition(() => setRenderCount((n) => n + step)), 30);
    return () => clearTimeout(timer);
  }, [drawing, renderCount, pairs.length, step]);
  const shown = Math.min(renderCount, pairs.length);
  const committed = useRef(shown);
  useEffect(() => {
    committed.current = shown;
  }, [shown]);

  // Touching the X/Y pickers stops drawing at once (and drops a step that
  // is still rendering); the grid follows the pickers once they settle.
  const settleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const stopForPickers = useCallback(() => {
    setDrawing(false);
    setRenderCount(committed.current);
    if (settleTimer.current !== null) clearTimeout(settleTimer.current);
    settleTimer.current = setTimeout(() => {
      settleTimer.current = null;
      const next = draftRef.current;
      startTransition(() => {
        setApplied(next);
        setDrawing(true);
      });
    }, PICKER_SETTLE_MS);
  }, []);
  useEffect(() => () => {
    if (settleTimer.current !== null) clearTimeout(settleTimer.current);
  }, []);
  const pickX = useCallback((next: string[]) => {
    setDraft((d) => ({ ...d, x: next }));
    stopForPickers();
  }, [stopForPickers]);
  const pickY = useCallback((next: string[]) => {
    setDraft((d) => ({ ...d, y: next }));
    stopForPickers();
  }, [stopForPickers]);

  const palette = useMemo(() => props.palette ?? groupColors(props.groupNames.length), [props.palette, props.groupNames.length]);
  const legendGroups = useMemo(() => {
    const present = new Set<string>();
    for (const row of props.rowIndices) present.add(props.groupLabels[row] ?? 'All');
    return props.groupNames.filter((g) => present.has(g));
  }, [props.rowIndices, props.groupLabels, props.groupNames]);

  // Pop-out windows: selections made there come back to this page.
  const [popLayout, setPopLayout] = useState<PopoutLayout>('all');
  const [popState, setPopState] = useState<PopoutState>({ kind: 'idle' });
  const openedKeys = useRef(new Set<string>());
  // Window groups opened one-per-plot, newest last, for "Tile windows".
  const [popGroups, setPopGroups] = useState<PopoutGroup[]>([]);
  const onSelectRef = useRef(props.onSelect);
  onSelectRef.current = props.onSelect;
  useEffect(
    () => onPopoutSelection((key) => openedKeys.current.has(key), (uuids) => onSelectRef.current?.(uuids)),
    [],
  );
  const openWindows = () => {
    if (draftPairs.length === 0) return;
    if (popLayout === 'each' && draftPairs.length > MAX_PLOT_WINDOWS) {
      setPopState({
        kind: 'error',
        text: `That is ${draftPairs.length} plots; one window per plot opens at most ${MAX_PLOT_WINDOWS}. Pick fewer X or Y columns.`,
      });
      return;
    }
    const key = newSnapshotKey();
    openedKeys.current.add(key);
    const snapshot = buildSnapshot({ ...props, palette }, { x: draftX, y: draftY, mode, pointSize, height });
    const col = (i: number) => snapshot.columns.indexOf(props.columns[i] ?? '');
    const screenW = window.screen?.availWidth || 1600;
    const screenH = window.screen?.availHeight || 1000;
    const popShape = gridShape(draftX.length, draftY.length, draftPairs.length);
    const popColumns = popShape.columns;
    // A full X × Y matrix keeps its layout on screen; a wrapped grid lets
    // the windows fill a grid that fits the screen.
    const cellOf = (p: PlotPair) => popShape.wrapped
      ? undefined
      : { row: draftY.indexOf(props.columns[p.yIndex] ?? ''), col: draftX.indexOf(props.columns[p.xIndex] ?? '') };
    const targets: PopoutTarget[] = popLayout === 'all'
      ? [{
          title: 'Multiplot',
          width: clamp(popColumns * (PANEL_WIDTH + 24) + 80, 640, screenW),
          height: clamp(height + 280, 600, screenH),
        }]
      : draftPairs.map((p) => ({
          title: `${props.columns[p.yIndex] ?? 'Y'} by ${props.columns[p.xIndex] ?? 'X'}`,
          pair: { x: col(p.xIndex), y: col(p.yIndex) },
          width: clamp(760, 400, screenW),
          height: clamp(680, 400, screenH),
          cell: cellOf(p),
        }));
    setPopState({ kind: 'busy' });
    openPopouts(key, snapshot, targets)
      .then(({ blocked, group }) => {
        if (popLayout === 'each' && blocked < targets.length) setPopGroups((gs) => [...gs, group]);
        if (blocked === 0) setPopState({ kind: 'idle' });
        else {
          setPopState({
            kind: 'error',
            text: blocked === targets.length
              ? 'The browser blocked the new window. Allow pop-ups for this site and try again.'
              : `The browser blocked ${blocked} of ${targets.length} windows. Allow pop-ups for this site and try again.`,
          });
        }
      })
      .catch((err: unknown) => {
        setPopState({ kind: 'error', text: `Could not open the plot window: ${err instanceof Error ? err.message : String(err)}` });
      });
  };

  const tileOpenWindows = () => {
    tileWindows(popGroups)
      .then(({ tiled }) => {
        if (tiled === 0) {
          setPopGroups([]);
          setPopState({ kind: 'error', text: 'No plot windows are open. Open them with “One window per plot” first.' });
        } else setPopState({ kind: 'idle' });
      })
      .catch((err: unknown) => {
        setPopState({ kind: 'error', text: `Could not tile the windows: ${err instanceof Error ? err.message : String(err)}` });
      });
  };

  /** Saves every drawn panel as one image (grid layout). */
  const savePlots = async (format: ImageFormat): Promise<unknown> => {
    const holder = gridRef.current;
    if (!holder) throw new Error('There is no plot to save.');
    // One part per grid cell, blanks included (the X = Y diagonal and
    // undrawn panels), so the saved image keeps the on-screen rows: rows
    // can hold different panel counts when only some Y columns are also X.
    // A wrapped grid (one column on an axis) has no diagonal blanks.
    const parts: SvgPart[] = [];
    let drawn = 0;
    const blank: SvgPart = {
      text: `<svg xmlns="http://www.w3.org/2000/svg" width="${PANEL_WIDTH}" height="${panelHeight}"></svg>`,
      width: PANEL_WIDTH,
      height: panelHeight,
    };
    const Plotly = mode === 'interactive' ? ((await import('plotly.js-dist-min')) as PlotlyModule) : null;
    for (const cell of holder.querySelectorAll(':scope > .multiplot-cell')) {
      const svg = mode === 'static' ? cell.querySelector<SVGSVGElement>('svg.multiplot-panel') : null;
      const fig = Plotly ? cell.querySelector('div.multiplot-panel-plotly') : null;
      if (svg) {
        parts.push(serializeSvg(svg));
        drawn++;
      } else if (Plotly && fig) {
        const url = await Plotly.toImage(fig as unknown as Parameters<typeof Plotly.toImage>[0], {
          format: 'svg',
          width: PANEL_WIDTH,
          height: panelHeight,
        });
        parts.push({ text: svgFromDataUrl(url), width: PANEL_WIDTH, height: panelHeight });
        drawn++;
      } else {
        parts.push(blank);
      }
    }
    if (drawn === 0) throw new Error('There is no plot to save.');
    return saveSvgPart(combineSvgParts(parts, shape.columns), 'Multiplot', format);
  };

  const samplingStatus = mode === 'interactive' ? samplingStatusText(plan) : null;
  const rowUuids = props.rowUuids ?? NO_UUIDS;

  return (
    <div className="multiplot">
      <div className="explore-controls" onPointerDownCapture={stopForPickers} onKeyDownCapture={stopForPickers}>
        <ColumnPicker label="X" columns={props.columns} selected={draftX} onChange={pickX} />
        <ColumnPicker label="Y" columns={props.columns} selected={draftY} onChange={pickY} />
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
        {props.windowControls !== false && (
          <span className="multiplot-popout" role="group" aria-label="Open in new window">
            <select aria-label="New window layout" value={popLayout} onChange={(e) => setPopLayout(e.target.value as PopoutLayout)}>
              <option value="all">All plots in one window</option>
              <option value="each">One window per plot</option>
            </select>
            <button
              type="button"
              className="btn btn-sm"
              disabled={draftPairs.length === 0 || popState.kind === 'busy'}
              onClick={openWindows}
              title="Open the plots in a separate window"
            >
              {popLayout === 'each'
                ? `Open ${draftPairs.length} window${draftPairs.length === 1 ? '' : 's'}`
                : 'Open in new window'}
            </button>
            {popGroups.length > 0 && (
              <button
                type="button"
                className="btn btn-sm"
                onClick={tileOpenWindows}
                title="Arrange the open plot windows in a grid across the screen"
              >
                Tile windows
              </button>
            )}
          </span>
        )}
      </div>
      {popState.kind === 'error' && <p role="alert">{popState.text}</p>}
      {samplingStatus !== null && <p role={plan.errorText === null ? 'status' : 'alert'}>{samplingStatus}</p>}
      {pairs.length === 0 && <p className="muted">Pick X and Y columns (at least one pair of different columns) for a multiplot.</p>}
      {legendGroups.length > 0 && pairs.length > 0 && <GroupLegend groups={legendGroups} groupNames={props.groupNames} palette={palette} />}
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
        style={{ gridTemplateColumns: `repeat(${shape.columns}, max-content)` }}
      >
        {(() => {
          let order = 0;
          return yIdx.flatMap((y) =>
            xIdx.flatMap((x) => {
              const key = `${x}-${y}`;
              if (x === y) {
                // A wrapped grid packs the panels, so the diagonal leaves no gap.
                if (shape.wrapped) return [];
                return <div key={key} className="multiplot-cell multiplot-cell-empty" aria-hidden="true" />;
              }
              // Pairs draw in grid order, so the first `shown` panels are drawn.
              if (order++ >= shown) return <div key={key} className="multiplot-cell multiplot-cell-pending" style={{ width: PANEL_WIDTH, height: panelHeight }} />;
              return (
                <MultiplotPanel
                  key={key}
                  columns={props.columns}
                  values={props.values}
                  groupLabels={props.groupLabels}
                  groupNames={props.groupNames}
                  palette={props.palette}
                  rowUuids={rowUuids}
                  onSelect={props.onSelect}
                  xIndex={x}
                  yIndex={y}
                  indices={indices}
                  pointSize={pointSize}
                  width={PANEL_WIDTH}
                  height={panelHeight}
                  mode={mode}
                />
              );
            }),
          );
        })()}
      </div>
    </div>
  );
}

/** Group color key shared by the grid and the pop-out windows. */
export function GroupLegend({
  groups,
  groupNames,
  palette,
}: {
  groups: readonly string[];
  groupNames: readonly string[];
  palette: readonly string[];
}): ReactElement {
  return (
    <ul className="multiplot-legend" aria-label="Groups">
      {groups.map((g) => (
        <li key={g}>
          <span className="multiplot-swatch" style={{ background: palette[groupNames.indexOf(g)] ?? '#888' }} />
          {g}
        </li>
      ))}
    </ul>
  );
}

export interface MultiplotPanelProps {
  readonly columns: readonly string[];
  readonly values: MultiplotProps['values'];
  readonly groupLabels: readonly string[];
  readonly groupNames: readonly string[];
  readonly palette?: readonly string[];
  readonly rowUuids: readonly string[];
  readonly onSelect?: (uuids: readonly string[]) => void;
  readonly xIndex: number;
  readonly yIndex: number;
  /** Rows to draw (the sampling plan in interactive mode). */
  readonly indices: readonly number[];
  readonly pointSize: number;
  readonly width: number;
  readonly height: number;
  readonly mode: MultiplotMode;
}

/**
 * One X × Y panel. Memoized on plain, stable props: a picker click or any
 * other change in the grid around it never redraws a finished panel.
 */
export const MultiplotPanel = memo(function MultiplotPanel({
  columns,
  values,
  groupLabels,
  groupNames,
  palette,
  rowUuids,
  onSelect,
  xIndex,
  yIndex,
  indices,
  pointSize,
  width,
  height,
  mode,
}: MultiplotPanelProps): ReactElement {
  const pair = useMemo(() => ({ xIndex, yIndex }), [xIndex, yIndex]);
  const traces = useMemo(
    () => panelPoints({ values, groupLabels, groupNames, palette }, pair, indices).traces,
    [values, groupLabels, groupNames, palette, pair, indices],
  );
  const xLabel = columns[xIndex] ?? `X${xIndex}`;
  const yLabel = columns[yIndex] ?? `Y${yIndex}`;
  if (mode === 'interactive') {
    return (
      <div className="multiplot-cell">
        <InteractivePanel
          pair={pair}
          xLabel={xLabel}
          yLabel={yLabel}
          width={width}
          height={height}
          pointSize={pointSize}
          traces={traces}
          rowUuids={rowUuids}
          onSelect={onSelect}
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
        width={width}
        height={height}
        pointSize={pointSize}
        traces={traces}
      />
    </div>
  );
});

const NO_UUIDS: readonly string[] = [];
