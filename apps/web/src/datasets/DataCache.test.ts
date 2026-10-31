import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupRowsResponse } from '@archaeodash/client';
import { DataCache, PRELOAD_ROW_LIMIT, rowsVersion } from './DataCache.tsx';

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

test('a new file under a path used before is not served the old rows', async () => {
  // New group files all start at the same revision: a one-row group made by
  // assigning a sample, deleted, then a 92-row group copied in under the same
  // file name has the same path and revision.
  const summary = (rowCount: number) => ({
    revision_id: 'rev-1',
    group_id: 'Gwynn_Canyon',
    row_count: rowCount,
    elemental_columns: ['Rb', 'Sr'],
    descriptive_columns: [],
  });
  const path = 'artifacts/Gwynn_Canyon.parquet';
  const response = (n: number): GroupRowsResponse => ({
    ...rows(path, 'rev-1'),
    rows: Array.from({ length: n }, (_, i) => ({
      analytical_uuid: `u${i}`,
      legacy_rowid: null,
      visible_id: `s${i}`,
      descriptive: [],
      elemental: [1, 2],
    })),
  });
  const cache = new DataCache();
  cache.observeScan([path]);
  await cache.rowsFor(path, rowsVersion(summary(1)), async () => response(1));
  await cache.result('explore:profile', async () => 'old result');
  // The group is deleted (the next scan no longer lists it)…
  cache.observeScan([]);
  assert.equal(cache.peek('explore:profile'), undefined, 'results of a removed file are dropped');
  // …and another file is copied in under the same name.
  cache.observeScan([path]);
  const copied = await cache.rowsFor(path, rowsVersion(summary(92)), async () => response(92));
  assert.equal(copied.rows.length, 92);
  // Same path and revision without a scan in between: the version still differs.
  const other = new DataCache();
  await other.rowsFor(path, rowsVersion(summary(1)), async () => response(1));
  const fresh = await other.rowsFor(path, rowsVersion(summary(92)), async () => response(92));
  assert.equal(fresh.rows.length, 92);
  assert.equal(rowsVersion(null), undefined);
});
