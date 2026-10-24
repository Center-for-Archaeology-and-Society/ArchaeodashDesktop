/**
 * Generic static Plotly chart. `plotly.js-dist-min` loads lazily inside the
 * effect so node:test `renderToString` never touches `window`.
 *
 * Charts follow the app theme (text, grid, and transparent backgrounds from
 * the CSS tokens, redrawn when the theme changes), resize when their tab is
 * shown again, and carry Save PNG / Save SVG buttons (Plotly's own download
 * button cannot open a save dialog in the desktop app).
 */
import { useEffect, useRef, useState, type ReactElement } from 'react';
import type { Data, Layout } from 'plotly.js-dist-min';
import { PlotSaveButtons } from '../ui/PlotSave.tsx';

export interface PlotlyChartProps {
  readonly data: readonly Data[];
  readonly layout: Partial<Layout>;
  readonly ariaLabel: string;
  readonly className?: string;
  readonly height?: number;
  /** File name for saved images; defaults to the aria label. `false` hides the buttons. */
  readonly saveName?: string | false;
}

/** Theme colors for Plotly, read from the CSS tokens. */
export function plotTheme(): Partial<Layout> {
  if (typeof document === 'undefined') return {};
  const css = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  const text = token('--fg', '#17221d');
  const grid = token('--grid', 'rgba(0,0,0,0.08)');
  const axis = { gridcolor: grid, zerolinecolor: grid, linecolor: grid, tickfont: { color: text } };
  return {
    paper_bgcolor: 'rgba(0,0,0,0)',
    plot_bgcolor: 'rgba(0,0,0,0)',
    font: { family: token('--font-sans', 'system-ui, sans-serif'), color: text, size: 12 },
    colorway: ['#1f8a63', '#d9822b', '#3b6fd8', '#c2417a', '#7a5ad6', '#2aa3b8', '#a3772b', '#5c6b64'],
    xaxis: axis,
    yaxis: axis,
    legend: { font: { color: text } },
    hoverlabel: { font: { family: token('--font-sans', 'system-ui, sans-serif') } },
  } as Partial<Layout>;
}

/** Merges per-axis theme defaults under the chart's own axis settings. */
function themed(layout: Partial<Layout>, height: number): Layout {
  const theme = plotTheme();
  const merged: Record<string, unknown> = { height, margin: { l: 64, r: 24, t: 32, b: 60 }, ...theme, ...layout };
  for (const axis of ['xaxis', 'yaxis'] as const) {
    merged[axis] = { ...(theme[axis] as object), ...((layout[axis] as object | undefined) ?? {}) };
  }
  return merged as unknown as Layout;
}

export function PlotlyChart({
  data,
  layout,
  ariaLabel,
  className = 'plotly-chart',
  height = 420,
  saveName,
}: PlotlyChartProps): ReactElement {
  const frame = useRef<HTMLDivElement | null>(null);
  const holder = useRef<HTMLDivElement | null>(null);
  const [failed, setFailed] = useState(false);
  const [themeTick, setThemeTick] = useState(0);

  // Redraw with the new colors when the theme changes.
  useEffect(() => {
    if (typeof MutationObserver === 'undefined') return;
    const observer = new MutationObserver(() => setThemeTick((t) => t + 1));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'style'] });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const el = holder.current;
    if (!el) return;
    let disposed = false;
    void (async () => {
      const Plotly = await import('plotly.js-dist-min');
      if (disposed) return;
      await Plotly.react(el, data as Data[], themed(layout, height), {
        responsive: true,
        displaylogo: false,
        modeBarButtonsToRemove: ['toImage'],
      });
      setFailed(false);
    })().catch(() => {
      if (!disposed) setFailed(true);
    });
    return () => {
      disposed = true;
    };
  }, [data, layout, height, themeTick]);

  // A chart drawn while its tab was hidden has no width; resize when shown.
  useEffect(() => {
    const el = holder.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    let last = el.clientWidth;
    const observer = new ResizeObserver(() => {
      const width = el.clientWidth;
      if (width > 0 && width !== last) {
        last = width;
        void import('plotly.js-dist-min').then((Plotly) => {
          if (el.isConnected && (el as HTMLElement & { data?: unknown }).data) void Plotly.Plots.resize(el);
        });
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return (
    <div className="chart-frame" ref={frame}>
      {saveName !== false && (
        <div className="chart-toolbar">
          <PlotSaveButtons target={frame} name={saveName ?? ariaLabel} kind="plotly" />
        </div>
      )}
      <div ref={holder} className={className} role="img" aria-label={ariaLabel} />
      {failed && <p role="alert">The chart could not be rendered.</p>}
    </div>
  );
}
