/**
 * Thin Plotly wrapper for the Visualize & Assign scatter (Section 9.4).
 *
 * `plotly.js-dist-min` is dynamically imported inside the layout effect so
 * node:test `renderToString` never touches `window`. Selection is keyed
 * internally by `analytical_uuid` carried in `customdata`; hover and table
 * surfaces show ANID/metadata only (the UUID is never rendered).
 */
import { useEffect, useRef, useState, type ReactElement } from 'react';
import type { Data, Layout } from 'plotly.js-dist-min';
import { plotTheme } from '../charts/PlotlyChart.tsx';
import { PlotSaveButtons } from '../ui/PlotSave.tsx';

export interface ScatterPoint {
  readonly uuid: string;
  readonly x: number;
  readonly y: number;
  readonly label: string;
  readonly hover: string;
  readonly groupName: string;
  readonly color: string;
  readonly symbol: string;
}

export interface ScatterTraceSpec {
  readonly name: string;
  readonly color: string;
  readonly symbol: string;
  readonly points: readonly ScatterPoint[];
  readonly ellipse?: { readonly px: number[]; readonly py: number[] };
  readonly showLabels?: boolean;
}

export interface VisualizeScatterProps {
  readonly traces: readonly ScatterTraceSpec[];
  readonly xLabel: string;
  readonly yLabel: string;
  /** 'lasso' | 'select' — Plotly drag modes. */
  readonly dragMode: 'lasso' | 'select';
  readonly onSelect: (uuids: readonly string[]) => void;
  /** Double-click clears the selection (Section 9.4). */
  readonly onClearSelection: () => void;
  /** Interactive selection requires a browser; SSR renders the placeholder. */
  readonly interactive?: boolean;
  /** Legend-only entries explaining the per-point "Symbols by" map. */
  readonly symbolLegend?: readonly { readonly name: string; readonly symbol: string }[];
}

/** Above this many points the WebGL renderer is used; SVG below it keeps
 *  text labels crisp and works in webviews without WebGL. */
export const WEBGL_POINT_THRESHOLD = 3000;

/** Stable default so the plot effect does not re-run every render. */
const NO_SYMBOL_LEGEND: readonly { readonly name: string; readonly symbol: string }[] = [];

type PlotlyModule = typeof import('plotly.js-dist-min');

interface PlotlyGraphDiv {
  on: (event: string, cb: (eventData: unknown) => void) => void;
  removeAllListeners?: (event: string) => void;
}

interface ScatterCallbacks {
  readonly onSelect: (uuids: readonly string[]) => void;
  readonly onClearSelection: () => void;
}

/** Attach selection handlers after Plotly has created the graph div. */
export function attachScatterSelectionHandlers(
  gd: PlotlyGraphDiv,
  callbacks: () => ScatterCallbacks,
): void {
  gd.removeAllListeners?.('plotly_selected');
  gd.removeAllListeners?.('plotly_doubleclick');
  gd.on('plotly_selected', (eventData: unknown) => {
    const points = (eventData as { points?: { customdata?: unknown[] }[] } | null)?.points ?? [];
    const uuids = points
      .map((point) => (Array.isArray(point.customdata) ? String(point.customdata[0]) : ''))
      .filter((uuid) => uuid !== '');
    callbacks().onSelect(uuids);
  });
  gd.on('plotly_doubleclick', () => callbacks().onClearSelection());
}

export function VisualizeScatter({
  traces,
  xLabel,
  yLabel,
  dragMode,
  onSelect,
  onClearSelection,
  interactive = true,
  symbolLegend = NO_SYMBOL_LEGEND,
}: VisualizeScatterProps): ReactElement {
  const holder = useRef<HTMLDivElement | null>(null);
  const frame = useRef<HTMLDivElement | null>(null);
  const callbacks = useRef<ScatterCallbacks>({ onSelect, onClearSelection });
  const [renderState, setRenderState] = useState<'loading' | 'complete' | 'error'>(
    interactive ? 'loading' : 'complete',
  );

  useEffect(() => {
    callbacks.current = { onSelect, onClearSelection };
  }, [onSelect, onClearSelection]);

  useEffect(() => {
    if (!interactive || !holder.current) return;
    let disposed = false;
    setRenderState('loading');
    void (async () => {
      const Plotly = (await import('plotly.js-dist-min')) as PlotlyModule;
      if (disposed || !holder.current) return;
      const wrapped: Data[] = [];
      const totalPoints = traces.reduce((sum, t) => sum + t.points.length, 0);
      const pointType = totalPoints > WEBGL_POINT_THRESHOLD ? 'scattergl' : 'scatter';
      for (const t of traces) {
        if (t.ellipse && t.ellipse.px.length > 0) {
          wrapped.push({
            type: 'scatter',
            mode: 'lines',
            name: `${t.name} ellipse`,
            x: [...t.ellipse.px],
            y: [...t.ellipse.py],
            line: { color: t.color, width: 1.5 },
            legendgroup: t.name,
            hoverinfo: 'skip',
            showlegend: false,
          });
        }
        wrapped.push({
          type: pointType,
          mode: t.showLabels ? 'markers+text' : 'markers',
          name: t.name,
          x: t.points.map((p) => p.x),
          y: t.points.map((p) => p.y),
          text: t.showLabels ? t.points.map((p) => p.label) : undefined,
          customdata: t.points.map((p) => [p.uuid, p.hover]),
          textposition: 'top center',
          // Hover shows the display label and group, never the uuid.
          hovertemplate: '%{customdata[1]}<extra></extra>',
          legendgroup: t.name,
          // Points carry per-point symbols, so a plain swatch trace below
          // stands in for the group in the legend.
          showlegend: false,
          marker: {
            color: t.color,
            // Per-point symbols carry the "Symbols by" field.
            symbol: t.points.map((p) => p.symbol),
            size: 8,
            line: { color: 'rgba(0,0,0,0.35)', width: 0.5 },
          },
          meta: t.name,
        } as Data);
        wrapped.push({
          type: 'scatter',
          mode: 'markers',
          name: t.name,
          x: [null],
          y: [null],
          legendgroup: t.name,
          hoverinfo: 'skip',
          marker: { color: t.color, symbol: 'circle', size: 9 },
        } as unknown as Data);
      }
      for (const entry of symbolLegend) {
        wrapped.push({
          type: 'scatter',
          mode: 'markers',
          name: entry.name,
          x: [null],
          y: [null],
          legendgroup: '__symbols__',
          legendgrouptitle: { text: 'Symbols' },
          hoverinfo: 'skip',
          marker: { color: '#777', symbol: entry.symbol, size: 8 },
        } as unknown as Data);
      }
      const theme = plotTheme();
      const layout = {
        ...theme,
        dragmode: dragMode,
        xaxis: { ...(theme.xaxis as object), title: { text: xLabel } },
        yaxis: { ...(theme.yaxis as object), title: { text: yLabel } },
        showlegend: true,
      } as Layout;
      const config = { responsive: true, displayModeBar: true, displaylogo: false, modeBarButtonsToRemove: ['toImage' as const] };
      const graph = holder.current;
      if (!graph) return;
      await Plotly.react(graph, wrapped, layout, config);
      if (disposed || holder.current !== graph) return;
      attachScatterSelectionHandlers(graph as unknown as PlotlyGraphDiv, () => callbacks.current);
      setRenderState('complete');
    })().catch(() => {
      if (disposed || !holder.current) return;
      setRenderState('error');
    });
    return () => {
      disposed = true;
    };
  }, [traces, xLabel, yLabel, dragMode, interactive, symbolLegend]);

  return (
    <div className="chart-frame" ref={frame}>
      <div className="chart-toolbar">
        <PlotSaveButtons target={frame} name={`${yLabel} vs ${xLabel}`} kind="plotly" />
      </div>
      <div
        ref={holder}
        className="visualize-scatter"
        data-render-state={renderState}
        role="img"
        aria-label={`Scatter of ${yLabel} by ${xLabel}`}
      />
      {renderState === 'error' && (
        <p role="alert">The scatter plot could not be rendered. Try changing the axes or reloading the view.</p>
      )}
    </div>
  );
}
