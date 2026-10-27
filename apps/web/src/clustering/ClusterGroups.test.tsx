import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ClusterFitResponse } from '@archaeodash/client';
import { ClusterGroupsAssignment } from './ClusterGroups.tsx';

const ids = ['0197aaaa-0000-7000-8000-000000000001', '0197aaaa-0000-7000-8000-000000000002', '0197aaaa-0000-7000-8000-000000000003'];
const data = {
  path: 'group2/A.parquet', revision_id: 'rev-1', analytical_uuids: ids, method: 'kmeans', n_rows: 3, cluster: [1, 2, 2],
} as ClusterFitResponse;
const sources = { refs: [{ path: 'group2/A.parquet', expected_revision: 'rev-1' }], names: ['A'] };
const render = (initialPrefix?: string) => renderToStaticMarkup(<ClusterGroupsAssignment data={data} cutK={2} mode="new_folder"
  sources={sources} busy={false} onApply={() => {}} onReviewChange={() => {}} initialPrefix={initialPrefix} />);

test('the name prefix field starts at “Cluster ” and names the groups', () => {
  const html = render();
  assert.match(html, /Group name prefix <input[^>]* value="Cluster "/);
  assert.match(html, /2 groups named Cluster 1 to Cluster 2 in a new folder/);
  assert.match(html, /aria-invalid="false"/);
  assert.match(html, /<button type="button">Review<\/button>/);
});

test('a custom prefix names the groups Cluster 5-1, Cluster 5-2', () => {
  const html = render('Cluster 5-');
  assert.match(html, /value="Cluster 5-"/);
  assert.match(html, /groups named Cluster 5-1 to Cluster 5-2/);
});

test('an invalid prefix explains why and blocks review', () => {
  const html = render('bad/name ');
  assert.match(html, /aria-invalid="true"/);
  assert.match(html, /cannot contain/);
  assert.match(html, /<button type="button" disabled="">Review<\/button>/);
  // The description falls back to the default names instead of the bad ones.
  assert.match(html, /groups named Cluster 1 to Cluster 2/);
});
