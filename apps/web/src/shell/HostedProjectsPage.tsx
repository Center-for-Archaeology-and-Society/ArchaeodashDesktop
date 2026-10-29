/**
 * Hosted projects page (Section 10.2 client surface): the signed-in user's
 * projects — list, create, and open one. Opening a project points the
 * analysis pages and the Data Manager at it; importing source files happens
 * in the Data Manager, inside the open project. Rendered only when the
 * transport exposes `hostedProjects` (hosted HTTP mode); desktop projects
 * are opened directories instead.
 */
import { useCallback, useContext, useEffect, useState, type FormEvent, type ReactElement } from 'react';
import type { HostedProjectsService, QuotaUsage } from '@archaeodash/client';
import { HostedWorkspaceContext } from './hosted.tsx';

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
      return 'Your storage is full. Delete groups or projects to make room.';
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
  const [list, setList] = useState<{ project_id: string; name: string }[]>([]);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [quota, setQuota] = useState<QuotaUsage | null>(null);
  // Hosted server, signed in: opening a project scopes the analysis pages to it.
  const workspace = useContext(HostedWorkspaceContext);

  const refreshProjects = useCallback(() => {
    if (!projects) return;
    setBusy(true);
    projects
      .list()
      .then((res) => {
        setList(res.projects);
        setError('');
      })
      .catch((err) => setError(errorText(err)))
      .finally(() => setBusy(false));
  }, [projects]);

  useEffect(() => {
    refreshProjects();
    projects?.quota().then(setQuota, () => setQuota(null));
  }, [projects, refreshProjects]);

  if (!projects) {
    return (
      <section aria-labelledby="projects-heading">
        <h1 id="projects-heading">Projects</h1>
        <p>Hosted projects require a hosted deployment and a signed-in account.</p>
      </section>
    );
  }

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
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
