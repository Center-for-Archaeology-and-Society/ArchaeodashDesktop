import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { AnalysisPage, ResultTable, type AnalysisDeps } from './AnalysisPage.tsx';

test('all analysis routes disable execution until a dataset is available', () => {
  for (const kind of ['cluster', 'membership', 'euclidean'] as const) {
    const html = renderToStaticMarkup(<AnalysisPage kind={kind} deps={{} as AnalysisDeps} />);
    assert.match(html, /Choose a group file/);
    assert.match(html, /button disabled=""/);
    assert.doesNotMatch(html, /workflow lands/);
  }
});

test('Euclidean results hide internal keys and bound the initially rendered table', () => {
  const html = renderToStaticMarkup(<ResultTable result={{ kind: 'euclidean', data: {
    path: 'private.parquet', revision_id: 'private-revision', rows: Array.from({ length: 101 }, (_, i) => ({
      analytical_uuid: 'hidden-observation-uuid', match_analytical_uuid: 'hidden-match-uuid', rowid: 'hidden-uuid', id: `visible-${i}`, match_id: 'match', distance: 1, group: 'A', match_group: 'B',
    })),
  } }} />);
  assert.doesNotMatch(html, /hidden-uuid|private-revision|private.parquet|visible-100/);
  assert.match(html, /visible-99/);
  assert.match(html, /Show 100 more rows/);
});

test('membership fallback visibly labels distances and renders missing cells', () => {
  const html = renderToStaticMarkup(<ResultTable result={{ kind: 'membership', data: {
    analytical_uuids: ['hidden-membership-uuid'], path: 'hidden-path', revision_id: 'hidden-revision', effective_method: 'mahalanobis',
    requested_method: 'hotellings', fallback_reason: 'hotellings_computation_failed',
    eligible_groups: ['A'], ids: ['sample'], groups: ['A'], probabilities: [[null]],
    best_group: [null], best_value: [null], in_group: [false],
  } }} />);
  assert.match(html, /Mahalanobis distances \(lower is closer\)/);
  assert.match(html, /role="status">Hotelling probabilities were unavailable/);
  assert.match(html, /hotellings computation failed/);
  assert.match(html, /Unavailable/);
  assert.doesNotMatch(html, /hidden-path|hidden-revision/);
});

test('hierarchical results show merge heights and diagnostic rows use one-based k', () => {
  const html = renderToStaticMarkup(<ResultTable result={{ kind: 'fit', data: {
    analytical_uuids: ['hidden-cluster-uuid-1', 'hidden-cluster-uuid-2'], path: '', revision_id: '', method: 'diana', n_rows: 2, cluster: null, size: null,
    tot_withinss: null, centers: null, medoids: null, merge: [[-1, -2]], height: [3],
    order: [1, 2], silhouette: null,
  } }} />);
  assert.match(html, /Merge step/);
  assert.match(html, /<td>1<\/td><td>-1<\/td><td>-2<\/td><td>3<\/td>/);
  const diag = renderToStaticMarkup(<ResultTable result={{ kind: 'diagnostics', data: {
    path: '', revision_id: '', column_names: ['x'], n_rows: 3, wss: [20, 3], silhouette: [0.5],
  } }} />);
  assert.match(diag, /<td>2<\/td><td>3<\/td><td>0.5<\/td>/);
});
