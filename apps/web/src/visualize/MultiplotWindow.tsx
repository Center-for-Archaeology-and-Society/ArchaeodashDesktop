/**
 * The multiplot pop-out window (a desktop window or a browser popup): no
 * shell and no backend. It reads the plotted data the main window saved
 * (multiplot-window.ts) and shows either the whole grid or one plot sized
 * to the window. Lasso selections go back to the main window.
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import { GroupLegend, Multiplot, MultiplotPanel, type MultiplotMode } from './Multiplot.tsx';
import { loadSnapshot, postSelection, type MultiplotSnapshot } from './multiplot-window.ts';
import { samplingPlan, samplingStatusText } from './multiplot-model.ts';
import { PlotSaveButtons, savePlotlyChart, saveSvgFigure, type ImageFormat } from '../ui/PlotSave.tsx';

type LoadState =
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'loaded'; data: MultiplotSnapshot; rows: number[] };

export function MultiplotWindow({ snapshotKey, pair }: { snapshotKey: string; pair: { x: number; y: number } | null }): ReactElement {
  const [state, setState] = useState<LoadState>({ kind: 'loading' });
  useEffect(() => {
    let cancelled = false;
    loadSnapshot(snapshotKey)
      .then((data) => {
        if (cancelled) return;
        if (!data) {
          setState({ kind: 'error', message: 'The plot data is no longer available. Open the window again from Visualize & Assign.' });
          return;
        }
        setState({ kind: 'loaded', data, rows: data.groupLabels.map((_, i) => i) });
      })
      .catch((err: unknown) => {
        if (!cancelled) setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [snapshotKey]);
  const select = useCallback((uuids: readonly string[]) => postSelection(snapshotKey, uuids), [snapshotKey]);

  useEffect(() => {
    if (state.kind !== 'loaded') return;
    const { columns } = state.data;
    document.title = pair
      ? `${columns[pair.y] ?? 'Y'} by ${columns[pair.x] ?? 'X'} — ArchaeoDash`
      : 'Multiplot — ArchaeoDash';
  }, [state, pair]);

  if (state.kind === 'loading') return <main className="multiplot-window main-panel"><p className="muted">Loading the plots…</p></main>;
  if (state.kind === 'error') return <main className="multiplot-window main-panel"><p role="alert">{state.message}</p></main>;
  const { data, rows } = state;
  if (pair) {
    if (!data.columns[pair.x] || !data.columns[pair.y] || pair.x === pair.y) {
      return <main className="multiplot-window main-panel"><p role="alert">This plot is not in the saved data.</p></main>;
    }
    return <SinglePlot data={data} rows={rows} pair={pair} onSelect={select} />;
  }
  return (
    <main className="multiplot-window main-panel">
      <Multiplot
        columns={data.columns}
        values={data.values}
        groupLabels={data.groupLabels}
        groupNames={data.groupNames}
        palette={data.palette}
        rowIndices={rows}
        rowUuids={data.rowUuids}
        onSelect={select}
        initialMode={data.mode}
        initialX={data.x}
        initialY={data.y}
        initialPointSize={data.pointSize}
        initialHeight={data.height}
        windowControls={false}
      />
    </main>
  );
}

/** Space kept below the plot (panel border and padding, page padding). */
const BOTTOM_SPACE = 32;

function SinglePlot({
  data,
  rows,
  pair,
  onSelect,
}: {
  data: MultiplotSnapshot;
  rows: number[];
  pair: { x: number; y: number };
  onSelect: (uuids: readonly string[]) => void;
}): ReactElement {
  const [mode, setMode] = useState<MultiplotMode>(data.mode);
  const [pointSize, setPointSize] = useState(data.pointSize);
  const [size, setSize] = useState(() => ({ w: window.innerWidth, h: window.innerHeight }));
  useEffect(() => {
    const onResize = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  const plotRef = useRef<HTMLDivElement | null>(null);
  // The toolbar and legend can wrap, so the plot starts where it lands.
  const [top, setTop] = useState(120);
  useLayoutEffect(() => {
    if (plotRef.current) setTop(Math.round(plotRef.current.getBoundingClientRect().top + window.scrollY));
  }, [size, mode]);
  const plan = useMemo(() => samplingPlan(rows, data.groupLabels, 1, 1), [rows, data.groupLabels]);
  const indices = mode === 'interactive' ? plan.indices : rows;
  const width = Math.max(320, size.w - 40);
  const height = Math.max(260, size.h - top - BOTTOM_SPACE);
  const legendGroups = useMemo(() => {
    const present = new Set(data.groupLabels);
    return data.groupNames.filter((g) => present.has(g));
  }, [data]);
  const save = async (format: ImageFormat): Promise<unknown> => {
    const el = plotRef.current;
    if (!el) throw new Error('There is no plot to save.');
    const name = `${data.columns[pair.y]} by ${data.columns[pair.x]}`;
    if (mode === 'interactive') {
      const fig = el.querySelector<HTMLElement>('.multiplot-panel-plotly');
      if (!fig) throw new Error('There is no plot to save.');
      return savePlotlyChart(fig, name, format);
    }
    return saveSvgFigure(el, name, format);
  };
  const samplingStatus = mode === 'interactive' ? samplingStatusText(plan) : null;
  return (
    <main className="multiplot-window multiplot-window-single main-panel">
      <div className="explore-controls">
        <label>
          Render mode
          <select aria-label="Render mode" value={mode} onChange={(e) => setMode(e.target.value as MultiplotMode)}>
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
        <PlotSaveButtons name="Multiplot panel" save={save} />
      </div>
      {samplingStatus !== null && <p role="status">{samplingStatus}</p>}
      {legendGroups.length > 0 && <GroupLegend groups={legendGroups} groupNames={data.groupNames} palette={data.palette} />}
      <div ref={plotRef}>
        <MultiplotPanel
          columns={data.columns}
          values={data.values}
          groupLabels={data.groupLabels}
          groupNames={data.groupNames}
          palette={data.palette}
          rowUuids={data.rowUuids}
          onSelect={onSelect}
          xIndex={pair.x}
          yIndex={pair.y}
          indices={indices}
          pointSize={pointSize}
          width={width}
          height={height}
          mode={mode}
        />
      </div>
    </main>
  );
}
