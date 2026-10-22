/**
 * Generic static Plotly chart. `plotly.js-dist-min` loads lazily inside the
 * effect so node:test `renderToString` never touches `window`.
 */
import { useEffect, useRef, useState, type ReactElement } from 'react';
import type { Data, Layout } from 'plotly.js-dist-min';

export interface PlotlyChartProps {
  readonly data: readonly Data[];
  readonly layout: Partial<Layout>;
  readonly ariaLabel: string;
  readonly className?: string;
  readonly height?: number;
}

export function PlotlyChart({
  data,
  layout,
  ariaLabel,
  className = 'plotly-chart',
  height = 420,
}: PlotlyChartProps): ReactElement {
  const holder = useRef<HTMLDivElement | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const el = holder.current;
    if (!el) return;
    let disposed = false;
    void (async () => {
      const Plotly = await import('plotly.js-dist-min');
      if (disposed) return;
      await Plotly.react(
        el,
        data as Data[],
        { height, margin: { l: 60, r: 20, t: 30, b: 60 }, ...layout } as Layout,
        { responsive: true, displaylogo: false },
      );
      setFailed(false);
    })().catch(() => {
      if (!disposed) setFailed(true);
    });
    return () => {
      disposed = true;
    };
  }, [data, layout, height]);

  return (
    <>
      <div ref={holder} className={className} role="img" aria-label={ariaLabel} />
      {failed && <p role="alert">The chart could not be rendered.</p>}
    </>
  );
}
