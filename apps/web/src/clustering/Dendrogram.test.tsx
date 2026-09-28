import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { Dendrogram } from './Dendrogram.tsx';

test('dendrogram exposes leaf ordinals and cut groups without row identifiers', () => {
  const result = { path: 'private.parquet', revision_id: 'private-revision', method: 'hclust_ward_d2' as const, n_rows: 3,
    merge: [[-2, -3], [-1, 1]] as [number, number][], height: [1, 4], order: [1, 2, 3] };
  const markup = renderToStaticMarkup(<Dendrogram result={result} cutK={2} leafSize={20} />);
  assert.match(markup, /role="img"/);
  assert.match(markup, /Merge height/);
  assert.match(markup, /Leaf order/);
  assert.match(markup, /Analytical unit 1 · Cluster 1/);
  assert.match(markup, /Analytical unit 2 · Cluster 2/);
  assert.match(markup, /Analytical unit 3 · Cluster 2/);
  assert.doesNotMatch(markup, /private\.parquet|private-revision/);
});

test('invalid clustering output renders an explicit unavailable state', () => {
  const markup = renderToStaticMarkup(<Dendrogram result={{ path: '', revision_id: '', method: 'diana', n_rows: 2,
    merge: null, height: null, order: null }} />);
  assert.match(markup, /Dendrogram unavailable for this result/);
});

test('multiple dendrograms have distinct accessible IDs and extreme finite heights remain readable', () => {
  const result = { path: '', revision_id: '', method: 'diana' as const, n_rows: 2,
    merge: [[-1, -2]] as [number, number][], height: [Number.MAX_VALUE], order: [1, 2] };
  const markup = renderToStaticMarkup(<><Dendrogram result={result} /><Dendrogram result={result} /></>);
  const ids = [...markup.matchAll(/ id="([^"]+)"/g)].map(match => match[1]);
  assert.equal(new Set(ids).size, ids.length);
  assert.doesNotMatch(markup, /NaN|Infinity/);
});
