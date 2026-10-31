import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  autoIdChoice, idChoiceLabel, idNotice, resolveIdChoice, rowidLabels, sampleIds, summarizeIds, type IdFile,
} from './sample-ids.ts';

const file = (column: string, ids: (string | null)[]): IdFile => ({ data: { visible_id_column: column, rows: ids.map(visible_id => ({ visible_id })) } });

test('unique rowids stay plain; repeats name their group and occurrence', () => {
  assert.deepEqual(rowidLabels([{ legacy_rowid: '1' }, { legacy_rowid: '2' }, { legacy_rowid: null }], ['A', 'A', 'B']), ['1', '2', '3']);
  assert.deepEqual(
    rowidLabels([{ legacy_rowid: '1' }, { legacy_rowid: '1' }, { legacy_rowid: '2' }, { legacy_rowid: '1' }], ['A', 'B', 'B', 'B']),
    ['A:1', 'B:1', 'B:2', 'B:1 (2)'],
  );
});

test('automatic IDs use rowid when the groups use different or no ID columns', () => {
  const same = summarizeIds([file('anid', ['a', 'b']), file('anid', ['c', null])]);
  assert.deepEqual(same.columns, ['anid']);
  assert.equal(same.missing, 1);
  assert.equal(autoIdChoice(same), 'visible', 'a few missing IDs keep the sample ID');
  const differ = summarizeIds([file('anid', ['a']), file('rowid', ['1'])]);
  assert.equal(autoIdChoice(differ), 'rowid');
  assert.equal(idChoiceLabel(differ, 'visible'), 'Sample ID');
  assert.equal(idChoiceLabel(same, 'visible'), 'Sample ID (anid)');
  const empty = summarizeIds([file('anid', ['a']), file('anid', [null, null])]);
  assert.equal(empty.emptyGroups, 1);
  assert.equal(autoIdChoice(empty), 'rowid');
  assert.equal(resolveIdChoice('visible', empty), 'visible', 'a fixed choice wins');
  assert.equal(resolveIdChoice('rowid', same), 'rowid');
});

test('sample IDs follow the choice and fall back to rowid labels', () => {
  const files = [file('anid', ['AN1', null]), file('Sample', ['S1'])];
  const summary = summarizeIds(files);
  const combined = {
    rows: [{ visible_id: 'AN1', legacy_rowid: '1' }, { visible_id: null, legacy_rowid: '2' }, { visible_id: 'S1', legacy_rowid: '1' }],
    groupOf: ['North', 'North', 'South'],
  };
  const visible = sampleIds(combined, summary, 'visible');
  assert.equal(visible.column, 'Sample ID');
  assert.deepEqual(visible.ids, ['AN1', 'North:2', 'S1']);
  const rowid = sampleIds(combined, summary, 'rowid');
  assert.equal(rowid.column, 'rowid');
  assert.deepEqual(rowid.ids, ['North:1', 'North:2', 'South:1']);
  assert.equal(rowid.qualified, true);
  assert.match(idNotice('auto', summary, rowid), /different ID columns \(anid, Sample\), so rowid is used\. Rowids repeat/);
  assert.match(idNotice('visible', summary, visible), /rowid may be clearer/);
  assert.equal(idNotice('rowid', summary, rowid), 'Rowids repeat across these groups, so they show as group:rowid.');
  const one = summarizeIds([file('anid', ['a', null])]);
  assert.equal(idNotice('auto', one, sampleIds({ rows: [{ visible_id: 'a', legacy_rowid: '1' }, { visible_id: null, legacy_rowid: '2' }], groupOf: ['A', 'A'] }, one, 'visible')),
    '1 sample has no sample ID and shows its rowid.');
  const clean = summarizeIds([file('anid', ['a'])]);
  assert.equal(idNotice('auto', clean, sampleIds({ rows: [{ visible_id: 'a', legacy_rowid: '1' }], groupOf: ['A'] }, clean, 'visible')), '');
});
