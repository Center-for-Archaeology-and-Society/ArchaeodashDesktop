import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { GroupCandidate } from '@archaeodash/client';
import { sharedScan } from './groupUpdates.ts';
import { groupUpdatesHeld, holdGroupUpdates, notifyGroupsChanged, onGroupUpdatesHeld, onGroupsChanged } from '../data-manager/events.ts';

test('callers in one change epoch share a scan; a later notice starts a new one', async () => {
  let scans = 0;
  let finish: (list: GroupCandidate[]) => void = () => {};
  const service = { scan: () => { scans += 1; return new Promise<GroupCandidate[]>((resolve) => { finish = resolve; }); } };
  const a = sharedScan(service);
  const b = sharedScan(service);
  assert.equal(a, b);
  assert.equal(scans, 1);
  await notifyGroupsChanged();
  const c = sharedScan(service);
  assert.notEqual(c, a, 'a scan from before the notice is not reused');
  assert.equal(scans, 2);
  finish([]);
  await c;
});

test('a hold reports to listeners and lasts until every release', () => {
  const seen: boolean[] = [];
  const off = onGroupUpdatesHeld((held) => seen.push(held));
  const first = holdGroupUpdates();
  const second = holdGroupUpdates();
  assert.equal(groupUpdatesHeld(), true);
  first();
  first();
  assert.equal(groupUpdatesHeld(), true, 'a repeated release counts once');
  second();
  assert.equal(groupUpdatesHeld(), false);
  off();
  assert.deepEqual(seen, [true, false]);
});

test('notifyGroupsChanged waits for the refreshes it starts', async () => {
  const target = new EventTarget();
  const previous = (globalThis as { window?: unknown }).window;
  (globalThis as { window?: unknown }).window = target;
  try {
    let done = false;
    const off = onGroupsChanged(() => new Promise<void>((resolve) => setTimeout(() => { done = true; resolve(); }, 20)));
    await notifyGroupsChanged();
    assert.equal(done, true);
    off();
  } finally {
    (globalThis as { window?: unknown }).window = previous;
  }
});
