import assert from 'node:assert/strict';
import { test } from 'node:test';
import { AUTOMATIC_ID_COLUMN, idColumnChoices, measuredColumnChoices, visibleIdForCommit } from './import-request.ts';

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
