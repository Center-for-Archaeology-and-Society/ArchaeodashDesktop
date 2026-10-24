import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { AnalysisPage, ResultTable, type AnalysisDeps } from './AnalysisPage.tsx';

test('all analysis routes disable execution until a dataset is available', () => {
  for (const kind of ['cluster', 'membership', 'euclidean'] as const) {
    const html = renderToStaticMarkup(<AnalysisPage kind={kind} deps={{} as AnalysisDeps} />);
    assert.match(html, /Dataset: No groups/);
    assert.match(html, /<button type="submit" class="btn btn-primary" disabled="">/);
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
  assert.match(html, /role="status"[^>]*>Hotelling probabilities were unavailable/);
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
  // Merge branches name samples (ANID when known), not hclust's -1/-2 codes.
  assert.match(html, /<td class="num">1<\/td><td>Unit 1<\/td><td>Unit 2<\/td><td class="num">3<\/td>/);
  const labelled = renderToStaticMarkup(<ResultTable rows={{ idColumn: 'ANID', label: (_u, i) => `AN${i + 1}`, group: () => 'G' }} result={{ kind: 'fit', data: {
    analytical_uuids: ['u1', 'u2'], path: '', revision_id: '', method: 'kmeans', n_rows: 2, cluster: [1, 2], size: [1, 1],
    tot_withinss: null, centers: null, medoids: null, merge: null, height: null, order: null, silhouette: [0.5, 0.25],
  } }} />);
  assert.match(labelled, />ANID<span[^>]*><\/span><\/button><\/th><th><button[^>]*>Group<span/);
  assert.match(labelled, /<td>AN1<\/td><td>G<\/td><td class="num">1<\/td><td class="num">0.5<\/td>/);
  assert.doesNotMatch(labelled, /u1|Analytical unit/);
  const diag = renderToStaticMarkup(<ResultTable result={{ kind: 'diagnostics', data: {
    path: '', revision_id: '', column_names: ['x'], n_rows: 3, wss: [20, 3], silhouette: [0.5],
  } }} />);
  assert.match(diag, /<td class="num">2<\/td><td class="num">3<\/td><td class="num">0.5<\/td>/);
});

const uuidA = '01970000-0000-7000-8000-00000000000a';
const uuidB = '01970000-0000-7000-8000-00000000000b';

test('membership results check rows in-table, round values, hide Best value and show the fallback warning', () => {
  const html = renderToStaticMarkup(<ResultTable selection={{ selected: new Set([uuidB]), onChange: () => {} }} result={{ kind: 'membership', data: {
    analytical_uuids: [uuidA, uuidB], path: 'hidden-path', revision_id: 'hidden-revision', effective_method: 'hotellings',
    eligible_groups: ['North', 'South'], ids: ['S1', 'S2'], groups: ['North', 'South'], probabilities: [[81.23456, 18.7], [null, 99.999]],
    best_group: ['North', 'South'], best_value: [81.23456, 99.999], in_group: [true, true], projection_included: [true, false],
    eligibility_warning: 'No group has enough samples to be eligible; using all groups.',
  } }} />);
  assert.match(html, /role="status"[^>]*>No group has enough samples/);
  assert.match(html, /aria-label="Select S1"/);
  assert.match(html, /aria-label="Select S2" checked=""/);
  assert.match(html, /aria-label="Select all 2 rows"/);
  assert.match(html, /<td class="num">81.23<\/td><td class="num">18.70<\/td>/);
  assert.match(html, /aria-label="Filter North"/);
  assert.match(html, /Compact table/);
  assert.doesNotMatch(html, />Best value</);
  assert.doesNotMatch(html, new RegExp(`${uuidA}|${uuidB}|hidden-path|hidden-revision`));
});

test('Euclidean match rows are checked individually without exposing identities', () => {
  const html = renderToStaticMarkup(<ResultTable selection={{ selected: new Set([`${uuidA}#2`]), onChange: () => {} }} result={{ kind: 'euclidean', data: {
    path: 'p', revision_id: 'r', rows: [
      { analytical_uuid: uuidA, match_analytical_uuid: uuidB, rowid: 'r1', id: 'S1', match_id: 'M1', distance: 1.234567, group: 'A', match_group: 'North' },
      { analytical_uuid: uuidA, match_analytical_uuid: uuidB, rowid: 'r1', id: 'S1', match_id: 'M2', distance: 2, group: 'A', match_group: 'South' },
    ],
  } }} />);
  assert.match(html, /aria-label="Select S1 match M1"/);
  assert.match(html, /aria-label="Select S1 match M2" checked=""/);
  assert.match(html, /<td class="num">1.2346<\/td>/);
  assert.doesNotMatch(html, new RegExp(`${uuidA}|${uuidB}`));
});
