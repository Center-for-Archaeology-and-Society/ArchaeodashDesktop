import assert from 'node:assert/strict';
import { test } from 'node:test';
import { missingBand, missingProfile } from './missing.ts';

const row = (elemental: (number | null)[]) => ({ analytical_uuid: 'u', descriptive: [], elemental });

test('missing profile counts NA per column, orders by count, keeps ties in column order', () => {
  const combined = {
    elementalColumns: ['Ti', 'Sr', 'Fe', 'Zr'],
    rows: [row([1, null, null, 4]), row([null, null, 3, 5]), row([2, 1, null, 6]), row([3, null, 1, 7])],
  };
  const profile = missingProfile(combined, ['Ti', 'Sr', 'Fe', 'Zr', 'Nope']);
  assert.deepEqual(profile.map((r) => [r.feature, r.num_missing]), [['Sr', 3], ['Fe', 2], ['Ti', 1], ['Zr', 0]]);
  assert.equal(profile[0]!.pct_missing, 0.75);
  assert.deepEqual(profile.map((r) => r.band), ['Bad', 'Bad', 'OK', 'Good']);
});

test('missing bands use the legacy cut points', () => {
  assert.equal(missingBand(0.05), 'Good');
  assert.equal(missingBand(0.0501), 'OK');
  assert.equal(missingBand(0.4), 'OK');
  assert.equal(missingBand(0.8), 'Bad');
  assert.equal(missingBand(0.81), 'Remove');
});

test('missing profile of no rows marks every column Remove', () => {
  const profile = missingProfile({ elementalColumns: ['Ti'], rows: [] }, ['Ti']);
  assert.equal(profile[0]!.band, 'Remove');
  assert.ok(Number.isNaN(profile[0]!.pct_missing));
});
