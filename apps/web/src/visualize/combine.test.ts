import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupRowsResponse } from '@archaeodash/client';
import { combineGroupFiles, selectionBySource } from './combine.ts';

function file(
  path: string,
  descriptive: string[],
  elemental: string[],
  rows: [string, (string | null)[], (number | null)[]][],
): GroupRowsResponse {
  return {
    path,
    revision_id: `rev-${path}`,
    visible_id_column: 'anid',
    legacy_rowid_column: 'rowid',
    descriptive_columns: descriptive,
    elemental_columns: elemental,
    rows: rows.map(([uuid, d, e], i) => ({
      analytical_uuid: uuid,
      legacy_rowid: String(i + 1),
      visible_id: uuid.toUpperCase(),
      descriptive: d,
      elemental: e,
    })),
  };
}

test('combining groups keeps shared elements, unions metadata, and tags each row', () => {
  const a = file('groups/A.parquet', ['Site'], ['as', 'fe', 'na'], [
    ['a1', ['Baca'], [1, 2, 3]],
  ]);
  const b = file('groups/B.parquet', ['Ware', 'Site'], ['fe', 'as'], [
    ['b1', ['Glaze', 'Hooper'], [20, 10]],
    ['b2', ['WMRW', null], [21, 11]],
  ]);
  const c = combineGroupFiles([
    { path: a.path, groupName: 'A', data: a },
    { path: b.path, groupName: 'B', data: b },
  ]);
  assert.deepEqual(c.elementalColumns, ['as', 'fe']);
  assert.deepEqual(c.descriptiveColumns, ['Site', 'Ware']);
  assert.deepEqual(c.groupOf, ['A', 'B', 'B']);
  assert.deepEqual(c.groupNames, ['A', 'B']);
  assert.deepEqual(c.rows.map((r) => r.elemental), [[1, 2], [10, 20], [11, 21]]);
  assert.deepEqual(c.rows.map((r) => r.descriptive), [['Baca', null], ['Hooper', 'Glaze'], [null, 'WMRW']]);

  const bySource = selectionBySource(c, new Set(['b2', 'a1']));
  assert.deepEqual([...bySource.entries()], [
    ['groups/A.parquet', ['a1']],
    ['groups/B.parquet', ['b2']],
  ]);
});

test('combining no files yields an empty dataset', () => {
  const c = combineGroupFiles([]);
  assert.equal(c.rows.length, 0);
  assert.deepEqual(c.elementalColumns, []);
});
