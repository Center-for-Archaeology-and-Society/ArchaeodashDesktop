/** Dataset table search and sort (legacy DT global search and column sort). */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { compareCells, nextSort, tableOrder, type DescriptiveText, type TableRow } from './tableView.ts';

const rows: TableRow[] = [
  { analytical_uuid: 'uuid-a', visible_id: 'S10', legacy_rowid: '10', descriptive: ['Baca', 'jar'], elemental: [12.5, null] },
  { analytical_uuid: 'uuid-b', visible_id: 'S2', legacy_rowid: '2', descriptive: ['Hooper', ''], elemental: [3, 40] },
  { analytical_uuid: 'uuid-c', visible_id: 'S1', legacy_rowid: '1', descriptive: ['baca', 'bowl'], elemental: [null, 7] },
  { analytical_uuid: 'uuid-d', visible_id: null, legacy_rowid: null, descriptive: ['Ridge', 'jar'], elemental: [100, 7] },
];
const text: DescriptiveText = (row, i) => row.descriptive[i] ?? null;

test('search keeps rows matching every word in some visible cell, case-insensitively', () => {
  assert.deepEqual(tableOrder(rows, 2, 'baca', null, text), [0, 2]);
  assert.deepEqual(tableOrder(rows, 2, 'BACA jar', null, text), [0]);
  assert.deepEqual(tableOrder(rows, 2, '  ', null, text), [0, 1, 2, 3]);
  // Elemental values match as displayed, NA included.
  assert.deepEqual(tableOrder(rows, 2, '12.5', null, text), [0]);
  assert.deepEqual(tableOrder(rows, 2, 'na', null, text), [0, 2]);
  // The hidden UUID is never searched.
  assert.deepEqual(tableOrder(rows, 2, 'uuid', null, text), []);
});

test('search reads the current (saved) descriptive text', () => {
  const edited: DescriptiveText = (row, i) => (row.analytical_uuid === 'uuid-b' && i === 1 ? 'pitcher' : text(row, i));
  assert.deepEqual(tableOrder(rows, 2, 'pitcher', null, edited), [1]);
});

test('sort is numeric-aware with missing values last in both directions', () => {
  const byId = (dir: 'asc' | 'desc') => tableOrder(rows, 2, '', { key: { kind: 'visible_id' }, dir }, text);
  // Natural order: S1 < S2 < S10; the row without an ID stays last.
  assert.deepEqual(byId('asc'), [2, 1, 0, 3]);
  assert.deepEqual(byId('desc'), [0, 1, 2, 3]);
  const byRowid = tableOrder(rows, 2, '', { key: { kind: 'legacy_rowid' }, dir: 'asc' }, text);
  assert.deepEqual(byRowid, [2, 1, 0, 3]);
  const byElement = (dir: 'asc' | 'desc') => tableOrder(rows, 2, '', { key: { kind: 'elemental', index: 0 }, dir }, text);
  assert.deepEqual(byElement('asc'), [1, 0, 3, 2]);
  assert.deepEqual(byElement('desc'), [3, 0, 1, 2]);
  // Text ignores case; blanks count as missing; ties keep file order.
  const byWare = tableOrder(rows, 2, '', { key: { kind: 'descriptive', index: 1 }, dir: 'asc' }, text);
  assert.deepEqual(byWare, [2, 0, 3, 1]);
  const bySite = tableOrder(rows, 2, '', { key: { kind: 'descriptive', index: 0 }, dir: 'asc' }, text);
  assert.deepEqual(bySite, [0, 2, 1, 3]);
  // Search and sort combine.
  assert.deepEqual(tableOrder(rows, 2, 'jar', { key: { kind: 'elemental', index: 0 }, dir: 'desc' }, text), [3, 0]);
});

test('header clicks cycle ascending, descending, unsorted', () => {
  const key = { kind: 'descriptive', index: 1 } as const;
  const asc = nextSort(null, key);
  assert.deepEqual(asc, { key, dir: 'asc' });
  const desc = nextSort(asc, key);
  assert.deepEqual(desc, { key, dir: 'desc' });
  assert.equal(nextSort(desc, key), null);
  // Another column starts ascending.
  assert.deepEqual(nextSort(desc, { kind: 'descriptive', index: 0 }), { key: { kind: 'descriptive', index: 0 }, dir: 'asc' });
  assert.deepEqual(nextSort(desc, { kind: 'visible_id' }), { key: { kind: 'visible_id' }, dir: 'asc' });
});

test('cell comparison treats numeric text as numbers', () => {
  assert.ok(compareCells('9', '10') < 0);
  assert.ok(compareCells(2, 10) < 0);
  assert.ok(compareCells('b', 'A') > 0);
  assert.ok(compareCells('S9', 'S10') < 0);
});
