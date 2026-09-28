/** Multiplot model tests: deterministic sampling, disjoint pairs, svg data url. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { allPairs, samplingPlan, samplingStatusText, svgToDataUrl, uuidsFromSelectedEvent } from './multiplot-model.ts';

test('sampling keeps every row at or below the 100k ceiling', () => {
  const plan = samplingPlan(100_000);
  assert.equal(plan.sampled, false);
  assert.equal(plan.indices.length, 100_000);
  assert.equal(plan.stride, 1);
  assert.equal(plan.indices[0], 0);
  assert.equal(plan.indices[99_999], 99_999);
});

test('sampling above the ceiling is deterministic stride-from-zero', () => {
  const plan = samplingPlan(250_000);
  assert.equal(plan.sampled, true);
  assert.equal(plan.stride, 3);
  assert.equal(plan.indices.length, 83_334);
  assert.equal(plan.indices[0], 0);
  assert.equal(plan.indices[1], 3);
  assert.equal(plan.indices.at(-1), 249_999);
  // Same input, same set.
  assert.deepEqual(samplingPlan(250_000).indices, plan.indices);
});

test('sampling is idempotent for tiny and degenerate inputs', () => {
  assert.deepEqual(samplingPlan(0).indices, []);
  assert.deepEqual(samplingPlan(2).indices, [0, 1]);
});

test('allPairs enumerates disjoint ordered pairs', () => {
  assert.deepEqual(allPairs(3), [
    { xIndex: 1, yIndex: 0 },
    { xIndex: 2, yIndex: 0 },
    { xIndex: 0, yIndex: 1 },
    { xIndex: 2, yIndex: 1 },
    { xIndex: 0, yIndex: 2 },
    { xIndex: 1, yIndex: 2 },
  ]);
  assert.deepEqual(allPairs(0), []);
});

test('svg serialization yields a downloadable data url', () => {
  const fakeSerializer = {
    serializeToString: (el: Element) => `<svg data-name="${(el as unknown as { id: string }).id}"/>`,
  };
  const el = { id: 'panel-1' } as unknown as Element;
  const url = svgToDataUrl(fakeSerializer, el);
  assert.ok(url.startsWith('data:image/svg+xml;charset=utf-8,'));
  assert.ok(decodeURIComponent(url).includes('panel-1'));
});

test('sampling status text appears only when the ceiling dropped points', () => {
  assert.equal(samplingStatusText(samplingPlan(100_000), 100_000), null);
  assert.equal(samplingStatusText(samplingPlan(0), 0), null);
  const plan = samplingPlan(100_001);
  assert.equal(plan.stride, 2);
  assert.equal(
    samplingStatusText(plan, 100_001),
    'Sampled 50001 of 100001 points (stride 2, deterministic)',
  );
});

test('selection uuids are extracted from plotly_selected customdata only', () => {
  const event = {
    points: [
      { customdata: ['uuid-1', 'Baca'] },
      { customdata: ['uuid-2', 'Other'] },
      { customdata: ['', 'skipped'] },
      { x: 1, y: 2 },
    ],
  };
  assert.deepEqual(uuidsFromSelectedEvent(event), ['uuid-1', 'uuid-2']);
  assert.deepEqual(uuidsFromSelectedEvent(null), []);
  assert.deepEqual(uuidsFromSelectedEvent({ points: [] }), []);
});
