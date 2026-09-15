import assert from 'node:assert/strict';
import { test } from 'node:test';
import { TransportError } from './transport.ts';

test('transport error carries the problem envelope', () => {
  const err = new TransportError({ code: 'http_503', message: 'outage' });
  assert.equal(err.envelope.code, 'http_503');
  assert.equal(err.message, 'outage');
});
