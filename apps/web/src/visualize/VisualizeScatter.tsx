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
}

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
}: VisualizeScatterProps): ReactElement {
  const holder = useRef<HTMLDivElement | null>(null);
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
      for (const t of traces) {
        if (t.ellipse && t.ellipse.px.length > 0) {
          wrapped.push({
            type: 'scatter',
            mode: 'lines',
            name: `${t.name} ellipse`,
            x: [...t.ellipse.px],
            y: [...t.ellipse.py],
            line: { color: t.color, width: 1, dash: 'dash' },
            hoverinfo: 'skip',
            showlegend: false,
          });
        }
        wrapped.push({
          type: 'scattergl',
          mode: t.showLabels ? 'markers+text' : 'markers',
          name: t.name,
          x: t.points.map((p) => p.x),
          y: t.points.map((p) => p.y),
          text: t.showLabels ? t.points.map((p) => p.label) : undefined,
          customdata: t.points.map((p) => [p.uuid, p.hover]),
          textposition: 'top center',
          // Hover shows the display label and group, never the uuid.
          hovertemplate: '%{customdata[1]}<extra></extra>',
          marker: {
            color: t.color,
            symbol: t.symbol,
            size: 8,
          },
          meta: t.name,
        } as Data);
      }
      const layout = {
        dragmode: dragMode,
        xaxis: { title: { text: xLabel } },
        yaxis: { title: { text: yLabel } },
        showlegend: true,
      } as Layout;
      const config = { responsive: true, displayModeBar: true } as const;
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
  }, [traces, xLabel, yLabel, dragMode, interactive]);

  return (
    <>
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
    </>
  );
}
