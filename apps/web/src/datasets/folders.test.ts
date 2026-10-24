import assert from 'node:assert/strict';
import { test } from 'node:test';
import { activeFolderAmong, exclusionWithin, folderLabel, folderNameForColumn, folderOf, inFolder } from './folders.ts';

test('folders come from group paths on any platform', () => {
  assert.equal(folderOf('Site/Baca.parquet'), 'Site');
  assert.equal(folderOf(String.raw`Site\Baca.parquet`), 'Site');
  assert.equal(folderOf('a/b/c.parquet'), 'a/b');
  assert.equal(folderOf('top.parquet'), '');
  assert.equal(folderLabel(''), 'Project root');
  assert.equal(folderLabel('a/b'), 'b');
});

test('analyses use the chosen folder while it has groups, else the first', () => {
  const paths = ['Site/A.parquet', 'Clusters/K1.parquet', 'Site/B.parquet'];
  assert.equal(activeFolderAmong(paths, 'Site'), 'Site');
  assert.equal(activeFolderAmong(paths, 'Gone'), 'Clusters');
  assert.equal(activeFolderAmong(paths, null), 'Clusters');
  assert.equal(activeFolderAmong([], 'Site'), null);
  assert.deepEqual(inFolder(paths, 'Site'), ['Site/A.parquet', 'Site/B.parquet']);
});

test('exclusions change only inside the given folder', () => {
  const current = new Set(['Site/B.parquet', 'Clusters/K2.parquet']);
  assert.deepEqual(
    exclusionWithin(current, ['Site/A.parquet', 'Site/B.parquet'], ['Site/B.parquet']).sort(),
    ['Clusters/K2.parquet', 'Site/A.parquet'],
  );
});

test('import folders are named after the group column, made safe', () => {
  assert.equal(folderNameForColumn('Site'), 'Site');
  assert.equal(folderNameForColumn('Core/Zone: A'), 'Core_Zone_ A');
  assert.equal(folderNameForColumn('  .hidden  '), 'hidden');
  assert.equal(folderNameForColumn(''), 'groups');
  assert.equal(folderNameForColumn('sources'), 'sources_groups');
  assert.equal(folderNameForColumn('CON'), 'CON_groups');
});
