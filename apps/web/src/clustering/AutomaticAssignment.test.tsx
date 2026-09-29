import assert from 'node:assert/strict';
import test from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { BatchTransferUnitsRequest, GroupCandidate } from '@archaeodash/client';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { AutomaticAssignment, BatchConfirmation } from './AutomaticAssignment.tsx';

const uuidA = '11111111-1111-4111-8111-111111111111';
const uuidB = '22222222-2222-4222-8222-222222222222';
const membership: AnalysisResult = { kind: 'membership', data: {
  analytical_uuids: [uuidA, uuidB], path: 'source.parquet', revision_id: 'private-source-revision',
  effective_method: 'hotellings', eligible_groups: ['North', 'South'], ids: ['sample A', 'sample B'],
  groups: ['North', 'South'], probabilities: [[0.9, 0.1], [0.2, 0.8]],
  best_group: ['North', 'South'], best_value: [0.9, 0.8], in_group: [true, true],
} };
const candidates: GroupCandidate[] = [
  { path: 'source.parquet', ready: true, group: { path: 'source.parquet', group_id: 's', group_name: 'North', revision_id: 'private-source-revision', row_count: 2, elemental_columns: [], descriptive_columns: [] } },
  { path: 'north.parquet', ready: true, group: { path: 'north.parquet', group_id: 'n', group_name: 'North', revision_id: 'private-north-revision', row_count: 3, elemental_columns: [], descriptive_columns: [] } },
  { path: 'south.parquet', ready: true, group: { path: 'south.parquet', group_id: 's2', group_name: 'South', revision_id: 'private-south-revision', row_count: 4, elemental_columns: [], descriptive_columns: [] } },
];
const noOp = () => {};

test('automatic assignment exposes explicit destination choices without confirming or leaking internal identifiers', () => {
  let calls = 0;
  const markup = renderToStaticMarkup(<AutomaticAssignment result={membership} selected={[uuidA, uuidB]} candidates={candidates} cutK={2} busy={false}
    onConfirm={() => { calls++; }} onReviewChange={noOp} />);
  assert.equal(calls, 0);
  assert.match(markup, /North destination/);
  assert.match(markup, /South destination/);
  assert.match(markup, /north\.parquet/);
  assert.match(markup, /south\.parquet/);
  assert.match(markup, /Keep in current group/);
  assert.match(markup, /Create a new group/);
  assert.match(markup, /No files change until you confirm/);
  assert.doesNotMatch(markup, new RegExp(`${uuidA}|${uuidB}|private-source-revision|private-north-revision|private-south-revision`));
  assert.doesNotMatch(markup, /analytical_uuid|revision_id|expected_destination_revision/);
});

test('empty selection prompts for selection and ambiguous closest groups report safely', () => {
  const empty = renderToStaticMarkup(<AutomaticAssignment result={membership} selected={[]} candidates={candidates} cutK={2} busy={false}
    onConfirm={noOp} onReviewChange={noOp} />);
  assert.match(empty, /Select analytical units above/);
  const euclidean: AnalysisResult = { kind: 'euclidean', data: { path: 'source.parquet', revision_id: 'private-revision', rows: [
    { analytical_uuid: uuidA, match_analytical_uuid: uuidB, rowid: 'r1', id: 'visible A', match_id: 'visible B', distance: 1, group: 'A', match_group: 'North' },
    { analytical_uuid: uuidA, match_analytical_uuid: uuidB, rowid: 'r2', id: 'visible A', match_id: 'visible B', distance: 1, group: 'A', match_group: 'South' },
  ] } };
  const ambiguous = renderToStaticMarkup(<AutomaticAssignment result={euclidean} selected={[uuidA]} candidates={candidates} cutK={2} busy={false}
    onConfirm={noOp} onReviewChange={noOp} />);
  assert.match(ambiguous, /closest Euclidean matches disagree/);
  assert.match(ambiguous, /role="alert"/);
  assert.doesNotMatch(ambiguous, new RegExp(`${uuidA}|${uuidB}|private-revision`));
});

test('batch review previews target counts and kept count before confirming', () => {
  let calls = 0;
  const request: BatchTransferUnitsRequest = {
    source_path: 'source.parquet', expected_source_revision: 'private-source-revision', targets: [
      { destination_path: 'north.parquet', expected_destination_revision: 'private-north-revision', selected_uuids: [uuidA, uuidB] },
      { destination_path: 'new-south.parquet', destination_group_name: 'South new', expected_destination_revision: null, selected_uuids: [uuidA] },
    ],
  };
  const markup = renderToStaticMarkup(<BatchConfirmation request={request} selectedCount={5} busy={false} onConfirm={() => { calls++; }} onCancel={noOp} />);
  assert.equal(calls, 0);
  assert.match(markup, /Move 3 analytical units/);
  assert.match(markup, /2 selected units stay in the source group/);
  assert.match(markup, /<td>north\.parquet<\/td><td>Existing group<\/td><td>2<\/td>/);
  assert.match(markup, /<td>new-south\.parquet<\/td><td>South new<\/td><td>1<\/td>/);
  assert.match(markup, /An empty source group file is removed/);
  assert.doesNotMatch(markup, new RegExp(`${uuidA}|${uuidB}|private-source-revision|private-north-revision`));
  assert.doesNotMatch(markup, /analytical_uuid|revision_id|expected_destination_revision/);
});
