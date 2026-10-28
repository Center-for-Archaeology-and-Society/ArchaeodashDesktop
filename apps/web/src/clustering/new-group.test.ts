import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupCandidate } from '@archaeodash/client';
import { newGroupPath, newGroupProblem, safePath } from './new-group.ts';

const candidates: GroupCandidate[] = [
  { path: 'groups/North.parquet', ready: true, group: { path: 'groups/North.parquet', group_id: 'n', group_name: 'North', revision_id: 'r', row_count: 1, elemental_columns: [], descriptive_columns: [] } },
  { path: 'groups/reserved.parquet', ready: false },
];

test('new group files sit beside the analyzed group with a sanitized name', () => {
  assert.equal(newGroupPath('groups/source.parquet', 'Group 7/b'), 'groups/Group_7_b.parquet');
  assert.equal(newGroupPath('source.parquet', ' ', 3), 'group_3.parquet');
  assert.equal(safePath('groups/a.parquet'), true);
  assert.equal(safePath('../a.parquet'), false);
});

test('Windows group paths with backslashes are safe; escapes and absolute paths are not', () => {
  assert.equal(safePath('group2\\A.parquet'), true);
  assert.equal(safePath('group2\\sub/A.parquet'), true);
  for (const bad of ['group2\\..\\A.parquet', '\\A.parquet', 'C:\\A.parquet', 'group2\\\\A.parquet', '..\\A.parquet']) {
    assert.equal(safePath(bad), false, bad);
  }
});

test('new groups refuse blank names, unsafe paths and collisions with existing groups or files', () => {
  assert.equal(newGroupProblem('Fresh', 'groups/Fresh.parquet', candidates), null);
  assert.match(newGroupProblem(' ', 'groups/x.parquet', candidates)!, /Enter a name/);
  assert.match(newGroupProblem('North: B', 'groups/North__B.parquet', candidates)!, /cannot contain :/);
  assert.match(newGroupProblem('CON', 'groups/CON.parquet', candidates)!, /Windows reserves/);
  assert.match(newGroupProblem('Fresh', '/abs.parquet', candidates)!, /safe/);
  assert.match(newGroupProblem(' north ', 'groups/other.parquet', candidates)!, /group named “North” already exists/);
  assert.match(newGroupProblem('Other', 'groups/NORTH.parquet', candidates)!, /already exists/);
  assert.match(newGroupProblem('Other', 'groups/reserved.parquet', candidates)!, /unused by every group candidate/);
});
