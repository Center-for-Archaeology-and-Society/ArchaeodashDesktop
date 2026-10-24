import { PartitionPlot } from './PartitionPlot.tsx';
import { useId, useRef, useState, type ReactElement, type ReactNode } from 'react';
import { PlotSaveButtons, combineSvgParts, saveSvgPart, serializeSvg, type ImageFormat } from '../ui/PlotSave.tsx';
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
  const viewport = useRef<HTMLDivElement | null>(null);
  // Every figure in the panel (diagnostics draw two) saves as one image.
  const save = async (format: ImageFormat) => {
    const svgs = [...(viewport.current?.querySelectorAll<SVGSVGElement>('svg[role="img"]') ?? [])];
    if (svgs.length === 0) throw new Error('There is no plot to save.');
    const parts = svgs.map(svg => serializeSvg(svg));
    return saveSvgPart(parts.length === 1 ? parts[0]! : combineSvgParts(parts, parts.length), title, format);
  };
  return <section className={`card cluster-plot-panel${expanded ? ' expanded' : ''}`} aria-label={title}>
    <div className="cluster-plot-toolbar">
      <h2 className="card-title">{title}</h2>
      <PlotSaveButtons name={title} save={save} />
      <button type="button" className="btn btn-sm btn-ghost" aria-expanded={expanded} aria-controls={id} onClick={() => setExpanded(value => !value)}>
        {expanded ? 'Reduce plot view' : 'Expand plot view'}
      </button>
    </div>
    <div id={id} ref={viewport} className="cluster-plot-viewport" role="region" aria-label={`${title} plot area`} tabIndex={0}>
      {children}
    </div>
  </section>;
}

export function AnalysisPlots({ result, cutK: controlledCutK, onCutKChange, disabled = false, rowLabel }: {
  result: AnalysisResult;
  cutK?: number;
  disabled?: boolean;
  onCutKChange?: (value: number) => void;
  /** Sample label (ANID) by hidden identity and input row index. */
  rowLabel?: (uuid: string | undefined, index: number) => string;
}): ReactElement | null {
  const [localCutK, setLocalCutK] = useState(2);
  const cutK = controlledCutK ?? localCutK;
  const setCutK = onCutKChange ?? setLocalCutK;
  const [leafSize, setLeafSize] = useState(12);
  if (result.kind === 'diagnostics') {
    return <PlotPanel title="Cluster diagnostics"><ClusterDiagnosticsPlots result={result.data} /></PlotPanel>;
  }
  const ids = result.kind === 'fit' ? result.data.analytical_uuids ?? [] : [];
  const labelOf = (row: number) => rowLabel ? rowLabel(ids[row - 1], row - 1) : `Analytical unit ${row}`;
  if (result.kind === 'fit' && result.data.cluster) return <PlotPanel title="Partition clusters"><PartitionPlot result={result.data} labelOf={labelOf} /></PlotPanel>;
  if (result.kind !== 'fit' || !['hclust', 'hclust_ward_d2', 'diana'].includes(result.data.method)) return null;
  const count = result.data.n_rows;
  const boundedK = Math.max(1, Math.min(count, cutK));
  return <section aria-label="Hierarchical cluster plots">
    <div className="cluster-plot-toolbar form-inline">
      <label>Cut into clusters <input disabled={disabled} type="number" min={1} max={count} value={boundedK}
        onChange={e => { const value = Number(e.target.value); if (Number.isInteger(value) && value >= 1 && value <= count) setCutK(value); }} /></label>
      <label>Leaf text size <input type="range" min={8} max={20} value={leafSize}
        onChange={e => setLeafSize(Number(e.target.value))} /> {leafSize}px</label>
    </div>
    <p className="muted">Colors show the {boundedK}-cluster cut. Cutting the tree does not change the group file.</p>
    <PlotPanel title={result.data.method === 'diana' ? 'DIANA dendrogram' : `${result.data.linkage ?? 'ward_d2'} dendrogram`}>
      <Dendrogram result={result.data} cutK={boundedK} leafSize={leafSize} labelOf={labelOf} />
    </PlotPanel>
  </section>;
}
