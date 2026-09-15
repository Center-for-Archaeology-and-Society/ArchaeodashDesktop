import assert from 'node:assert/strict';
import { test } from 'node:test';
import { themes } from './index.ts';

test('three themes are defined', () => {
  assert.equal(themes.length, 3);
  assert.deepEqual(themes.map((t) => t.name), ['light', 'dark', 'high-contrast']);
});
