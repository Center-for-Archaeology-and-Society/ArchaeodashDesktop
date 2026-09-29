import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ClusterFitResponse } from '@archaeodash/client';
import { PartitionPlot } from './PartitionPlot.tsx';

test('partition plot uses cluster projection and keeps identities out of SVG', () => {
  const result: ClusterFitResponse = { path: 'hidden-path', revision_id: 'hidden-revision', analytical_uuids: ['hidden-a', 'hidden-b'], method: 'pam', n_rows: 2, cluster: [1, 2], plot_coordinates: [[100, 200], [200, 400]], plot_column_names: ['a', 'b'], cluster_plot_coordinates: [[-1, -2], [1, 2]], cluster_plot_column_names: ['PC1', 'PC2'], plot_groups: ['A', 'B'] };
  const html = renderToStaticMarkup(<PartitionPlot result={result} />);
  assert.match(html, /PC1 \/ PC2/); assert.match(html, /Cluster 1/); assert.match(html, /Existing groups/);
  assert.doesNotMatch(html, /hidden-a|hidden-b|hidden-path|hidden-revision/);
  assert.match(html, /Analytical unit 1/);
});
