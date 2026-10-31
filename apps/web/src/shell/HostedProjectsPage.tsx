/**
 * Hosted projects page (Section 10.2 client surface): the signed-in user's
 * projects — list, create, open, and delete one. Opening a project points
 * the analysis pages and the Data Manager at it; importing source files
 * happens in the Data Manager, inside the open project. Deleted projects go
 * to the Trash, where they can be restored until the server purges them.
 * Rendered only when the transport exposes `hostedProjects` (hosted HTTP
 * mode); desktop projects are opened directories instead.
 */
import { useCallback, useContext, useEffect, useState, type FormEvent, type ReactElement, type ReactNode } from 'react';
import type { HostedProjectsService, QuotaUsage, TrashedHostedProject } from '@archaeodash/client';
import { HostedWorkspaceContext, type HostedWorkspace } from './hosted.tsx';

export interface HostedProjectsPageProps {
  readonly projects?: HostedProjectsService;
}

/** Storage used, e.g. "12.5 MB of 1.0 GB used", or null when unknown. */
export function usageText(quota: QuotaUsage | null): string | null {
  if (!quota) return null;
  const used = quota.logical_bytes + quota.reserved_bytes + (quota.workspace_bytes ?? 0);
  const size = (bytes: number) => {
    const units = ['bytes', 'KB', 'MB', 'GB', 'TB'];
    let value = bytes;
    let unit = 0;
    while (value >= 1000 && unit < units.length - 1) {
      value /= 1000;
      unit += 1;
    }
    return unit === 0 ? `${value} ${units[0]}` : `${value.toFixed(1)} ${units[unit]}`;
  };
  return `${size(used)} of ${size(quota.limit_bytes)} used`;
}

/** Days the server keeps a trashed project, until it reports its own setting. */
export const DEFAULT_TRASH_DAYS = 30;

/** When a trashed project will be deleted for good, relative to `now`. */
export function purgeText(purgeAt: string, now: Date = new Date()): string {
  const at = Date.parse(purgeAt);
  if (Number.isNaN(at)) return 'Deleted permanently after the retention period';
  const days = Math.ceil((at - now.getTime()) / 86_400_000);
  if (days <= 0) return 'Deleted permanently within the hour';
  return `Deleted permanently in ${days} ${days === 1 ? 'day' : 'days'}`;
}

type Named = { project_id: string; name: string };

/** Which delete is waiting for confirmation. */
export type PendingDelete = { kind: 'trash' | 'purge'; project: Named };

/** The confirm step for moving a project to the Trash or deleting it for good. */
export function DeleteConfirm({
  pending,
  retentionDays,
  busy,
  onConfirm,
  onCancel,
}: {
  pending: PendingDelete;
  retentionDays: number;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}): ReactElement {
  const { kind, project } = pending;
  const message =
    kind === 'trash'
      ? `Move “${project.name}” to the Trash? You can restore it from the Trash for ${retentionDays} days; after that it is permanently deleted.`
      : `Permanently delete “${project.name}” and all of its groups and files now? This cannot be undone.`;
  return (
    <div role="group" aria-label={`Confirm delete of ${project.name}`} className="hosted-project-confirm">
      <p>{message}</p>
      <button type="button" className="btn btn-sm btn-danger" disabled={busy} onClick={onConfirm}>
        {kind === 'trash' ? 'Move to Trash' : 'Delete permanently'}
      </button>
      <button type="button" className="btn btn-sm btn-ghost" disabled={busy} onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}

/** The Trash: deleted projects with their purge dates, Restore and Delete now. */
export function TrashList({
  trash,
  retentionDays,
  busy,
  now,
  onRestore,
  onDeleteNow,
  confirmFor,
}: {
  trash: readonly TrashedHostedProject[];
  retentionDays: number;
  busy: boolean;
  now?: Date;
  onRestore: (project: TrashedHostedProject) => void;
  onDeleteNow: (project: TrashedHostedProject) => void;
  /** The confirm step to show under one project, if any. */
  confirmFor?: (projectId: string) => ReactNode;
}): ReactElement {
  return (
    <section aria-labelledby="trash-heading" className="hosted-trash">
      <h2 id="trash-heading">Trash</h2>
      <p>
        Deleted projects stay here for {retentionDays} days, then they are permanently deleted. They still count toward
        your storage until then.
      </p>
      {trash.length === 0 ? (
        <p>The Trash is empty.</p>
      ) : (
        <ul aria-label="Deleted projects" className="hosted-project-list">
          {trash.map((project) => (
            <li key={project.project_id}>
              <div className="hosted-project-row">
                <span className="hosted-project-name">{project.name}</span>
                <span className="project-current" title={new Date(project.purge_at).toLocaleString()}>
                  {purgeText(project.purge_at, now)}
                </span>
                <button
                  type="button"
                  className="btn btn-sm"
                  disabled={busy}
                  onClick={() => onRestore(project)}
                  aria-label={`Restore ${project.name}`}
                >
                  Restore
                </button>
                <button
                  type="button"
                  className="btn btn-sm btn-ghost"
                  disabled={busy}
                  onClick={() => onDeleteNow(project)}
                  aria-label={`Delete ${project.name} now`}
                >
                  Delete now
                </button>
              </div>
              {confirmFor?.(project.project_id)}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * Moves a project to the Trash; if it is the open project, closes it so the
 * app never keeps working on a deleted project.
 */
export async function trashProject(
  projects: Pick<HostedProjectsService, 'delete'>,
  workspace: HostedWorkspace | null,
  project: Named,
): Promise<void> {
  await projects.delete(project.project_id);
  if (workspace?.projectId === project.project_id) workspace.close?.();
}

function errorText(error: unknown): string {
  const envelope =
    typeof error === 'object' && error !== null && 'envelope' in error
      ? (error as { envelope?: { code?: unknown } }).envelope
      : error;
  const code =
    typeof envelope === 'object' && envelope !== null && 'code' in envelope
      ? String(envelope.code)
      : '';
  switch (code) {
    case 'quota_exceeded':
      return 'Your storage is full. Delete groups or projects (and empty them from the Trash) to make room.';
    case 'not_found':
    case 'http_404':
      return 'That project no longer exists. The list has been refreshed.';
    case 'invalid_project_name':
      return 'Project names are 1–200 characters.';
    case 'unauthorized':
    case 'http_401':
      return 'Sign in on the Account page to see your projects.';
    default:
      return 'Something went wrong. Please try again.';
  }
}

export function HostedProjectsPage({ projects }: HostedProjectsPageProps): ReactElement {
  const [list, setList] = useState<Named[]>([]);
  const [trash, setTrash] = useState<TrashedHostedProject[]>([]);
  const [retentionDays, setRetentionDays] = useState(DEFAULT_TRASH_DAYS);
  const [pending, setPending] = useState<PendingDelete | null>(null);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [quota, setQuota] = useState<QuotaUsage | null>(null);
  // Hosted server, signed in: opening a project scopes the analysis pages to it.
  const workspace = useContext(HostedWorkspaceContext);

  /** Reloads the projects, the Trash and storage use; `clearError` drops an old error. */
  const refreshProjects = useCallback((clearError = true) => {
    if (!projects) return;
    setBusy(true);
    projects
      .list()
      .then((res) => {
        setList(res.projects);
        if (clearError) setError('');
      })
      .catch((err) => setError(errorText(err)))
      .finally(() => setBusy(false));
    projects
      .trash()
      .then((res) => {
        setTrash(res.projects);
        setRetentionDays(res.retention_days);
      })
      .catch(() => setTrash([]));
    projects.quota().then(setQuota, () => setQuota(null));
  }, [projects]);

  useEffect(() => {
    refreshProjects();
  }, [refreshProjects]);

  if (!projects) {
    return (
      <section aria-labelledby="projects-heading">
        <h1 id="projects-heading">Projects</h1>
        <p>Hosted projects require a hosted deployment and a signed-in account.</p>
      </section>
    );
  }

  /** Runs one catalog change, then shows `done` and refreshes the lists. */
  const change = (action: () => Promise<unknown>, done: string) => {
    setBusy(true);
    action()
      .then(() => {
        setNotice(done);
        setError('');
      })
      .catch((err) => {
        setNotice('');
        setError(errorText(err));
      })
      .finally(() => {
        setPending(null);
        setBusy(false);
        refreshProjects(false);
      });
  };

  const create = (event: FormEvent) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    projects
      .create(trimmed)
      .then((created) => {
        setName('');
        setNotice(`Project “${trimmed}” created.`);
        setError('');
        if (workspace && created) workspace.open(created);
        else refreshProjects();
      })
      .catch((err) => {
        setNotice('');
        setError(errorText(err));
      })
      .finally(() => setBusy(false));
  };

  const confirmPending = () => {
    if (!pending) return;
    const { kind, project } = pending;
    if (kind === 'trash') {
      change(() => trashProject(projects, workspace, project), `“${project.name}” moved to the Trash.`);
    } else {
      change(() => projects.deletePermanently(project.project_id), `“${project.name}” permanently deleted.`);
    }
  };

  const restore = (project: Named) =>
    change(() => projects.restore(project.project_id), `“${project.name}” restored.`);

  const confirm = pending && (
    <DeleteConfirm
      pending={pending}
      retentionDays={retentionDays}
      busy={busy}
      onConfirm={confirmPending}
      onCancel={() => setPending(null)}
    />
  );

  return (
    <section aria-labelledby="projects-heading">
      <h1 id="projects-heading">Projects</h1>
      <p>Open a project to work with it. Import source files from the Data Manager once the project is open.</p>
      {usageText(quota) && <p aria-label="Storage used">{usageText(quota)}</p>}
      {error && (
        <p role="alert" className="form-error">
          {error}
        </p>
      )}
      {notice && (
        <p role="status" className="form-notice">
          {notice}
        </p>
      )}
      <form onSubmit={create} aria-label="Create project">
        <label>
          New project name
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={200}
            required
          />
        </label>
        <button type="submit" disabled={busy || !name.trim()}>
          Create project
        </button>
      </form>
      <h2>Your projects</h2>
      {list.length === 0 ? (
        <p>No projects yet.</p>
      ) : (
        <ul aria-label="Hosted projects" className="hosted-project-list">
          {list.map((project) => {
            const isOpen = workspace?.projectId === project.project_id;
            return (
              <li key={project.project_id}>
                <div className="hosted-project-row">
                  <span className="hosted-project-name">{project.name}</span>
                  {workspace &&
                    (isOpen ? (
                      <span className="project-current">Open now</span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => workspace.open(project)}
                        aria-label={`Open ${project.name}`}
                      >
                        Open
                      </button>
                    ))}
                  <button
                    type="button"
                    className="btn btn-sm btn-ghost"
                    disabled={busy}
                    onClick={() => setPending({ kind: 'trash', project })}
                    aria-label={`Delete ${project.name}`}
                  >
                    Delete
                  </button>
                </div>
                {pending?.kind === 'trash' && pending.project.project_id === project.project_id && confirm}
              </li>
            );
          })}
        </ul>
      )}
      <TrashList
        trash={trash}
        retentionDays={retentionDays}
        busy={busy}
        onRestore={restore}
        onDeleteNow={(project) => setPending({ kind: 'purge', project })}
        confirmFor={(projectId) =>
          pending?.kind === 'purge' && pending.project.project_id === projectId ? confirm : null}
      />
    </section>
  );
}
