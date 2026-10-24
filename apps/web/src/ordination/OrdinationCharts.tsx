/**
 * Ordination charts beyond the score scatter: the PCA variables plot coloured
 * by contribution, the contribution table, and the LDA vector view with its
 * one-dimensional alternative (Section 8.5/8.7, IMPLEMENTATION.md §3.2).
 */
import { useMemo, type ReactElement } from 'react';
import type { Data, Layout } from 'plotly.js-dist-min';
import { PlotlyChart } from '../charts/PlotlyChart.tsx';
import { plotlyText } from '../charts/plotly-text.ts';
import { groupColors } from '../visualize/visualize-model.ts';
import { TableCard, num, type TableSheet } from '../ui/TableExport.tsx';
import {
  GRADIENT_SCALE,
  contributionRule,
  contributionTable,
  gradientColor,
  indicesByGroup,
  ldaVectors,
  normalize,
  variableContrib,
  variableCoords,
} from './ordination-model.ts';

/** Unit circle as a closed line (drawn for scaled PCA, as factoextra does). */
function unitCircle(): Data {
  const t = Array.from({ length: 121 }, (_, i) => (i / 120) * 2 * Math.PI);
  return {
    type: 'scatter',
    mode: 'lines',
    x: t.map(Math.cos),
    y: t.map(Math.sin),
    line: { color: 'rgba(128,128,128,0.6)', width: 1 },
    hoverinfo: 'skip',
    showlegend: false,
  } as Data;
}

/** Arrow annotations from the origin to each end point. */
function arrows(points: readonly { x: number; y: number; color: string }[]): Partial<Layout>['annotations'] {
  return points.map((p) => ({
    x: p.x,
    y: p.y,
    ax: 0,
    ay: 0,
    xref: 'x',
    yref: 'y',
    axref: 'x',
    ayref: 'y',
    text: '',
    showarrow: true,
    arrowhead: 2,
    arrowsize: 1,
    arrowwidth: 1.6,
    arrowcolor: p.color,
  })) as Partial<Layout>['annotations'];
}

/** Symmetric axis range covering every end point (and the unit circle when drawn). */
function span(values: readonly number[], floor = 0): [number, number] {
  const m = Math.max(floor, ...values.map((v) => (Number.isFinite(v) ? Math.abs(v) : 0))) * 1.15 || 1;
  return [-m, m];
}

/**
 * PCA variables plot (legacy `fviz_pca_var(col.var = "contrib")`): arrows to
 * `rotation · sdev` on the first two components, coloured by the
 * eigenvalue-weighted contribution to both.
 */
export function PcaVariablesPlot({
  rotation,
  sdev,
  names,
  xLabel,
  yLabel,
  scaled,
}: {
  rotation: readonly (readonly number[])[];
  sdev: readonly number[];
  names: readonly string[];
  xLabel: string;
  yLabel: string;
  /** Unit-variance PCA: coordinates are correlations, so draw the unit circle. */
  scaled: boolean;
}): ReactElement {
  const { data, layout } = useMemo(() => {
    const x = variableCoords(rotation, sdev, 0);
    const y = variableCoords(rotation, sdev, 1);
    const contrib = variableContrib(rotation, sdev, 0, 1);
    const t = normalize(contrib);
    const labels = rotation.map((_, j) => plotlyText(names[j] ?? `Column ${j + 1}`));
    const points: Data = {
      type: 'scatter',
      mode: 'text+markers',
      x,
      y,
      text: labels,
      textposition: x.map((v) => (v >= 0 ? 'middle right' : 'middle left')),
      customdata: contrib,
      hovertemplate: `%{text}<br>${xLabel}: %{x:.3f}<br>${yLabel}: %{y:.3f}<br>contrib: %{customdata:.2f}%<extra></extra>`,
      marker: {
        size: 5,
        color: contrib,
        colorscale: GRADIENT_SCALE,
        showscale: true,
        colorbar: { title: { text: 'contrib' }, thickness: 12 },
      },
      textfont: { color: t.map(gradientColor) },
      showlegend: false,
    } as Data;
    const xr = span(x, scaled ? 1 : 0);
    const yr = span(y, scaled ? 1 : 0);
    const lay: Partial<Layout> = {
      xaxis: { title: { text: xLabel }, zeroline: true, range: xr },
      yaxis: { title: { text: yLabel }, zeroline: true, range: yr, scaleanchor: 'x', scaleratio: 1 },
      annotations: arrows(x.map((xv, j) => ({ x: xv, y: y[j] ?? 0, color: gradientColor(t[j] ?? 0) }))),
      showlegend: false,
    };
    return { data: scaled ? [unitCircle(), points] : [points], layout: lay };
  }, [rotation, sdev, names, xLabel, yLabel, scaled]);
  return <PlotlyChart data={data} layout={layout} ariaLabel="PCA variables coloured by contribution" saveName="PCA variables" />;
}

/** Contribution table: summed contributions over the first min(4, K) components. */
export function ContributionTableCard({
  rotation,
  names,
  scoreNames,
}: {
  rotation: readonly (readonly number[])[];
  names: readonly string[];
  scoreNames: readonly string[];
}): ReactElement {
  const table = useMemo(() => contributionTable(rotation, names), [rotation, names]);
  const rule = contributionRule(table.components, scoreNames);
  const sheet: TableSheet = {
    name: 'Contributions',
    columns: ['Variable', 'Contribution (%)'],
    rows: table.rows.map(([name, value]) => [name, num(value)]),
  };
  return (
    <TableCard title="Variable contributions" fileName="PCA contributions" sheets={() => [sheet]} note={rule}>
      <table className="data-table numeric">
        <caption className="visually-hidden">{rule}</caption>
        <thead>
          <tr>
            <th>Variable</th>
            <th>Contribution (%)</th>
          </tr>
        </thead>
        <tbody>
          {table.rows.map(([name, value]) => (
            <tr key={name}>
              <td>{name}</td>
              <td>{value.toFixed(2)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </TableCard>
  );
}

/** LD1/LD2 vector plot (legacy `plotLDAvectors`): arrows to each variable's scaling. */
export function LdaVectorPlot({
  scaling,
  names,
  axisNames,
}: {
  scaling: readonly (readonly number[])[];
  names: readonly string[];
  axisNames: readonly [string, string];
}): ReactElement {
  const { data, layout } = useMemo(() => {
    const vectors = ldaVectors(scaling, names);
    const x = vectors.map((v) => v.x);
    const y = vectors.map((v) => v.y);
    const labels: Data = {
      type: 'scatter',
      mode: 'text',
      x,
      y,
      text: vectors.map((v) => plotlyText(v.variable)),
      textposition: x.map((v) => (v >= 0 ? 'middle right' : 'middle left')),
      textfont: { color: '#AA0317', size: 13 },
      hovertemplate: `%{text}<br>${axisNames[0]}: %{x:.3f}<br>${axisNames[1]}: %{y:.3f}<extra></extra>`,
      showlegend: false,
    } as Data;
    const lay: Partial<Layout> = {
      xaxis: { title: { text: axisNames[0] }, zeroline: true, range: span(x) },
      yaxis: { title: { text: axisNames[1] }, zeroline: true, range: span(y) },
      annotations: arrows(vectors.map((v) => ({ x: v.x, y: v.y, color: '#4A90E2' }))),
      showlegend: false,
    };
    return { data: [labels], layout: lay };
  }, [scaling, names, axisNames]);
  return <PlotlyChart data={data} layout={layout} ariaLabel={`LDA vectors on ${axisNames[0]} and ${axisNames[1]}`} saveName="LDA vectors" />;
}

/** One-discriminant scores: per-group box with every point (jittered strip). */
export function LdaStripPlot({
  values,
  groups,
  ids,
  axisName,
}: {
  values: readonly number[];
  groups?: readonly string[];
  ids?: readonly string[];
  axisName: string;
}): ReactElement {
  const { data, layout } = useMemo(() => {
    const byGroup = indicesByGroup(values.length, groups);
    const colors = byGroup.length === 1 ? ['#1f8a63'] : groupColors(byGroup.length);
    const traces = byGroup.map(([name, idx], gi) => ({
      type: 'box',
      orientation: 'h',
      name: plotlyText(name),
      x: idx.map((i) => values[i]),
      text: idx.map((i) => plotlyText(ids?.[i] || `Row ${i + 1}`)),
      boxpoints: 'all',
      jitter: 0.5,
      pointpos: 0,
      hovertemplate: `%{text} · ${plotlyText(name)}<br>${axisName}: %{x:.3f}<extra></extra>`,
      marker: { color: colors[gi], size: 6, opacity: 0.8 },
      line: { color: colors[gi] },
      fillcolor: 'rgba(0,0,0,0)',
    })) as Data[];
    const lay: Partial<Layout> = {
      xaxis: { title: { text: axisName }, zeroline: true },
      yaxis: { automargin: true },
      showlegend: false,
    };
    return { data: traces, layout: lay };
  }, [values, groups, ids, axisName]);
  return <PlotlyChart data={data} layout={layout} ariaLabel={`${axisName} scores by group`} saveName={`LDA ${axisName} scores`} />;
}

/** One-discriminant coefficients: bar chart of each variable's scaling. */
export function LdaScalingBars({
  scaling,
  names,
  axisName,
}: {
  scaling: readonly (readonly number[])[];
  names: readonly string[];
  axisName: string;
}): ReactElement {
  const { data, layout } = useMemo(() => {
    const vectors = ldaVectors(scaling, names);
    const trace = {
      type: 'bar',
      x: vectors.map((v) => plotlyText(v.variable)),
      y: vectors.map((v) => v.x),
      marker: { color: '#4A90E2' },
      hovertemplate: `%{x}: %{y:.3f}<extra></extra>`,
    } as Data;
    const lay: Partial<Layout> = {
      xaxis: { title: { text: 'Variable' } },
      yaxis: { title: { text: `${axisName} coefficient` }, zeroline: true },
      showlegend: false,
    };
    return { data: [trace], layout: lay };
  }, [scaling, names, axisName]);
  return <PlotlyChart data={data} layout={layout} ariaLabel={`${axisName} coefficients by variable`} saveName={`LDA ${axisName} coefficients`} height={320} />;
}
