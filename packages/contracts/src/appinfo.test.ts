import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { AppInfo } from './index.ts';

test('AppInfo shape matches the Rust DTO field order and types', () => {
  const info: AppInfo = { app: 'archaeodash', version: '0.1.0', transport: 'http', ready: true };
  assert.equal(info.app, 'archaeodash');
  assert.ok(info.ready);
});
