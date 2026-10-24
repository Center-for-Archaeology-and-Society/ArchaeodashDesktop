/** Pure ordination computations: cos², variable coordinates/contributions,
 *  the min(4, K) contribution table, LDA dimension handling, request gate. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  componentContrib,
  contributionRule,
  contributionTable,
  gradientColor,
  indicesByGroup,
  individualCos2,
  ldaDimensions,
  ldaLayout,
  ldaVectors,
  requestGate,
  variableContrib,
  variableCoords,
} from './ordination-model.ts';

const close = (actual: number, expected: number, eps = 1e-9) =>
  assert.ok(Math.abs(actual - expected) < eps, `${actual} ≈ ${expected}`);

// Orthonormal 3x3 rotation with sdev 2, 1, 0.5 (eigenvalues 4, 1, 0.25).
const rotation = [
  [0.8, 0.6, 0],
  [0.6, -0.8, 0],
  [0, 0, 1],
];
const sdev = [2, 1, 0.5];

test('individual cos² is the displayed pair over every component', () => {
  const cos2 = individualCos2([[3, 4, 0], [0, 0, 5], [0, 0, 0], [1, 1, 1, 1]]);
  assert.deepEqual(cos2, [1, 0, 0, 0.5]);
  const other = individualCos2([[1, 2, 2]], 1, 2);
  close(other[0] ?? NaN, 8 / 9);
});

test('variable coordinates are loading × component sdev', () => {
  assert.deepEqual(variableCoords(rotation, sdev, 0), [1.6, 1.2, 0]);
  const y = variableCoords(rotation, sdev, 1);
  close(y[0] ?? NaN, 0.6);
  close(y[1] ?? NaN, -0.8);
});

test('per-component contribution is 100·cos²/Σcos² and sums to 100', () => {
  const c = componentContrib(rotation, 0);
  close(c[0] ?? NaN, 64);
  close(c[1] ?? NaN, 36);
  close(c[2] ?? NaN, 0);
  assert.deepEqual(componentContrib([[0], [0]], 0), [0, 0], 'zero component gives zeros');
});

test('combined axis 1–2 contribution is eigenvalue weighted (factoextra)', () => {
  const c = variableContrib(rotation, sdev, 0, 1);
  close(c[0] ?? NaN, (64 * 4 + 36 * 1) / 5);
  close(c[1] ?? NaN, (36 * 4 + 64 * 1) / 5);
  close(c[2] ?? NaN, 0);
});

test('contribution table sums the first four components only, sorted descending', () => {
  const wide = [
    [1, 1, 1, 1, 0],
    [0, 0, 0, 0, 1],
  ];
  const table = contributionTable(wide, ['Fe', 'Na']);
  assert.equal(table.components, 4);
  assert.deepEqual(table.rows, [['Fe', 400], ['Na', 0]]);
  assert.match(contributionRule(4, ['PC1', 'PC2', 'PC3', 'PC4', 'PC5']), /PC1–PC4.*min\(4, number of components\) = 4/);
});

test('contribution table with fewer than four components labels the shortfall', () => {
  const table = contributionTable([[0.6], [0.8]], ['Ti', 'Sr']);
  assert.equal(table.components, 1);
  assert.equal(table.rows[0]?.[0], 'Sr');
  close(table.rows[0]?.[1] ?? NaN, 64);
  close(table.rows[1]?.[1] ?? NaN, 36);
  const rule = contributionRule(1, ['PC1']);
  assert.match(rule, /to PC1:/);
  assert.match(rule, /only 1 component available/);
});

test('gradient colours match the legacy factoextra stops', () => {
  assert.equal(gradientColor(0), '#00afbb');
  assert.equal(gradientColor(0.5), '#e7b800');
  assert.equal(gradientColor(1), '#fc4e07');
  assert.equal(gradientColor(Number.NaN), '#00afbb');
});

test('LDA layout validates dimensions: two, one, or none', () => {
  const two = { score_names: ['LD1', 'LD2'], scores: [[1, 2]], scaling: [[1, 2], [3, 4]] };
  assert.deepEqual(ldaLayout(two), { kind: 'two', names: ['LD1', 'LD2'] });
  // A response naming LD2 but carrying one scaling column never indexes LD2.
  const one = { score_names: ['LD1', 'LD2'], scores: [[1, 2]], scaling: [[1]] };
  assert.equal(ldaDimensions(one), 1);
  assert.deepEqual(ldaLayout(one), { kind: 'one', name: 'LD1' });
  assert.deepEqual(ldaLayout({ score_names: [], scores: [], scaling: [] }), { kind: 'none' });
});

test('LDA vectors end at each variable scaling on LD1/LD2', () => {
  assert.deepEqual(ldaVectors([[1, 2], [3, 4]], ['Ti']), [
    { variable: 'Ti', x: 1, y: 2 },
    { variable: 'Column 2', x: 3, y: 4 },
  ]);
});

test('rows group by first appearance; mismatched groups collapse to one', () => {
  assert.deepEqual(indicesByGroup(3, ['a', 'b', 'a']), [['a', [0, 2]], ['b', [1]]]);
  assert.deepEqual(indicesByGroup(2, ['a']), [['All rows', [0, 1]]]);
});

test('request gate keeps only the latest request current', () => {
  const gate = requestGate();
  const first = gate.begin();
  const second = gate.begin();
  assert.equal(first(), false, 'superseded request is stale');
  assert.equal(second(), true);
  gate.cancel();
  assert.equal(second(), false, 'cancel (unmount/input change) invalidates');
});
