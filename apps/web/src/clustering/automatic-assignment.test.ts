import assert from 'node:assert/strict';
import test from 'node:test';
import type { AnalysisResult } from './AnalysisPage.tsx';
import type { GroupCandidate } from '@archaeodash/client';
import { buildAutomaticAssignmentRequest, recommendedAssignments } from './automatic-assignment.ts';

const a = '11111111-1111-4111-8111-111111111111';
const b = '22222222-2222-4222-8222-222222222222';
const c = '33333333-3333-4333-8333-333333333333';
const fit: AnalysisResult = { kind: 'fit', data: {
  analytical_uuids: [a, b, c], path: 'source.parquet', revision_id: 'source-rev', method: 'kmeans', n_rows: 3, cluster: [1, 2, 1],
} };
const membership: AnalysisResult = { kind: 'membership', data: {
  analytical_uuids: [a, b], path: 'source.parquet', revision_id: 'source-rev', effective_method: 'hotellings',
  eligible_groups: ['North', 'South'], ids: ['a', 'b'], groups: ['North', 'South'], probabilities: [[0.8, 0.2], [0.1, 0.9]],
  best_group: ['North', 'South'], best_value: [0.8, 0.9], in_group: [true, true],
} };
const candidates: GroupCandidate[] = [{
  path: 'north.parquet', ready: true,
  group: { path: 'north.parquet', group_id: 'g', group_name: 'North', revision_id: 'north-rev', row_count: 4,
    elemental_columns: [], descriptive_columns: [] },
}];

test('fit recommendations use partitions and dendrogram cuts', () => {
  assert.deepEqual(recommendedAssignments(fit, [c, a, a]), [
    { analyticalUuid: c, groupLabel: 'Cluster 1' }, { analyticalUuid: a, groupLabel: 'Cluster 1' },
  ]);
  const dendrogram: AnalysisResult = { kind: 'fit', data: {
    ...fit.data, cluster: undefined, n_rows: 3, merge: [[-1, -2], [1, -3]], height: [1, 2], order: [1, 2, 3],
  } };
  assert.deepEqual(recommendedAssignments(dendrogram, [b, c], 2), [
    { analyticalUuid: b, groupLabel: 'Cluster 1' }, { analyticalUuid: c, groupLabel: 'Cluster 2' },
  ]);
});

test('membership requires a finite best score and usable group for every selected row', () => {
  assert.deepEqual(recommendedAssignments(membership, [a]), [{ analyticalUuid: a, groupLabel: 'North' }]);
  const repeatedDisplayIds: AnalysisResult = { kind: 'membership', data: { ...membership.data, ids: ['same visible ID', 'same visible ID'] } };
  assert.deepEqual(recommendedAssignments(repeatedDisplayIds, [b, a]), [
    { analyticalUuid: b, groupLabel: 'South' }, { analyticalUuid: a, groupLabel: 'North' },
  ]);
  const missingScore = () => recommendedAssignments({ kind: 'membership', data: {
    ...membership.data, best_value: [null, 0.9],
  } }, [a]);
  assert.throws(missingScore, error => error instanceof Error && /eligible group/.test(error.message) && !error.message.includes(a));
  assert.throws(() => recommendedAssignments({ kind: 'membership', data: {
    ...membership.data, best_group: ['Unknown', 'South'],
  } }, [a]), /eligible group/);
});

test('Euclidean recommendations select the nearest finite group and reject cross-group ties', () => {
  const euclidean: AnalysisResult = { kind: 'euclidean', data: { path: 'source.parquet', revision_id: 'source-rev', rows: [
    { analytical_uuid: a, match_analytical_uuid: a, rowid: 'self', id: 'A', match_id: 'A', distance: 0, group: 'old', match_group: 'Self' },
    { analytical_uuid: a, match_analytical_uuid: b, rowid: '1', id: 'A', match_id: 'B', distance: null, group: 'old', match_group: 'Ignored' },
    { analytical_uuid: a, match_analytical_uuid: b, rowid: '2', id: 'A', match_id: 'B', distance: 4, group: 'old', match_group: 'South' },
    { analytical_uuid: a, match_analytical_uuid: c, rowid: '3', id: 'A', match_id: 'C', distance: 2, group: 'old', match_group: 'North' },
  ] } };
  assert.deepEqual(recommendedAssignments(euclidean, [a]), [{ analyticalUuid: a, groupLabel: 'North' }]);
  assert.throws(() => recommendedAssignments({ ...euclidean, data: { ...euclidean.data, rows: [
    ...euclidean.data.rows, { ...euclidean.data.rows[3]!, match_group: 'South' },
  ] } }, [a]), error => error instanceof Error && /choose a destination manually/i.test(error.message) && !error.message.includes(a));
  const malformedNearest: AnalysisResult = { ...euclidean, data: { ...euclidean.data, rows: [
    { ...euclidean.data.rows[2]!, distance: 0, match_group: '' },
    { ...euclidean.data.rows[3]!, distance: 1, match_group: 'North' },
  ] } };
  assert.throws(() => recommendedAssignments(malformedNearest, [a]), error =>
    error instanceof Error && /incomplete identity or group/.test(error.message) && !error.message.includes(a));
  assert.throws(() => recommendedAssignments(euclidean, [b]), /valid analytical units/);
});

test('batch requests require complete explicit mappings and capture destination revisions', () => {
  const request = buildAutomaticAssignmentRequest(fit, [a, b], [
    { groupLabel: 'Cluster 1', destinationPath: 'north.parquet' },
    { groupLabel: 'Cluster 2', destinationPath: 'south-new.parquet', newGroupName: 'South' },
  ], candidates);
  assert.deepEqual(request, {
    source_path: 'source.parquet', expected_source_revision: 'source-rev', targets: [
      { destination_path: 'north.parquet', expected_destination_revision: 'north-rev', selected_uuids: [a] },
      { destination_path: 'south-new.parquet', destination_group_name: 'South', expected_destination_revision: null, selected_uuids: [b] },
    ],
  });
  assert.throws(() => buildAutomaticAssignmentRequest(fit, [a, b], [{ groupLabel: 'Cluster 1', destinationPath: 'north.parquet' }], candidates), /every recommended group/i);
  assert.throws(() => buildAutomaticAssignmentRequest(fit, [a, b], [
    { groupLabel: 'Cluster 1', destinationPath: 'north.parquet' },
    { groupLabel: 'Cluster 2', destinationPath: 'north.parquet' },
  ], candidates), /distinct/i);
  assert.throws(() => buildAutomaticAssignmentRequest(fit, [a, b], [
    { groupLabel: 'Cluster 1', destinationPath: 'north.parquet' },
    { groupLabel: 'Cluster 2', destinationPath: '../escape.parquet', newGroupName: 'South' },
  ], candidates), /safe destination/i);
  assert.throws(() => buildAutomaticAssignmentRequest(fit, [a, b], [
    { groupLabel: 'Cluster 1', destinationPath: 'north.parquet' },
    { groupLabel: 'Cluster 2', destinationPath: 'north.parquet', newGroupName: 'Collision' },
  ], candidates), /distinct/i);
});

test('new paths cannot collide with unready candidates and source labels can be kept', () => {
  const allCandidates: GroupCandidate[] = [...candidates, { path: 'reserved.parquet', ready: false }];
  assert.throws(() => buildAutomaticAssignmentRequest(fit, [a, b], [
    { groupLabel: 'Cluster 1', destinationPath: 'north.parquet' },
    { groupLabel: 'Cluster 2', destinationPath: 'reserved.parquet', newGroupName: 'South' },
  ], allCandidates), /unused by every group candidate/i);
  assert.throws(() => buildAutomaticAssignmentRequest(fit, [a, c], [
    { groupLabel: 'Cluster 1', destinationPath: 'source.parquet' },
  ], candidates), /already in its recommended group/i);
  const partial = buildAutomaticAssignmentRequest(fit, [a, b], [
    { groupLabel: 'Cluster 1', destinationPath: 'source.parquet' },
    { groupLabel: 'Cluster 2', destinationPath: 'north.parquet' },
  ], candidates);
  assert.deepEqual(partial.targets.map(target => target.selected_uuids), [[b]]);
});
