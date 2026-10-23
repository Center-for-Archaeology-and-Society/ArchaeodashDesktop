import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ActivityTracker, activityLabel } from './activity.ts';
import { requestKey } from './transport.ts';

test('activity labels name the work for Tauri commands and HTTP requests', () => {
  assert.equal(activityLabel('group_rows'), 'Loading group data');
  assert.equal(activityLabel('batch_transfer_units'), 'Assigning units to groups');
  assert.equal(activityLabel('POST /api/v1/groups/transfer-units'), 'Assigning units to groups');
  assert.equal(activityLabel('explore_histogram'), 'Computing histogram');
  assert.equal(activityLabel('ordination_umap'), 'Computing UMAP');
  assert.equal(activityLabel('export_measured_data'), 'Preparing export');
  assert.equal(activityLabel('save_transformation'), 'Saving analysis setup');
  // Dialogs and quick bookkeeping are not shown.
  for (const quiet of ['save_export_file', 'open_project', 'pick_and_upload_source', 'preferences_get', 'get_analysis_job', 'list_transformations', 'GET /api/v1/transformations']) {
    assert.equal(activityLabel(quiet), null, quiet);
  }
  assert.equal(activityLabel('something_new'), 'Working');
});

test('http request keys are method plus path', () => {
  assert.equal(requestKey('/api/v1/groups/rows?path=a', undefined), 'GET /api/v1/groups/rows');
  assert.equal(requestKey('https://host/api/v1/ordination/pca', { method: 'post' }), 'POST /api/v1/ordination/pca');
});

test('tracker counts running work and reports the latest label', async () => {
  const tracker = new ActivityTracker();
  let notified = 0;
  tracker.subscribe(() => { notified += 1; });
  let finishA!: () => void;
  let failB!: (e: Error) => void;
  const a = tracker.track('Loading group data', new Promise<void>((resolve) => { finishA = resolve; }));
  const b = tracker.track('Computing PCA', new Promise<void>((_, reject) => { failB = reject; }));
  tracker.track(null, Promise.resolve());
  assert.deepEqual(tracker.snapshot(), { count: 2, label: 'Computing PCA' });
  failB(new Error('boom'));
  await b.catch(() => undefined);
  assert.deepEqual(tracker.snapshot(), { count: 1, label: 'Loading group data' });
  finishA();
  await a;
  assert.deepEqual(tracker.snapshot(), { count: 0, label: '' });
  assert.equal(notified, 4);
});
