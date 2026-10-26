import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupRowsResponse } from '@archaeodash/client';
import { combineGroupFiles, type LoadedGroupFile } from '../visualize/combine.ts';
import { realignByUuid, rowContentKey, rowGroupKey } from './rowKeys.ts';

type Row = [uuid: string, values: (number | null)[]];

function file(path: string, groupName: string, rows: Row[], columns = ['Ti', 'Sr']): LoadedGroupFile {
  const data: GroupRowsResponse = {
    path,
    revision_id: `${path}-rev`,
    visible_id_column: 'anid',
    legacy_rowid_column: 'rowid',
    descriptive_columns: ['Site'],
    elemental_columns: columns,
    rows: rows.map(([uuid, elemental]) => ({
      analytical_uuid: uuid,
      legacy_rowid: null,
      visible_id: uuid,
      descriptive: [groupName],
      elemental,
    })),
  };
  return { path, groupName, data };
}

const a1: Row = ['u1', [1, 2]];
const a2: Row = ['u2', [3, null]];
const b1: Row = ['u3', [5, 6]];
const before = combineGroupFiles([file('g/A', 'A', [a1, a2]), file('g/B', 'B', [b1])]);

test('moving samples between groups keeps the content key but not the group key', () => {
  const moved = combineGroupFiles([file('g/A', 'A', [a1]), file('g/B', 'B', [b1, a2])]);
  assert.equal(rowContentKey(moved), rowContentKey(before));
  assert.notEqual(rowGroupKey(moved), rowGroupKey(before));
  // A new group file holding moved samples is the same content too.
  const split = combineGroupFiles([file('g/A', 'A', [a1]), file('g/B', 'B', [b1]), file('g/C', 'C', [a2])]);
  assert.equal(rowContentKey(split), rowContentKey(before));
  // Group order does not matter either.
  const reversed = combineGroupFiles([file('g/B', 'B', [b1]), file('g/A', 'A', [a1, a2])]);
  assert.equal(rowContentKey(reversed), rowContentKey(before));
  assert.equal(rowGroupKey(reversed), rowGroupKey(before));
});

test('different samples, values, missing values, or columns change the content key', () => {
  const base = rowContentKey(before);
  const fewer = combineGroupFiles([file('g/A', 'A', [a1, a2])]);
  const changed = combineGroupFiles([file('g/A', 'A', [a1, ['u2', [3, 4]]]), file('g/B', 'B', [b1])]);
  const missing = combineGroupFiles([file('g/A', 'A', [['u1', [null, 2]], a2]), file('g/B', 'B', [b1])]);
  const swapped = combineGroupFiles([file('g/A', 'A', [['u1', [2, 1]], a2]), file('g/B', 'B', [b1])]);
  const renamed = combineGroupFiles([file('g/A', 'A', [a1, a2], ['Ti', 'Zr']), file('g/B', 'B', [b1], ['Ti', 'Zr'])]);
  const keys = [base, ...[fewer, changed, missing, swapped, renamed].map(rowContentKey)];
  assert.equal(new Set(keys).size, keys.length);
});

test('realignByUuid follows samples to their new rows', () => {
  assert.deepEqual(realignByUuid(['u1', 'u2', 'u3'], ['x', 'y', 'z'], ['u3', 'u1', 'u4', 'u2']), ['z', 'x', undefined, 'y']);
  // Copies of one sample match in order.
  assert.deepEqual(realignByUuid(['u1', 'u1'], [1, 2], ['u1', 'u1', 'u1']), [1, 2, undefined]);
});
