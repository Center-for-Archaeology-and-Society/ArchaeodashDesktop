import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ClusterDiagnosticsResponse } from '@archaeodash/client';
import { ClusterDiagnosticsPlots } from './ClusterDiagnosticsPlots.tsx';

function render(wss: unknown, silhouette: unknown): string {
  return renderToStaticMarkup(<ClusterDiagnosticsPlots result={{
    path: '', revision_id: '', column_names: [], n_rows: 0,
    wss, silhouette,
  } as ClusterDiagnosticsResponse} />);
}

test('plots align WSS to k=1 and silhouette to k=2 with accessible point titles', () => {
  const html = render([12, 5], [0.4, 0.6]);
  assert.match(html, /data-k="1"/);
  assert.match(html, /data-k="2"/);
  assert.match(html, /k=1, within-cluster sum of squares \(wss\)=12/);
  assert.match(html, /k=2, mean silhouette width=0.4/);
  assert.match(html, /Number of clusters \(k\)/);
  assert.match(html, /font-size="12"/);
  assert.match(html, />0\.4<\/text>/);
  assert.match(html, />0\.5<\/text>/);
  assert.match(html, />0\.6<\/text>/);
  assert.match(html, />WSS<\/text>/);
});

test('missing and malformed values leave line gaps instead of joining across them', () => {
  const html = render([8, Number.NaN, 3, 2], [0.2, null, 0.5, Number.POSITIVE_INFINITY]);
  const paths = [...html.matchAll(/<path\b/g)];
  assert.equal(paths.length, 1, 'only the contiguous WSS tail should have a line');
  assert.match(html, /data-k="3"/);
  assert.match(html, /data-k="4"/);
  assert.doesNotMatch(html, /NaN|Infinity/);
  assert.equal((html.match(/<circle\b/g) ?? []).length, 5);
});

test('constant domains remain finite and empty series show unavailable text', () => {
  const constant = render([4, 4, 4], [0.5, 0.5]);
  assert.doesNotMatch(constant, /NaN|Infinity/);
  assert.equal((constant.match(/<circle\b/g) ?? []).length, 5);
  assert.equal([...constant.matchAll(/<circle\b[^>]*cy="([^"]+)"/g)].every(match => Number(match[1]) === 118), true,
    'constant series points sit at the center of the plot domain');
  const empty = render([], [null, Number.NaN]);
  assert.match(empty, /WSS values are unavailable for plotting/);
  assert.match(empty, /Mean silhouette values are unavailable for plotting/);
  assert.doesNotMatch(empty, /<svg\b/);
});

test('extreme finite values retain finite SVG geometry and readable exponent ticks', () => {
  const html = render([-Number.MAX_VALUE, Number.MAX_VALUE], [Number.MAX_VALUE, -Number.MAX_VALUE]);
  assert.doesNotMatch(html, /NaN|Infinity/);
  assert.match(html, /1\.798e\+308/);
  const geometry = [...html.matchAll(/\s(?:cx|cy|x|y)="([^"]+)"/g)].map(match => Number(match[1]));
  assert.equal(geometry.every(Number.isFinite), true);
});

test('non-array malformed series are reported as unavailable safely', () => {
  const html = render({ value: 4 }, null);
  assert.match(html, /WSS values are unavailable for plotting/);
  assert.match(html, /Mean silhouette values are unavailable for plotting/);
  assert.doesNotMatch(html, /NaN|Infinity/);
});
