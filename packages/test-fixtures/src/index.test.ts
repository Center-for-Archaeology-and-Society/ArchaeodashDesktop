import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fixtureAppInfo } from './index.ts';

test('fixture app info is ready', () => {
  assert.equal(fixtureAppInfo.ready, true);
});
