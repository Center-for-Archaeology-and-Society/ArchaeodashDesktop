/** Visualize-model unit tests: selection, filter, symbols, labels, ellipse. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  chiSquare2,
  clearSelection,
  colorFor,
  ellipsePoints,
  covTrob,
  dark3Colors,
  fQuantile2,
  groupColors,
  paletteColors,
  rowPassesFilterValues,
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

test('ellipse outlines a closed 52-point path around a symmetric centre', () => {
  const x = [0, 2, 0, -2, 0, 1, -1, 1, -1];
  const y = [1, 0, -1, 0, 0, 0.5, 0.5, -0.5, -0.5];
  const { px, py } = ellipsePoints(x, y, 0.95);
  assert.equal(px.length, 52);
  assert.equal(py.length, 52);
  assert.ok(Math.abs(px[0]! - px[51]!) < 1e-9);
  assert.ok(Math.abs(py[0]! - py[51]!) < 1e-9);
  // Evenly spaced points on an ellipse average to its centre.
  const cx = px.slice(0, 51).reduce((a, v) => a + v, 0) / 51;
  const cy = py.slice(0, 51).reduce((a, v) => a + v, 0) / 51;
  assert.ok(Math.abs(cx) < 1e-6, `x-centre ${cx}`);
  assert.ok(Math.abs(cy) < 1e-6, `y-centre ${cy}`);
  // Wider along x, where the data spread more.
  assert.ok(Math.max(...px) > Math.max(...py));
  const half = ellipsePoints(x, y, 0.5);
  assert.ok(Math.max(...px) > Math.max(...half.px));
});

test('ellipse radius uses the F(2, n-1) quantile like stat_ellipse', () => {
  // qf(0.9, 2, 4) = 4.324555...
  assert.ok(Math.abs(fQuantile2(0.9, 4) - 4.32455532033676) < 1e-9);
  // Large n approaches chi-square / 2.
  assert.ok(Math.abs(2 * fQuantile2(0.95, 1e7) - chiSquare2(0.95)) < 1e-4);
});

test('ellipse needs four points with distinct x and y', () => {
  assert.deepEqual(ellipsePoints([1], [2], 0.95), { px: [], py: [] });
  assert.deepEqual(ellipsePoints([1, 2], [2, 3], 0.95), { px: [], py: [] });
  // stat_ellipse: dfd = n - 1 < 3 is "Too few points to calculate an ellipse".
  assert.deepEqual(ellipsePoints([0, 1, 0], [0, 0, 1], 0.9), { px: [], py: [] });
  assert.equal(ellipsePoints([0, 1, 0, 1], [0, 0, 1, 1.5], 0.9).px.length, 52);
  assert.deepEqual(ellipsePoints([1, 1, 1], [1, 2, 3], 0.95), { px: [], py: [] });
  // Collinear points have a singular scatter.
  assert.deepEqual(ellipsePoints([1, 2, 3, 4], [2, 4, 6, 8], 0.9), { px: [], py: [] });
});

test('covTrob downweights an outlier relative to the mean', () => {
  const x = [0, 1, 0, 1, 0.5, 20];
  const y = [0, 0, 1, 1, 0.5, 20];
  const fit = covTrob(x, y)!;
  const meanX = x.reduce((s, v) => s + v, 0) / x.length;
  assert.ok(fit.center[0] < meanX, `robust centre ${fit.center[0]} below mean ${meanX}`);
});

test('Dark 3 palette matches R hcl.colors for two groups', () => {
  // hcl.colors(2, "Dark 3") == c("#E16A86", "#00AD9A")
  const two = dark3Colors(2);
  assert.equal(two.length, 2);
  const close = (hex: string, want: string) => {
    const a = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
    const b = [1, 3, 5].map((i) => parseInt(want.slice(i, i + 2), 16));
    return a.every((v, i) => Math.abs(v - b[i]!) <= 2);
  };
  assert.ok(close(two[0]!, '#E16A86'), two[0]);
  assert.ok(close(two[1]!, '#00AD9A'), two[1]);
  assert.deepEqual(paletteColors('viridis', 3), groupColors(3));
});

test('multi-value filter keeps rows whose value is chosen', () => {
  const row = { analytical_uuid: 'u', descriptive: ['Baca', null] };
  assert.ok(rowPassesFilterValues(row, 0, new Set()));
  assert.ok(rowPassesFilterValues(row, 0, new Set(['Baca', 'Other'])));
  assert.ok(!rowPassesFilterValues(row, 0, new Set(['Other'])));
  assert.ok(rowPassesFilterValues(row, 1, new Set(['(Missing)'])));
});

test('groupColors spreads distinct viridis anchors across few groups', async () => {
  const { groupColors, VIRIDIS } = await import('./visualize-model.ts');
  const five = groupColors(5);
  assert.equal(new Set(five).size, 5);
  assert.equal(five[0], VIRIDIS[0]);
  assert.equal(five[4], VIRIDIS[9]);
  assert.equal(groupColors(12).length, 12);
  assert.deepEqual(groupColors(0), []);
});
