import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupCandidate, OrdinationService } from '@archaeodash/client';
import { planPrecompute, precomputeStatusText } from './precompute.ts';
import { UMAP_SEED, newSetup } from './setup.ts';

const candidate = (name: string, elements: string[], ready = true): GroupCandidate => ({
  path: `groups/${name}.parquet`,
  ready,
  group: { path: `groups/${name}.parquet`, group_id: name, group_name: name, revision_id: 'rev-1', row_count: 10, elemental_columns: elements, descriptive_columns: ['Site'] },
});

test('no precompute choices means no work', () => {
  const plan = planPrecompute(newSetup(['na', 'fe']), [candidate('A', ['na', 'fe'])]);
  assert.deepEqual(plan.jobs, []);
  assert.deepEqual(plan.skipped, []);
});

test('precompute sends the pages\' all-groups requests, slowest last', () => {
  const setup = { ...newSetup(['fe', 'na', 'cs']), transform_method: 'log10' as const, precompute: ['umap', 'pca', 'lda'] as ('umap' | 'pca' | 'lda')[] };
  const groups = [candidate('A', ['na', 'fe', 'cs']), candidate('B', ['na', 'fe']), candidate('Broken', ['na'], false), candidate('C', ['fe', 'na', 'k'])];
  const plan = planPrecompute(setup, groups);
  assert.deepEqual(plan.jobs.map((j) => j.kind), ['pca', 'lda', 'umap']);
  // Ready groups in scan order; columns every group shares, in the first group's order.
  assert.equal(plan.requests.pca?.path, 'groups/A.parquet');
  assert.deepEqual(plan.requests.pca?.extra_paths, ['groups/B.parquet', 'groups/C.parquet']);
  assert.deepEqual(plan.requests.pca?.columns, ['na', 'fe']);
  assert.equal(plan.requests.pca?.transformation?.transform_method, 'log10');
  assert.equal(plan.requests.umap?.seed, UMAP_SEED);
  // LDA as Visualize (by file) and as Ordination (synthetic group column).
  assert.deepEqual(plan.requests.lda.map((r) => [r.group_column, r.group_by_file ?? false]), [['', true], ['Group', false]]);
});

test('LDA needs three groups and every ordination needs two shared elements', () => {
  const setup = { ...newSetup(['na', 'fe']), precompute: ['pca', 'lda'] as ('pca' | 'lda')[] };
  const two = planPrecompute(setup, [candidate('A', ['na', 'fe']), candidate('B', ['na', 'fe'])]);
  assert.deepEqual(two.jobs.map((j) => j.kind), ['pca']);
  assert.deepEqual(two.skipped, [{ kind: 'lda', reason: 'needs three or more groups' }]);
  const narrow = planPrecompute(setup, [candidate('A', ['na']), candidate('B', ['na', 'fe'])]);
  assert.equal(narrow.jobs.length, 0);
  assert.equal(narrow.skipped.length, 2);
  assert.match(narrow.skipped[0]!.reason, /two selected elements/);
});

test('jobs call the ordination service with the planned requests', async () => {
  const calls: string[] = [];
  const service = {
    pca: async () => { calls.push('pca'); return {} as never; },
    umap: async (r: { seed?: number | null }) => { calls.push(`umap:${r.seed}`); return {} as never; },
    lda: async (r: { group_by_file?: boolean }) => { calls.push(`lda:${r.group_by_file ? 'file' : 'column'}`); return {} as never; },
  } as unknown as OrdinationService;
  const setup = { ...newSetup(['na', 'fe']), precompute: ['pca', 'umap', 'lda'] as ('pca' | 'umap' | 'lda')[] };
  const plan = planPrecompute(setup, ['A', 'B', 'C'].map((n) => candidate(n, ['na', 'fe'])));
  for (const job of plan.jobs) await job.run(service);
  assert.deepEqual(calls, ['pca', 'lda:file', 'lda:column', `umap:${UMAP_SEED}`]);
  assert.equal(precomputeStatusText('umap', 'computing'), 'UMAP computing…');
  assert.equal(precomputeStatusText('lda', { skipped: 'needs three or more groups' }), 'LDA skipped (needs three or more groups)');
});
