import assert from 'node:assert/strict';
import { test } from 'node:test';
import { RERUN_DELAY_MS, supersedingRuns, waitUnlessAborted } from './rerun.ts';

test('rerun delay is short enough to feel immediate but covers a burst of clicks', () => {
  assert.ok(RERUN_DELAY_MS >= 250 && RERUN_DELAY_MS <= 600);
});

test('waitUnlessAborted resolves true after the delay', async () => {
  assert.equal(await waitUnlessAborted(5, new AbortController().signal), true);
});

test('waitUnlessAborted resolves false at once when aborted mid-wait or beforehand', async () => {
  const controller = new AbortController();
  const started = Date.now();
  const waiting = waitUnlessAborted(10_000, controller.signal);
  controller.abort();
  assert.equal(await waiting, false);
  assert.ok(Date.now() - started < 1_000);
  assert.equal(await waitUnlessAborted(10_000, controller.signal), false);
});

test('supersedingRuns aborts the previous run when the next starts, and on stop', () => {
  const runs = supersedingRuns();
  const first = runs.next();
  assert.equal(first.aborted, false);
  const second = runs.next();
  assert.equal(first.aborted, true);
  assert.equal(second.aborted, false);
  runs.stop();
  assert.equal(second.aborted, true);
  runs.stop(); // idempotent
  assert.equal(runs.next().aborted, false);
});

test('a burst of input changes starts one backend run', async () => {
  // Models the page loop: each change supersedes the last and waits first.
  const runs = supersedingRuns();
  const started: number[] = [];
  const change = async (n: number) => {
    const signal = runs.next();
    if (await waitUnlessAborted(20, signal)) started.push(n);
  };
  await Promise.all([change(1), change(2), change(3)]);
  assert.deepEqual(started, [3]);
});
