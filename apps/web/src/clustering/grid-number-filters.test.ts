import assert from 'node:assert/strict';
import { test } from 'node:test';
import { cellMatches, isNumericColumn, numberFilterText, parseNumberFilter, sortRows } from './grid-model.ts';

test('number filters: comparisons, inclusive ranges, and their text form', () => {
  assert.deepEqual(parseNumberFilter('>= 50'), { op: '>=', a: 50 });
  assert.deepEqual(parseNumberFilter('≤ 2.5'), { op: '<=', a: 2.5 });
  assert.deepEqual(parseNumberFilter('80..10'), { op: 'between', a: 10, b: 80 });
  assert.equal(parseNumberFilter('north'), null);
  assert.equal(cellMatches(10, '10..80'), true);
  assert.equal(cellMatches(80, '10..80'), true);
  assert.equal(cellMatches(80.01, '10..80'), false);
  assert.equal(cellMatches(null, '10..80'), false);
  assert.equal(cellMatches(-3, '< -1'), true);
  assert.equal(numberFilterText('>=', '50'), '>= 50');
  assert.equal(numberFilterText('between', '10', '80'), '10..80');
  assert.equal(numberFilterText('between', '10', ''), '', 'incomplete range filters nothing');
  assert.equal(numberFilterText('', '10'), '');
  assert.equal(numberFilterText('>', 'abc'), '');
});

test('number columns: fixed decimals or only numbers present', () => {
  const rows = [['A', 1, null], ['B', 2, 'x'], ['C', null, null]];
  assert.equal(isNumericColumn(rows, 0), false);
  assert.equal(isNumericColumn(rows, 1), true);
  assert.equal(isNumericColumn(rows, 2), false);
  assert.equal(isNumericColumn([[null], [null]], 0), false);
  assert.equal(isNumericColumn([[null]], 0, { label: 'p', digits: 2 }), true);
});

test('two sort keys: ties on the first are ordered by the second', () => {
  // Membership-style rows: group, probability.
  const rows = [['B', 40], ['A', 90], ['B', 95], ['A', 10], ['A', null]];
  assert.deepEqual(
    sortRows(rows, [0, 1, 2, 3, 4], [{ column: 0, direction: 'asc' }, { column: 1, direction: 'desc' }]),
    [1, 3, 4, 2, 0],
  );
  assert.deepEqual(sortRows(rows, [0, 1, 2], []), [0, 1, 2]);
});
