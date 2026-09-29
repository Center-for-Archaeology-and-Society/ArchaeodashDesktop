import { useId, useState, type ReactElement, type ReactNode } from 'react';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { ClusterDiagnosticsPlots } from './ClusterDiagnosticsPlots.tsx';
import { Dendrogram } from './Dendrogram.tsx';

/** Enlarges the scrollable plot viewport without moving keyboard focus. */
export function PlotPanel({ title, children, initialExpanded = false }: {
  title: string;
  children: ReactNode;
  initialExpanded?: boolean;
}): ReactElement {
  const [expanded, setExpanded] = useState(initialExpanded);
  const id = useId();
  return <section className={`cluster-plot-panel${expanded ? ' expanded' : ''}`} aria-label={title}>
    <div className="cluster-plot-toolbar">
      <h2>{title}</h2>
      <button type="button" aria-expanded={expanded} aria-controls={id} onClick={() => setExpanded(value => !value)}>
        {expanded ? 'Reduce plot view' : 'Expand plot view'}
      </button>
    </div>
    <div id={id} className="cluster-plot-viewport" role="region" aria-label={`${title} plot area`} tabIndex={0}>
      {children}
    </div>
  </section>;
}

export function AnalysisPlots({ result, cutK: controlledCutK, onCutKChange, disabled = false }: {
  result: AnalysisResult;
  cutK?: number;
  disabled?: boolean;
  onCutKChange?: (value: number) => void;
}): ReactElement | null {
  const [localCutK, setLocalCutK] = useState(2);
  const cutK = controlledCutK ?? localCutK;
  const setCutK = onCutKChange ?? setLocalCutK;
  const [leafSize, setLeafSize] = useState(12);
  if (result.kind === 'diagnostics') {
    return <PlotPanel title="Cluster diagnostics"><ClusterDiagnosticsPlots result={result.data} /></PlotPanel>;
  }
  if (result.kind !== 'fit' || !['hclust_ward_d2', 'diana'].includes(result.data.method)) return null;
  const count = result.data.n_rows;
  const boundedK = Math.max(1, Math.min(count, cutK));
  return <section aria-label="Hierarchical cluster plots">
    <div className="cluster-plot-toolbar">
      <label>Cut into clusters <input disabled={disabled} type="number" min={1} max={count} value={boundedK}
        onChange={e => { const value = Number(e.target.value); if (Number.isInteger(value) && value >= 1 && value <= count) setCutK(value); }} /></label>
      <label>Leaf text size <input type="range" min={8} max={20} value={leafSize}
        onChange={e => setLeafSize(Number(e.target.value))} /> {leafSize}px</label>
    </div>
    <p>Colors show the {boundedK}-cluster cut. Analytical unit numbers refer to the input row order; cutting the tree does not change the group file.</p>
    <PlotPanel title={result.data.method === 'diana' ? 'DIANA dendrogram' : 'Ward.D2 dendrogram'}>
      <Dendrogram result={result.data} cutK={boundedK} leafSize={leafSize} />
    </PlotPanel>
  </section>;
}
