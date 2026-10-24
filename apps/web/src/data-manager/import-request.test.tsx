import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ExistingImportGroup } from '@archaeodash/contracts';
import { AUTOMATIC_ID_COLUMN, commitMode, commitRequestFor, descriptiveColumnChoices, freeFolderName, idColumnChoices, importConflicts, keptDescriptive, measuredColumnChoices, visibleIdForCommit } from './import-request.ts';
import { DescriptiveColumns, ImportDestination } from './ImportDestination.tsx';

test('automatic IDs are sent explicitly as the row-number column', () => {
  assert.equal(visibleIdForCommit(''), AUTOMATIC_ID_COLUMN);
  assert.equal(visibleIdForCommit('anid'), 'anid');
});

test('rowid is never offered as an ID or measured column', () => {
  const columns = ['rowid', 'anid', 'Site', 'fe', 'na'];
  assert.deepEqual(idColumnChoices(columns), ['anid', 'Site', 'fe', 'na']);
  assert.deepEqual(measuredColumnChoices(columns, 'anid', 'Site'), ['fe', 'na']);
  assert.deepEqual(measuredColumnChoices(columns, '', ''), ['anid', 'Site', 'fe', 'na']);
});

const existing = (path: string, group: string, same: boolean): ExistingImportGroup => ({
  path, folder: path.slice(0, path.lastIndexOf('/')), group_name: group, revision_id: 'rev-1', same_source: same,
});
const partitions = [
  { group_name: 'Baca', row_count: 2, suggested_path: 'groups/Baca.parquet' },
  { group_name: 'Zuni', row_count: 1, suggested_path: 'groups/Zuni.parquet' },
];

test('descriptive choices exclude rowid, the ID, and measured columns', () => {
  const choices = descriptiveColumnChoices(['rowid', 'anid', 'Site', 'Ware', 'fe'], 'anid', ['fe']);
  assert.deepEqual(choices, ['Site', 'Ware']);
  assert.deepEqual(keptDescriptive(choices, ['Ware']), ['Site']);
});

test('conflicts report clashes, the same source, and append targets per folder', () => {
  const preview = {
    partitions,
    existing_groups: [existing('Site/Baca.parquet', 'Baca', true), existing('Other/Zuni.parquet', 'Zuni', false), existing('Old/Baca.parquet', 'Baca', true)],
  };
  const site = importConflicts(preview, 'Site');
  assert.deepEqual(site.clashes, ['Baca.parquet']);
  assert.equal(site.sameSource.length, 1);
  assert.deepEqual(site.sameSourceElsewhere, ['Old']);
  assert.equal(commitMode(site, ''), null, 'a choice is required');
  assert.equal(commitMode(site, 'new'), null, 'new groups would be refused');
  assert.equal(commitMode(site, 'replace'), 'replace_source');
  assert.equal(commitMode(site, 'append'), null, 'the same file is never appended twice');

  const other = importConflicts(preview, 'Other');
  assert.deepEqual(other.matchingNames, ['Zuni']);
  assert.equal(commitMode(other, 'append'), 'append');
  assert.equal(commitMode(other, 'replace'), null);
  assert.equal(commitMode(other, 'new'), null, 'Zuni.parquet clashes');

  const empty = importConflicts(preview, 'Fresh');
  assert.equal(commitMode(empty, ''), 'new_groups');
  assert.equal(freeFolderName('Site', ['Site', 'site 2', 'Other']), 'Site 3');
  assert.equal(freeFolderName('Fresh', ['Site']), 'Fresh');
});

test('the commit request carries the column choice, the ID, and the mode', () => {
  const request = commitRequestFor({
    source: 'sources/a.csv', sheet: '', groupMode: 'column', groupColumn: 'Site', groupName: '', idColumn: '',
    measured: ['fe'], descriptive: ['Site'], recipe: { zero_as_na: true, negative_as_na: true, na_as_zero: false, blank_non_element_label: null },
    folder: 'Site', mode: 'append',
  });
  assert.equal(request.visible_id_column, 'rowid');
  assert.deepEqual(request.descriptive_columns, ['Site']);
  assert.equal(request.mode, 'append');
  assert.equal(request.group_name, null);
  assert.equal('sheet' in request, false);
});

test('destination prompt requires a choice and offers a new folder on clashes', () => {
  const conflicts = importConflicts({ partitions, existing_groups: [existing('Site/Baca.parquet', 'Baca', true)] }, 'Site');
  const html = renderToStaticMarkup(<ImportDestination folder="Site" conflicts={conflicts} choice="" freeFolder="Site 2" onFolderChange={() => {}} onChoice={() => {}} />);
  assert.match(html, /already has 1 group/);
  assert.match(html, /Replace the 1 group imported from this file/);
  assert.match(html, /Choose one to continue/);
  const blocked = renderToStaticMarkup(<ImportDestination folder="Site" conflicts={conflicts} choice="new" freeFolder="Site 2" onFolderChange={() => {}} onChoice={() => {}} onCancel={() => {}} />);
  assert.match(blocked, /role="alert"/);
  assert.match(blocked, /Use a new folder “Site 2”/);
  assert.match(blocked, /Cancel import/);
  const free = renderToStaticMarkup(<ImportDestination folder="New" conflicts={importConflicts({ partitions, existing_groups: [] }, 'New')} choice="" freeFolder="New" onFolderChange={() => {}} onChoice={() => {}} />);
  assert.doesNotMatch(free, /What should this import do/);
});

test('descriptive checklist keeps every column by default', () => {
  const html = renderToStaticMarkup(<DescriptiveColumns choices={['Site', 'Ware']} dropped={['Ware']} onChange={() => {}} />);
  assert.match(html, /Keep as descriptive/);
  assert.equal((html.match(/checked=""/g) ?? []).length, 1);
});
