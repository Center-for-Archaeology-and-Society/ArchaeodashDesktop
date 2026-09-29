import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { MoveConfirmation, ResultAssignment } from './ResultAssignment.tsx';
import type { AnalysisResult } from './AnalysisPage.tsx';
const uuid = '01970000-0000-7000-8000-000000000001';
const result: AnalysisResult = { kind: 'fit', data: { path: 'source.parquet', revision_id: 'hidden-revision', analytical_uuids: [uuid], n_rows: 1, method: 'kmeans', cluster: [1] } };

test('selection hides UUIDs, excludes source destination and requires a review before confirmation', () => {
  const html = renderToStaticMarkup(<ResultAssignment result={result} destinations={['source.parquet', 'target.parquet']} busy={false} onConfirm={() => {}} />);
  assert.match(html, /type="checkbox"/);
  assert.match(html, /Analytical unit 1/);
  assert.match(html, /Cluster 1/);
  assert.match(html, /Review move/);
  assert.match(html, /disabled=""/);
  assert.doesNotMatch(html, new RegExp(uuid + '|hidden-revision|Confirm move|<option[^>]*>source.parquet'));
});

test('confirmation names the effect and destination while keeping request identities hidden', () => {
  const html = renderToStaticMarkup(<MoveConfirmation request={{ action: 'move', source_path: 'source.parquet', destination_path: 'target.parquet', selected_uuids: [uuid], expected_source_revision: 'hidden-revision' }} busy onConfirm={() => {}} onCancel={() => {}} />);
  assert.match(html, /Move 1 selected analytical units/);
  assert.match(html, /target.parquet/);
  assert.match(html, /empty source group file is removed/);
  assert.match(html, /Confirm move/);
  assert.equal((html.match(/disabled=""/g) ?? []).length, 2);
  assert.doesNotMatch(html, new RegExp(uuid + '|hidden-revision'));
});

test('missing result identities fail closed and do not expose move controls', () => {
  const missing: AnalysisResult = { kind: 'fit', data: { ...result.data, analytical_uuids: [] } as never };
  const html = renderToStaticMarkup(<ResultAssignment result={missing} destinations={['target.parquet']} busy={false} onConfirm={() => {}} />);
  assert.match(html, /No assignable analytical units/);
  assert.doesNotMatch(html, /Review move|type="checkbox"/);
});
