import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupRowsResponse } from '@archaeodash/client';
import { DataCache, PRELOAD_ROW_LIMIT } from './DataCache.tsx';

const rows = (path: string, revision: string): GroupRowsResponse => ({
  path,
  revision_id: revision,
  visible_id_column: 'anid',
  legacy_rowid_column: 'rowid',
  descriptive_columns: [],
  elemental_columns: [],
  rows: [],
});

test('rows are reused for the same revision and refetched for a new one', async () => {
  const cache = new DataCache();
  let fetches = 0;
  const fetch = (rev: string) => async () => { fetches += 1; return rows('g/A', rev); };
  await cache.rowsFor('g/A', 'rev-1', fetch('rev-1'));
  await cache.rowsFor('g/A', 'rev-1', fetch('rev-1'));
  assert.equal(fetches, 1, 'same revision is served from memory');
  const fresh = await cache.rowsFor('g/A', 'rev-2', fetch('rev-2'));
  assert.equal(fresh.revision_id, 'rev-2');
  assert.equal(fetches, 2, 'a saved change (new revision) reloads');
  cache.dropRows(['g/A']);
  await cache.rowsFor('g/A', 'rev-2', fetch('rev-2'));
  assert.equal(fetches, 3, 'a manual reload skips the cache');
  // An unknown revision always fetches.
  await cache.rowsFor('g/A', undefined, fetch('rev-2'));
  assert.equal(fetches, 4);
});

test('results are shared, peekable once finished, and failures are not kept', async () => {
  const cache = new DataCache();
  let runs = 0;
  const compute = async () => { runs += 1; return { value: 42 }; };
  const [a, b] = await Promise.all([cache.result('k', compute), cache.result('k', compute)]);
  assert.equal(runs, 1, 'identical in-flight requests share one computation');
  assert.equal(a, b);
  assert.deepEqual(cache.peek('k'), { value: 42 });
  assert.equal(cache.peek('other'), undefined);
  await assert.rejects(cache.result('bad', async () => { throw new Error('boom'); }));
  let retried = false;
  await cache.result('bad', async () => { retried = true; return 1; });
  assert.ok(retried, 'a failed result is computed again next time');
});

test('preload loads files in order within the row budget', async () => {
  const cache = new DataCache();
  const loaded: string[] = [];
  const groups = { rows: async (path: string) => { loaded.push(path); return rows(path, 'rev-1'); } };
  await cache.preload(groups, [
    { path: 'g/A', revision: 'rev-1', rowCount: 10 },
    { path: 'g/Broken', revision: null },
    { path: 'g/B', revision: 'rev-1', rowCount: 10 },
  ]);
  assert.deepEqual(loaded, ['g/A'], 'a file without a revision stops the preload');
  loaded.length = 0;
  await cache.preload(groups, [
    { path: 'g/C', revision: 'rev-1', rowCount: PRELOAD_ROW_LIMIT },
    { path: 'g/D', revision: 'rev-1', rowCount: 1 },
  ]);
  assert.deepEqual(loaded, ['g/C'], 'stops once the row budget is used');
  // Preloaded rows are then served without another fetch.
  await cache.rowsFor('g/C', 'rev-1', async () => { throw new Error('should be cached'); });
});
