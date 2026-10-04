/**
 * Hosted projects page (Section 10.2 client surface): the per-account
 * project catalog — list, create, and inspect the files of a selected
 * project. Rendered only when the transport exposes `hostedProjects`
 * (hosted HTTP mode); desktop projects are opened directories instead.
 */
import { useCallback, useEffect, useState, type FormEvent, type ReactElement } from 'react';
import type { HostedFileMeta, HostedFilesService, HostedProjectsService } from '@archaeodash/client';

export interface HostedProjectsPageProps {
  readonly projects?: HostedProjectsService;
  readonly files?: HostedFilesService;
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
      return 'Storage quota exceeded. Delete files or contact the operator.';
    case 'invalid_path':
      return 'That path is not allowed (relative, no "..", csv/tsv/xlsx).';
    case 'unsupported_format':
      return 'Unsupported format: use csv, tsv, or xlsx.';
    case 'http_401':
      return 'Sign in on the Account page to manage hosted projects.';
    default:
      return 'Something went wrong. Please try again.';
  }
}

export function HostedProjectsPage({ projects, files }: HostedProjectsPageProps): ReactElement {
  const [list, setList] = useState<{ project_id: string; name: string }[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [files_, setFiles] = useState<HostedFileMeta[]>([]);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

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

  const refreshFiles = useCallback(
    (projectId: string) => {
      if (!files) return;
      setBusy(true);
      files
        .list(projectId)
        .then((res) => {
          setFiles(res.files);
          setError('');
        })
        .catch((err) => setError(errorText(err)))
        .finally(() => setBusy(false));
    },
    [files],
  );

  useEffect(() => {
    refreshProjects();
  }, [refreshProjects]);

  useEffect(() => {
    if (selected) refreshFiles(selected);
    else setFiles([]);
  }, [selected, refreshFiles]);

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
      .then(() => {
        setName('');
        setNotice(`Project “${trimmed}” created.`);
        setError('');
        refreshProjects();
      })
      .catch((err) => {
        setNotice('');
        setError(errorText(err));
      })
      .finally(() => setBusy(false));
  };

  const upload = (event: FormEvent<HTMLFormElement> & { target: HTMLFormElement }) => {
    event.preventDefault();
    if (!files || !selected) return;
    const input = event.target.elements.namedItem('file') as HTMLInputElement | null;
    const pathInput = event.target.elements.namedItem('path') as HTMLInputElement | null;
    const file = input?.files?.[0];
    const path = (pathInput?.value ?? '').trim() || file?.name;
    if (!file || !path) return;
    setBusy(true);
    file
      .arrayBuffer()
      .then((buffer) =>
        files.upload({
          projectId: selected,
          path,
          content: new Uint8Array(buffer),
        }),
      )
      .then(() => {
        setError('');
        refreshFiles(selected);
      })
      .catch((err) => {
        setNotice('');
        setError(errorText(err));
      })
      .finally(() => setBusy(false));
  };

  const removeFile = (fileId: string) => {
    if (!files || !selected) return;
    setBusy(true);
    files
      .delete(fileId)
      .then(() => {
        setError('');
        refreshFiles(selected);
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
        <ul aria-label="Hosted projects">
          {list.map((project) => (
            <li key={project.project_id}>
              <button
                type="button"
                onClick={() => setSelected(project.project_id)}
                aria-pressed={selected === project.project_id}
              >
                {project.name}
              </button>
            </li>
          ))}
        </ul>
      )}
      {selected && files && (
        <>
          <h2>Files</h2>
          <form
            onSubmit={upload}
            aria-label="Upload file"
          >
            <label>
              Logical path (e.g. sources/INAA.csv)
              <input name="path" placeholder="sources/INAA.csv" />
            </label>
            <label>
              File
              <input type="file" name="file" accept=".csv,.tsv,.xlsx" required />
            </label>
            <button type="submit" disabled={busy}>
              Upload
            </button>
          </form>
          {files_.length === 0 ? (
            <p>No files in this project.</p>
          ) : (
            <table>
              <caption>
                Files in the selected project (logical path, size, parse state)
              </caption>
              <thead>
                <tr>
                  <th scope="col">Path</th>
                  <th scope="col">Size</th>
                  <th scope="col">Parse</th>
                  <th scope="col">Actions</th>
                </tr>
              </thead>
              <tbody>
                {files_.map((file) => (
                  <tr key={file.file_id}>
                    <td>{file.logical_path}</td>
                    <td>{file.size_bytes} B</td>
                    <td>{file.parse_state}</td>
                    <td>
                      <button type="button" onClick={() => removeFile(file.file_id)}>
                        Delete
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </>
      )}
    </section>
  );
}
