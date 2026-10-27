import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { ClusterFitResponse } from '@archaeodash/client';
import {
  DEFAULT_CLUSTER_NAME_PREFIX, buildApplyClustersRequest, clusterGroupNames, clusterMembers, clusterNamePrefixProblem, folderOfPath,
} from './cluster-groups.ts';

const ids = ['0197aaaa-0000-7000-8000-000000000001', '0197aaaa-0000-7000-8000-000000000002', '0197aaaa-0000-7000-8000-000000000003'];
const fit = (overrides: Partial<ClusterFitResponse>): ClusterFitResponse => ({
  path: 'group2\\A.parquet', revision_id: 'rev-1', analytical_uuids: ids, method: 'kmeans', n_rows: 3,
  cluster: null, size: null, tot_withinss: null, centers: null, medoids: null, merge: null, height: null, order: null, silhouette: null,
  ...overrides,
} as ClusterFitResponse);

test('partition results list each cluster’s samples, cluster 1 first', () => {
  assert.deepEqual(clusterMembers(fit({ cluster: [2, 1, 2] }), 2), [[ids[1]], [ids[0], ids[2]]]);
});

test('hierarchical results are cut into the requested number of clusters', () => {
  const tree = fit({ method: 'hclust', merge: [[-1, -2], [1, -3]], height: [1, 2], order: [1, 2, 3] });
  assert.deepEqual(clusterMembers(tree, 2), [[ids[0], ids[1]], [ids[2]]]);
  assert.deepEqual(clusterMembers(tree, 1), [ids]);
});

test('the request covers every sample, keeps Windows source paths, and needs loaded sources', () => {
  const sources = [{ path: 'group2\\B.parquet', expected_revision: 'rev-4' }];
  const request = buildApplyClustersRequest(fit({ cluster: [1, 1, 2] }), 2, sources, 'replace');
  assert.deepEqual(request, { sources, clusters: [[ids[0], ids[1]], [ids[2]]], mode: 'replace', name_prefix: 'Cluster ' });
  assert.throws(() => buildApplyClustersRequest(fit({ cluster: [1, 1, 2] }), 2, [], 'new_folder'), /not loaded/);
  assert.throws(() => clusterMembers(fit({ cluster: [1, 0, 2] }), 2), /incomplete/);
  assert.throws(() => clusterMembers(fit({ cluster: [1, 2, 2], analytical_uuids: [] }), 2), /identities/);
});

test('cluster group names use the default or a custom prefix', () => {
  const sources = [{ path: 'group2\\B.parquet', expected_revision: 'rev-4' }];
  const data = fit({ cluster: [1, 1, 2] });
  assert.equal(DEFAULT_CLUSTER_NAME_PREFIX, 'Cluster ');
  assert.equal(clusterGroupNames(DEFAULT_CLUSTER_NAME_PREFIX, 3), 'Cluster 1 to Cluster 3');
  assert.equal(clusterGroupNames('Cluster 5-', 1), 'Cluster 5-1');
  assert.equal(clusterGroupNames('Cluster 5-', 2), 'Cluster 5-1 to Cluster 5-2');
  assert.equal(buildApplyClustersRequest(data, 2, sources, 'new_folder', 'Cluster 5-').name_prefix, 'Cluster 5-');
  // Leading spaces are dropped, a trailing space is kept.
  assert.equal(buildApplyClustersRequest(data, 2, sources, 'new_folder', '  Site ').name_prefix, 'Site ');
  for (const ok of ['Cluster ', 'Cluster 5-', 'COM ', 'Grupo ñ_']) assert.equal(clusterNamePrefixProblem(ok), null, ok);
});

test('invalid cluster name prefixes are refused', () => {
  const sources = [{ path: 'group2\\B.parquet', expected_revision: 'rev-4' }];
  const cases: [string, RegExp][] = [
    ['', /Enter the text/], ['   ', /Enter the text/], ['a/b ', /cannot contain/], ['a\\b ', /cannot contain/],
    ['k: ', /cannot contain/], ['why? ', /cannot contain/], ['tab\t', /cannot contain/],
    ['.hidden ', /dot/], ['com', /Windows reserves/], ['x'.repeat(101), /longer than 100/],
  ];
  for (const [prefix, why] of cases) {
    assert.match(clusterNamePrefixProblem(prefix) ?? '', why, JSON.stringify(prefix));
  }
  assert.throws(() => buildApplyClustersRequest(fit({ cluster: [1, 1, 2] }), 2, sources, 'replace', 'bad/name '), /cannot contain/);
});

test('folders of Windows and slash paths', () => {
  assert.equal(folderOfPath('group2\\A.parquet'), 'group2');
  assert.equal(folderOfPath('a/b/C.parquet'), 'a/b');
  assert.equal(folderOfPath('C.parquet'), '');
});
