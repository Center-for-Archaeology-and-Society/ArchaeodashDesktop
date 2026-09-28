import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { AnalysisPlots, PlotPanel } from './AnalysisPlots.tsx';
import type { ClusterFitResponse } from '@archaeodash/client';

const fit: ClusterFitResponse = {
  path: 'hidden-file', revision_id: 'hidden-revision', method: 'hclust_ward_d2', n_rows: 3,
  cluster: null, size: null, tot_withinss: null, centers: null, medoids: null,
  merge: [[-1, -2], [1, -3]], height: [1, 3], order: [1, 2, 3], silhouette: null,
};

test('hierarchical results integrate cut and leaf controls with a focusable expanded viewport', () => {
  const html = renderToStaticMarkup(<AnalysisPlots result={{ kind: 'fit', data: fit }} />);
  assert.match(html, /Cut into clusters/);
  assert.match(html, /Leaf text size/);
  assert.match(html, /Ward.D2 dendrogram/);
  assert.match(html, /tabindex="0"/);
  assert.match(html, /aria-expanded="false"/);
  const controls = html.match(/aria-controls="([^"]+)"/)?.[1];
  assert.ok(controls && html.includes(`id="${controls}"`));
  assert.doesNotMatch(html, /hidden-file|hidden-revision/);
});

test('expanded panel retains the chart and offers a reduction control', () => {
  const html = renderToStaticMarkup(<PlotPanel title="Diagnostic test" initialExpanded><p>Chart data</p></PlotPanel>);
  assert.match(html, /cluster-plot-panel expanded/);
  assert.match(html, /aria-expanded="true"/);
  assert.match(html, /Reduce plot view/);
  assert.match(html, /Chart data/);
});

test('diagnostics integrate both plots while non-hierarchical results do not render a tree', () => {
  const html = renderToStaticMarkup(<AnalysisPlots result={{ kind: 'diagnostics', data: {
    path: 'hidden-file', revision_id: 'hidden-revision', column_names: ['Fe'], n_rows: 4,
    wss: [10, 4, 2], silhouette: [0.3, 0.4],
  } }} />);
  assert.match(html, /Elbow plot/);
  assert.match(html, /Mean silhouette plot/);
  assert.doesNotMatch(html, /hidden-file|hidden-revision/);
  assert.equal(renderToStaticMarkup(<AnalysisPlots result={{ kind: 'fit', data: { ...fit, method: 'pam' } }} />), '');
});
