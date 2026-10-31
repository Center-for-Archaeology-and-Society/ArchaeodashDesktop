import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupRowsResponse } from '@archaeodash/client';
import { combineGroupFiles, defaultGroupColumn, inputPaths, selectionBySource, syntheticGroupColumn, zeroFillNotice } from './combine.ts';

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

test('combining groups unions elements (absent ones 0) and metadata, and tags each row', () => {
  const a = file('groups/A.parquet', ['Site'], ['as', 'fe', 'na'], [
    ['a1', ['Baca'], [1, 2, 3]],
  ]);
  const b = file('groups/B.parquet', ['Ware', 'Site'], ['fe', 'as'], [
    ['b1', ['Glaze', 'Hooper'], [20, 10]],
    ['b2', ['WMRW', null], [null, 11]],
  ]);
  const c = combineGroupFiles([
    { path: a.path, groupName: 'A', data: a },
    { path: b.path, groupName: 'B', data: b },
  ]);
  assert.deepEqual(c.elementalColumns, ['as', 'fe', 'na']);
  assert.deepEqual(c.descriptiveColumns, ['Site', 'Ware', 'Group']);
  assert.equal(c.groupColumn, 'Group');
  assert.deepEqual(c.groupOf, ['A', 'B', 'B']);
  assert.deepEqual(c.groupNames, ['A', 'B']);
  // B has no `na`: 0 there. B's own missing `fe` stays missing.
  assert.deepEqual(c.rows.map((r) => r.elemental), [[1, 2, 3], [10, 20, 0], [11, null, 0]]);
  assert.deepEqual(c.zeroFilled, [{ path: 'groups/B.parquet', groupName: 'B', columns: ['na'] }]);
  assert.equal(zeroFillNotice(c), 'Some groups lack measured columns, which are 0 for their rows when groups are combined: B (na).');
  const twice = combineGroupFiles([
    { path: a.path, groupName: 'A', data: a },
    { path: b.path, groupName: 'B', data: b },
    { path: 'groups/C.parquet', groupName: 'C', data: { ...b, path: 'groups/C.parquet' } },
  ]);
  assert.equal(zeroFillNotice(twice), 'Some groups lack measured columns, which are 0 for their rows when groups are combined: B, C (na).');
  assert.deepEqual(c.rows.map((r) => r.descriptive), [
    ['Baca', null, 'A'],
    ['Hooper', 'Glaze', 'B'],
    [null, 'WMRW', 'B'],
  ]);

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
  assert.equal(zeroFillNotice(c), '');
});

test('synthetic group column avoids a real "Group" column', () => {
  assert.equal(syntheticGroupColumn(['Site', 'Ware']), 'Group');
  assert.equal(syntheticGroupColumn(['Site', 'Group']), 'Group (file)');
  // Case-sensitive, matching the backend rule.
  assert.equal(syntheticGroupColumn(['group']), 'Group');
});

test('input paths put the first file in path and the rest in extra_paths', () => {
  const a = file('groups/A.parquet', [], ['x'], []);
  const b = file('groups/B.parquet', [], ['x'], []);
  assert.deepEqual(
    inputPaths([
      { path: a.path, groupName: 'A', data: a },
      { path: b.path, groupName: 'B', data: b },
    ]),
    { path: 'groups/A.parquet', extra_paths: ['groups/B.parquet'] },
  );
});

test('default grouping is the group column only when several groups are combined', () => {
  const a = file('groups/A.parquet', ['Site'], ['x'], []);
  const b = file('groups/B.parquet', ['Site'], ['x'], []);
  const one = combineGroupFiles([{ path: a.path, groupName: 'A', data: a }]);
  const two = combineGroupFiles([
    { path: a.path, groupName: 'A', data: a },
    { path: b.path, groupName: 'B', data: b },
  ]);
  assert.equal(defaultGroupColumn(one), 'Site');
  assert.equal(defaultGroupColumn(two), 'Group');
});
