import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import type { ClusterFitResponse } from '@archaeodash/client';
import { layoutDendrogram } from './dendrogram-model.ts';

const fixture = JSON.parse(readFileSync(new URL('../../../../fixtures/golden/09_clustering.json', import.meta.url), 'utf8')) as {
  hclust: Pick<ClusterFitResponse, 'merge' | 'height' | 'order'>;
  diana: Pick<ClusterFitResponse, 'merge' | 'height' | 'order'>;
};

test('Ward.D2 and DIANA golden trees lay out every leaf and branch with a five-cluster cut', () => {
  for (const method of ['hclust', 'diana'] as const) {
    const tree = fixture[method];
    const layout = layoutDendrogram({ n_rows: 307, ...tree }, 5);
    assert.ok(layout, `${method} tree should be valid`);
    assert.equal(layout.leaves.length, 307);
    assert.equal(layout.segments.length, 3 * 306);
    assert.equal(new Set(layout.leaves.map(leaf => leaf.row)).size, 307);
    assert.equal(new Set(layout.leaves.map(leaf => leaf.group)).size, 5);
    assert.ok(layout.height > 307 * 24, 'leaf rows fit the scrollable plot');
  }
});

test('rejects malformed, disconnected, cyclic, and crossing merge topology without throwing', () => {
  const base = { n_rows: 3, merge: [[-1, -2], [1, -3]] as [number, number][], height: [1, 2], order: [1, 2, 3] };
  assert.ok(layoutDendrogram(base));
  assert.equal(layoutDendrogram({ ...base, merge: [[-1, -1], [1, -3]] }), null);
  assert.equal(layoutDendrogram({ ...base, merge: [[-1, -2], [2, -3]] }), null);
  assert.equal(layoutDendrogram({ ...base, order: [1, 3, 2] }), null);
  assert.equal(layoutDendrogram({ ...base, order: [1, 1, 3] }), null);
  assert.equal(layoutDendrogram({ ...base, height: [1, Number.NaN] }), null);
  assert.equal(layoutDendrogram({ ...base, merge: null }), null);
  assert.equal(layoutDendrogram({ n_rows: 1, merge: [], height: [], order: [1] }), null);
});

test('cut size is bounded to a valid integer', () => {
  const base = { n_rows: 2, merge: [[-1, -2]] as [number, number][], height: [1], order: [1, 2] };
  assert.equal(layoutDendrogram(base, 0), null);
  assert.equal(layoutDendrogram(base, 1.5), null);
  assert.equal(layoutDendrogram(base, 3), null);
  assert.equal(new Set(layoutDendrogram(base, 2)?.leaves.map(leaf => leaf.group)).size, 2);
});

test('zero-height branches meet the leaf labels and invalid negative heights are rejected', () => {
  const tree = { n_rows: 2, merge: [[-1, -2]] as [number, number][], height: [0], order: [1, 2] };
  const layout = layoutDendrogram(tree, 1)!;
  assert.ok(layout.segments.every(segment => segment.x1 === layout.plotRight && segment.x2 === layout.plotRight));
  assert.equal(layoutDendrogram({ ...tree, height: [-1] }), null);
});

test('the full service row limit renders without sampling at either cut extreme', () => {
  const n = 1000;
  const merge: [number, number][] = [[-1, -2]];
  for (let row = 3; row <= n; row++) merge.push([row - 2, -row]);
  const tree = { n_rows: n, merge, height: merge.map((_, i) => i), order: Array.from({ length: n }, (_, i) => i + 1) };
  for (const k of [1, n]) {
    const layout = layoutDendrogram(tree, k)!;
    assert.equal(layout.leaves.length, n);
    assert.equal(new Set(layout.leaves.map(leaf => leaf.group)).size, k);
  }
});
