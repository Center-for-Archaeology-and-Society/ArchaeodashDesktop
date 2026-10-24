import assert from 'node:assert/strict';
import { test } from 'node:test';
import { cellMatches, compareCells, filterRows, formatCell, isVisible, loadView, nextSort, saveView, sortRows, viewKey } from './grid-model.ts';

const rows = [['b', 3, null], ['A', 10, 'x'], ['c', null, 'y'], ['a2', 2.5, 'x']] as const;

test('filters match displayed text case-insensitively and compare numbers', () => {
  assert.equal(cellMatches('North', 'nor'), true);
  assert.equal(cellMatches(null, 'unav'), true);
  assert.equal(cellMatches(81.234, '81.23', 2), true);
  assert.equal(cellMatches(5, '>= 5'), true);
  assert.equal(cellMatches(5, '<5'), false);
  assert.equal(cellMatches('5', '>1'), false);
  assert.deepEqual(filterRows(rows, ['a', '', '']), [1, 3]);
  assert.deepEqual(filterRows(rows, ['', '>2.5', 'x']), [1]);
  assert.deepEqual(filterRows(rows, ['', '', '']), [0, 1, 2, 3]);
});

test('sorting is natural, stable and keeps missing values last', () => {
  assert.ok(compareCells('a2', 'a10') < 0);
  assert.deepEqual(sortRows(rows, [0, 1, 2, 3], { column: 1, direction: 'asc' }), [3, 0, 1, 2]);
  assert.deepEqual(sortRows(rows, [0, 1, 2, 3], { column: 1, direction: 'desc' }), [1, 0, 3, 2]);
  assert.deepEqual(sortRows(rows, [0, 1, 2, 3], { column: 0, direction: 'asc' }), [1, 3, 0, 2]);
  assert.deepEqual(sortRows(rows, [2, 0], null), [2, 0]);
  assert.deepEqual(nextSort(null, 1), { column: 1, direction: 'asc' });
  assert.deepEqual(nextSort({ column: 1, direction: 'asc' }, 1), { column: 1, direction: 'desc' });
  assert.equal(nextSort({ column: 1, direction: 'desc' }, 1), null);
  assert.deepEqual(nextSort({ column: 1, direction: 'desc' }, 2), { column: 2, direction: 'asc' });
});

test('cells format with fixed digits and missing values read Unavailable', () => {
  assert.equal(formatCell(1.23456, 2), '1.23');
  assert.equal(formatCell(null), 'Unavailable');
  assert.equal(formatCell('x', 2), 'x');
});

test('views persist per table and tolerate broken or missing storage', () => {
  const store = new Map<string, string>();
  const fake = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => { store.set(k, v); } };
  assert.deepEqual(loadView('membership', fake), { visibility: {}, compact: true });
  saveView('membership', { visibility: { 'Best value': true, Group: false }, compact: false }, fake);
  assert.deepEqual(loadView('membership', fake), { visibility: { 'Best value': true, Group: false }, compact: false });
  assert.deepEqual(loadView('euclidean', fake), { visibility: {}, compact: true });
  store.set(viewKey('broken'), '{not json');
  assert.deepEqual(loadView('broken', fake), { visibility: {}, compact: true });
  const throwing = { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } };
  assert.deepEqual(loadView('membership', throwing), { visibility: {}, compact: true });
  assert.doesNotThrow(() => saveView('membership', { visibility: {}, compact: true }, throwing));
  assert.equal(isVisible({ visibility: {}, compact: true }, { label: 'Best value', hiddenByDefault: true }), false);
  assert.equal(isVisible({ visibility: { 'Best value': true }, compact: true }, { label: 'Best value', hiddenByDefault: true }), true);
});
