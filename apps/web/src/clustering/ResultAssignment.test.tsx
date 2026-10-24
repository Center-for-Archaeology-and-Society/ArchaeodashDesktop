import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { MoveConfirmation, ResultAssignment } from './ResultAssignment.tsx';
import type { AnalysisResult } from './AnalysisPage.tsx';
const uuid = '01970000-0000-7000-8000-000000000001';
const result: AnalysisResult = { kind: 'fit', data: { path: 'source.parquet', revision_id: 'hidden-revision', analytical_uuids: [uuid], n_rows: 1, method: 'kmeans', cluster: [1] } };

test('assignment reflects the table selection, hides UUIDs and excludes the source destination', () => {
  const html = renderToStaticMarkup(<ResultAssignment result={result} destinations={['source.parquet', 'target.parquet']} busy={false} onConfirm={() => {}} />);
  assert.match(html, /0 of 1 samples selected/);
  assert.match(html, /Review move/);
  assert.match(html, /disabled=""/);
  assert.match(html, /Create a new group…/);
  assert.doesNotMatch(html, new RegExp(uuid + '|hidden-revision|Confirm move|<option[^>]*>source.parquet'));
  const chosen = renderToStaticMarkup(<ResultAssignment result={result} selected={new Set([uuid])} onSelectedChange={() => {}} destinations={['target.parquet']} busy={false} onConfirm={() => {}} />);
  assert.match(chosen, /1 of 1 samples selected/);
  assert.doesNotMatch(chosen, new RegExp(uuid));
});

test('Euclidean selection counts match rows and samples and offers the nearest-match shortcut', () => {
  const other = '01970000-0000-7000-8000-000000000002';
  const euclidean: AnalysisResult = { kind: 'euclidean', data: { path: 'source.parquet', revision_id: 'r', rows: [
    { analytical_uuid: uuid, match_analytical_uuid: other, rowid: '1', id: 'S1', match_id: 'S2', distance: 1, group: 'A', match_group: 'B' },
    { analytical_uuid: uuid, match_analytical_uuid: other, rowid: '1', id: 'S1', match_id: 'S3', distance: 2, group: 'A', match_group: 'C' },
  ] } };
  const html = renderToStaticMarkup(<ResultAssignment result={euclidean} selected={new Set([`${uuid}#2`])} onSelectedChange={() => {}} destinations={['target.parquet']} busy={false} onConfirm={() => {}} onBatchConfirm={() => {}} />);
  assert.match(html, /1 match rows checked \(1 of 1 samples\)/);
  assert.match(html, /Check the nearest match of every sample/);
  assert.match(html, /Use checked match groups/);
  assert.doesNotMatch(html, new RegExp(uuid + '|' + other));
});

test('confirmation names the effect and destination while keeping request identities hidden', () => {
  const html = renderToStaticMarkup(<MoveConfirmation request={{ action: 'move', source_path: 'source.parquet', destination_path: 'target.parquet', selected_uuids: [uuid], expected_source_revision: 'hidden-revision' }} busy onConfirm={() => {}} onCancel={() => {}} />);
  assert.match(html, /Move 1 selected analytical units/);
  assert.match(html, /target.parquet/);
  assert.match(html, /empty source group file is removed/);
  // Busy: the confirm button shows a spinner and what it is doing.
  assert.match(html, /<span class="spinner" aria-hidden="true"><\/span>Moving…/);
  assert.equal((html.match(/disabled=""/g) ?? []).length, 2);
  const idle = renderToStaticMarkup(<MoveConfirmation request={{ action: 'move', source_path: 'source.parquet', destination_path: 'target.parquet', selected_uuids: [uuid], expected_source_revision: 'hidden-revision' }} busy={false} onConfirm={() => {}} onCancel={() => {}} />);
  assert.match(idle, />Confirm move<\/button>/);
  assert.doesNotMatch(html, new RegExp(uuid + '|hidden-revision'));
});

test('missing result identities fail closed and do not expose move controls', () => {
  const missing: AnalysisResult = { kind: 'fit', data: { ...result.data, analytical_uuids: [] } as never };
  const html = renderToStaticMarkup(<ResultAssignment result={missing} destinations={['target.parquet']} busy={false} onConfirm={() => {}} />);
  assert.match(html, /No assignable analytical units/);
  assert.doesNotMatch(html, /Review move|type="checkbox"/);
});

test('confirmation names a new group by name and file', () => {
  const html = renderToStaticMarkup(<MoveConfirmation request={{ action: 'move', source_path: 'source.parquet', destination_path: 'groups/Fresh.parquet', destination_group_name: 'Fresh', selected_uuids: [uuid], expected_source_revision: 'hidden-revision' }} busy={false} onConfirm={() => {}} onCancel={() => {}} />);
  assert.match(html, /the new group Fresh \(groups\/Fresh.parquet\)/);
});
