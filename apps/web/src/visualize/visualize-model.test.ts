/** Visualize-model unit tests: selection, filter, symbols, labels, ellipse. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  chiSquare2,
  clearSelection,
  colorFor,
  ellipsePoints,
  labelFor,
  normalizeFilterValue,
  replaceSelection,
  rowPassesFilter,
  symbolFor,
  toggleUuid,
} from './visualize-model.ts';

test('ten-symbol map repeats conservatively and never yields undefined', () => {
  assert.equal(symbolFor(0), 'circle');
  assert.equal(symbolFor(9), 'hourglass');
  assert.equal(symbolFor(10), 'circle');
  assert.equal(symbolFor(-1), 'hourglass');
});

test('viridis colors cycle by group index', () => {
  assert.equal(colorFor(0), '#440154');
  assert.equal(colorFor(9), '#fde725');
  assert.equal(colorFor(10), '#440154');
});

test('(Missing) normalization covers null, undefined, blank, and literal', () => {
  assert.equal(normalizeFilterValue(null), '(Missing)');
  assert.equal(normalizeFilterValue(undefined), '(Missing)');
  assert.equal(normalizeFilterValue('  '), '(Missing)');
  assert.equal(normalizeFilterValue('Baca'), 'Baca');
});

test('metadata filter matches normalized cells and passes empty filters', () => {
  const row = { analytical_uuid: 'u1', descriptive: ['Baca', null] };
  assert.ok(rowPassesFilter(row, 0, 'Baca'));
  assert.ok(!rowPassesFilter(row, 0, 'Other'));
  assert.ok(rowPassesFilter(row, 1, '(Missing)'));
  assert.ok(rowPassesFilter(row, 1, null));
  assert.ok(rowPassesFilter(row, 0, null), 'null filter passes everything');
});

test('label fallback chain: anid / sample id / row number', () => {
  const row = { analytical_uuid: 'u-42', visible_id: 'A1' };
  assert.equal(labelFor(row, 'anid', 3), 'u-42');
  assert.equal(labelFor(row, 'sampleId', 3), 'A1');
  assert.equal(labelFor({ analytical_uuid: 'u-42', visible_id: null }, 'sampleId', 3), 'Row 3');
  assert.equal(labelFor(row, 'rowNumber', 3), '3');
});

test('selection replace/toggle/clear keyed by uuid', () => {
  let sel = replaceSelection(['u1', 'u2']);
  assert.equal(sel.uuids.size, 2);
  sel = toggleUuid(sel, 'u2');
  assert.deepEqual([...sel.uuids], ['u1']);
  sel = toggleUuid(sel, 'u3');
  assert.equal(sel.uuids.size, 2);
  assert.deepEqual([...clearSelection(sel).uuids], []);
});

test('chi-square quantile for 2 dof matches legacy values', () => {
  assert.ok(Math.abs(chiSquare2(0.5) - 1.3862943611198906) < 1e-12);
  assert.ok(Math.abs(chiSquare2(0.95) - 5.991464547107979) < 1e-12);
  assert.ok(Math.abs(chiSquare2(0.99) - 9.21034037197618) < 1e-12);
  assert.throws(() => chiSquare2(0));
  assert.throws(() => chiSquare2(1));
});

test('ellipse outlines a closed path around the data mean', () => {
  const x = [1, 2, 3, 4, 5];
  const y = [2, 4, 3, 5, 6];
  const { px, py } = ellipsePoints(x, y, 0.95);
  assert.equal(px.length, 37);
  assert.equal(py.length, 37);
  // Closed path.
  assert.equal(px[0], px[36]);
  assert.equal(py[0], py[36]);
  // Centered near the mean (3, 4).
  const cx = (Math.min(...px) + Math.max(...px)) / 2;
  const cy = (Math.min(...py) + Math.max(...py)) / 2;
  assert.ok(Math.abs(cx - 3) < 1e-9, `ellipse x-center ${cx} near mean 3`);
  assert.ok(Math.abs(cy - 4) < 1e-9, `ellipse y-center ${cy} near mean 4`);
  // Higher level strictly encloses lower level.
  const half = ellipsePoints(x, y, 0.5);
  assert.ok(Math.max(...px) > Math.max(...half.px));
});

test('ellipse of fewer than two points is empty', () => {
  assert.deepEqual(ellipsePoints([1], [2], 0.95), { px: [], py: [] });
});
