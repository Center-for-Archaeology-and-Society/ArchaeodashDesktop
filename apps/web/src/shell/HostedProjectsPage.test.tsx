/**
 * Hosted projects page: Delete with a confirm step, the Trash with purge
 * dates, Restore and Delete now, and closing the open project when it is
 * deleted. node:test + renderToString.
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import type { TrashedHostedProject } from '@archaeodash/client';
import { HostedWorkspaceContext } from './hosted.tsx';
import { DeleteConfirm, HostedProjectsPage, TrashList, purgeText, trashProject } from './HostedProjectsPage.tsx';

const now = new Date('2026-10-30T12:00:00Z');

function trashed(name: string, purgeAt: string): TrashedHostedProject {
  return {
    project_id: `id-${name}`,
    name,
    created_at: '2026-10-01T00:00:00Z',
    updated_at: '2026-10-29T12:00:00Z',
    deleted_at: '2026-10-29T12:00:00Z',
    purge_at: purgeAt,
  };
}

test('purge text counts the days left until the sweep', () => {
  assert.equal(purgeText('2026-11-28T12:00:00Z', now), 'Deleted permanently in 29 days');
  assert.equal(purgeText('2026-10-31T06:00:00Z', now), 'Deleted permanently in 1 day');
  assert.equal(purgeText('2026-10-30T11:00:00Z', now), 'Deleted permanently within the hour');
  assert.equal(purgeText('not a date', now), 'Deleted permanently after the retention period');
});

test('the Trash lists deleted projects with Restore and Delete now', () => {
  const noop = () => {};
  const empty = renderToString(<TrashList trash={[]} retentionDays={30} busy={false} now={now} onRestore={noop} onDeleteNow={noop} />);
  assert.ok(empty.includes('Trash'));
  assert.ok(empty.includes('The Trash is empty.'));
  assert.match(empty, /stay here for (<!-- -->)?30(<!-- -->)? days/);

  const html = renderToString(
    <TrashList
      trash={[trashed('Field 2025', '2026-11-28T12:00:00Z')]}
      retentionDays={30}
      busy={false}
      now={now}
      onRestore={noop}
      onDeleteNow={noop}
      confirmFor={(id) => (id === 'id-Field 2025' ? <span>confirm here</span> : null)}
    />,
  );
  assert.ok(html.includes('Field 2025'));
  assert.ok(html.includes('Deleted permanently in 29 days'));
  assert.ok(html.includes('aria-label="Restore Field 2025"'));
  assert.ok(html.includes('aria-label="Delete Field 2025 now"'));
  assert.ok(html.includes('confirm here'));
});

test('the confirm step explains the Trash, or that a permanent delete cannot be undone', () => {
  const noop = () => {};
  const project = { project_id: 'p1', name: 'Survey' };
  const toTrash = renderToString(
    <DeleteConfirm pending={{ kind: 'trash', project }} retentionDays={30} busy={false} onConfirm={noop} onCancel={noop} />,
  );
  assert.ok(toTrash.includes('Move “Survey” to the Trash?'));
  assert.ok(toTrash.includes('for 30 days'));
  assert.ok(toTrash.includes('permanently deleted'));
  assert.ok(toTrash.includes('Move to Trash'));
  assert.ok(toTrash.includes('Cancel'));

  const forGood = renderToString(
    <DeleteConfirm pending={{ kind: 'purge', project }} retentionDays={30} busy={false} onConfirm={noop} onCancel={noop} />,
  );
  assert.ok(forGood.includes('Permanently delete “Survey”'));
  assert.ok(forGood.includes('cannot be undone'));
  assert.ok(forGood.includes('Delete permanently'));
});

test('deleting the open project closes it; deleting another project does not', async () => {
  const deleted: string[] = [];
  const projects = { delete: async (id: string) => void deleted.push(id) };
  let closed = 0;
  const workspace = { projectId: 'p1', open: () => {}, close: () => void (closed += 1) };

  await trashProject(projects, workspace, { project_id: 'p2', name: 'Other' });
  assert.deepEqual(deleted, ['p2']);
  assert.equal(closed, 0);

  await trashProject(projects, workspace, { project_id: 'p1', name: 'Open one' });
  assert.deepEqual(deleted, ['p2', 'p1']);
  assert.equal(closed, 1);

  // No hosted workspace (or a failed delete): nothing is closed.
  await trashProject(projects, null, { project_id: 'p1', name: 'Open one' });
  await assert.rejects(
    trashProject({ delete: async () => Promise.reject(new Error('404')) }, workspace, { project_id: 'p1', name: 'x' }),
  );
  assert.equal(closed, 1);
});

test('the projects page shows the Trash section in hosted mode', () => {
  const projects = {
    list: async () => ({ projects: [] }),
    trash: async () => ({ projects: [], retention_days: 30 }),
    quota: async () => ({ logical_bytes: 0, reserved_bytes: 0, file_count: 0, limit_bytes: 1 }),
  } as never;
  const html = renderToString(
    <HostedWorkspaceContext.Provider value={{ projectId: '', open: () => {} }}>
      <HostedProjectsPage projects={projects} />
    </HostedWorkspaceContext.Provider>,
  );
  assert.ok(html.includes('id="trash-heading"'));
  assert.ok(html.includes('The Trash is empty.'));
  // Without the hosted service (desktop) there is no project list or Trash.
  const desktop = renderToString(<HostedProjectsPage />);
  assert.ok(!desktop.includes('Trash'));
});
