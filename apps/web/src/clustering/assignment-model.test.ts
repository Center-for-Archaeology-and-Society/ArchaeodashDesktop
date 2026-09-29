import assert from 'node:assert/strict';
import test from 'node:test';
import type { ClusterFitResponse, EuclideanMatchesResponse, MembershipProbabilitiesResponse } from '@archaeodash/client';
import { buildAssignmentRequest, selectableRows } from './assignment-model.ts';

const a = '00000000-0000-4000-8000-000000000001';
const b = '00000000-0000-4000-8000-000000000002';
const c = '00000000-0000-4000-8000-000000000003';
const fit: ClusterFitResponse = {
  path: 'groups/source.parquet', revision_id: 'revision-newer-than-analysis', method: 'kmeans', n_rows: 3,
  analytical_uuids: [a, b, c], cluster: [1, 2, 1], size: [2, 1],
};

test('cluster selections address UUIDs even when visible identifiers duplicate', () => {
  const rows = selectableRows({ kind: 'fit', data: fit });
  assert.deepEqual(rows.map(row => row.analyticalUuid), [a, b, c]);
  assert.deepEqual(rows.map(row => row.label), ['Analytical unit 1', 'Analytical unit 2', 'Analytical unit 3']);
  assert.deepEqual(rows.map(row => row.detail), ['Cluster 1', 'Cluster 2', 'Cluster 1']);
  assert.deepEqual(selectableRows({ kind: 'fit', data: { ...fit, cluster: [1, 0, 1] } }), []);
});

test('hierarchical selection maps a requested cut back to input row order', () => {
  const data: ClusterFitResponse = {
    ...fit, method: 'hclust_ward_d2', cluster: null,
    merge: [[-1, -2], [1, -3]], height: [1, 2], order: [2, 1, 3],
  };
  const rows = selectableRows({ kind: 'fit', data });
  assert.equal(rows.length, 3);
  assert.equal(rows[0]?.analyticalUuid, a);
  assert.equal(rows[0]?.detail, rows[1]?.detail);
  assert.notEqual(rows[0]?.detail, rows[2]?.detail);
  assert.equal(selectableRows({ kind: 'fit', data }, 1).every(row => row.detail === 'Cluster 1'), true);
});

test('membership vectors fail closed on length mismatch and expose identity order', () => {
  const membership: MembershipProbabilitiesResponse = {
    path: 'groups/source.parquet', revision_id: 'r', effective_method: 'hotellings', eligible_groups: ['x'],
    analytical_uuids: [a, b], ids: ['same', 'same'], groups: ['x', 'x'], probabilities: [[1], [1]],
    best_group: ['x', 'x'], best_value: [1, 1], in_group: [true, true],
  };
  assert.deepEqual(selectableRows({ kind: 'membership', data: membership }).map(row => row.analyticalUuid), [a, b]);
  assert.deepEqual(selectableRows({ kind: 'membership', data: { ...membership, best_value: [1] } }), []);
  assert.deepEqual(selectableRows({ kind: 'membership', data: { ...membership, analytical_uuids: undefined as never } }), []);
});

test('Euclidean repeated matches deduplicate observation UUIDs, not visible IDs', () => {
  const euclidean: EuclideanMatchesResponse = {
    path: 'groups/source.parquet', revision_id: 'r', rows: [
      { analytical_uuid: a, match_analytical_uuid: b, rowid: 'hidden-a', id: 'duplicate', match_id: 'm1', distance: 1, group: 'g', match_group: 'h' },
      { analytical_uuid: a, match_analytical_uuid: b, rowid: 'hidden-a', id: 'duplicate', match_id: 'm2', distance: 2, group: 'g', match_group: 'h' },
      { analytical_uuid: b, match_analytical_uuid: a, rowid: 'hidden-b', id: 'duplicate', match_id: 'm1', distance: 3, group: 'g', match_group: 'h' },
      { analytical_uuid: 'bad', match_analytical_uuid: a, rowid: 'hidden-c', id: 'other', match_id: 'm1', distance: 4, group: 'g', match_group: 'h' },
    ],
  };
  assert.deepEqual(selectableRows({ kind: 'euclidean', data: euclidean }).map(row => row.analyticalUuid), [a, b]);
});

test('assignment validates selection and carries the result snapshot revision', () => {
  const request = buildAssignmentRequest({ kind: 'fit', data: fit }, [a, a, b], 'groups/destination.parquet', 'Destination');
  assert.deepEqual(request.selected_uuids, [a, b]);
  assert.equal(request.action, 'move');
  assert.equal(request.expected_source_revision, 'revision-newer-than-analysis');
  assert.equal(request.source_path, fit.path);
  assert.equal(request.destination_group_name, 'Destination');
  assert.throws(() => buildAssignmentRequest({ kind: 'fit', data: fit }, ['bad'], 'groups/destination.parquet'));
  assert.throws(() => buildAssignmentRequest({ kind: 'fit', data: fit }, [a], fit.path));
  assert.throws(() => buildAssignmentRequest({ kind: 'fit', data: fit }, [a], 'groups/destination.parquet', ' '));
  assert.throws(() => buildAssignmentRequest({ kind: 'fit', data: { ...fit, revision_id: '' } }, [a], 'groups/destination.parquet'));
});
