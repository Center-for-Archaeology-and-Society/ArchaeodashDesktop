/**
 * Data Manager sidebar: a compact file manager for the project's group files.
 * Toolbar (Import…, Refresh, Duplicate, Merge, Delete), a keyboard-accessible
 * tree (role="tree") with multi-select, inline name/confirm forms, a details
 * panel for the selected file, and the import wizard in a modal dialog.
 */
import { useCallback, useEffect, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent, type ReactElement } from 'react';
import type { GroupSummary } from '@archaeodash/contracts';
import { ImportDialog } from './ImportDialog.tsx';
import { ImportWizard } from './ImportWizard.tsx';
import { onGroupsChanged } from './events.ts';
import { applySelection, pruneSelection, type SelectionState } from './selection.ts';
import type { DataManagerTransport } from './types.ts';
import './file-manager.css';

export type { DataManagerTransport } from './types.ts';

export interface GroupFileEntry {
  readonly path: string;
  readonly name: string;
  readonly summary: GroupSummary | null;
  /** Revision for optimistic-concurrency writes (validate() or scan()). */
  readonly revision: string | null;
  /** Validation error; empty when the file is valid. */
  readonly error: string;
}

export interface DataManagerProps {
  readonly transport: DataManagerTransport;
  readonly projectName?: string;
  /** Called after the sidebar itself changed project data (import, duplicate, merge, delete). */
  readonly onDataChanged?: () => void;
}

type Pending =
  | { readonly kind: 'duplicate'; readonly name: string }
  | { readonly kind: 'merge'; readonly name: string }
  | { readonly kind: 'delete' }
  | null;

const FOLDER_ID = '__groups_folder__';
const message = (cause: unknown) => (cause instanceof Error ? cause.message : String(cause));
const basename = (path: string) => path.split(/[\\/]/).at(-1) ?? path;
const plural = (count: number, noun: string) => `${count} ${noun}${count === 1 ? '' : 's'}`;
const stripExtension = (name: string) => name.replace(/\.[^.]+$/, '');

export async function loadGroupFiles(groups: DataManagerTransport['groups']): Promise<GroupFileEntry[]> {
  const found = await groups.scan();
  return Promise.all(
    found.map(async (candidate): Promise<GroupFileEntry> => {
      const fallbackName = candidate.group?.group_name ?? stripExtension(basename(candidate.path));
      try {
        const summary = await groups.validate(candidate.path);
        return { path: candidate.path, name: summary.group_name || fallbackName, summary, revision: summary.revision_id, error: '' };
      } catch (cause) {
        return {
          path: candidate.path,
          name: fallbackName,
          summary: candidate.group ?? null,
          revision: candidate.group?.revision_id ?? null,
          error: message(cause) || 'Validation failed',
        };
      }
    }),
  );
}

export function DataManager({ transport, projectName = '', onDataChanged }: DataManagerProps): ReactElement {
  const [entries, setEntries] = useState<GroupFileEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [selection, setSelection] = useState<SelectionState>({ selected: [], anchor: null });
  const [folderOpen, setFolderOpen] = useState(true);
  const [focusId, setFocusId] = useState<string>(FOLDER_ID);
  const [pending, setPending] = useState<Pending>(null);
  const [importOpen, setImportOpen] = useState(false);
  const loadSeq = useRef(0);
  const itemRefs = useRef(new Map<string, HTMLLIElement>());

  const refresh = useCallback(async () => {
    const seq = ++loadSeq.current;
    setLoading(true);
    try {
      const next = await loadGroupFiles(transport.groups);
      if (seq !== loadSeq.current) return;
      setEntries(next);
      const order = next.map((entry) => entry.path);
      setSelection((current) => pruneSelection(order, current));
      setFocusId((current) => (current === FOLDER_ID || order.includes(current) ? current : FOLDER_ID));
      setError('');
    } catch (cause) {
      if (seq === loadSeq.current) setError(message(cause));
    } finally {
      if (seq === loadSeq.current) setLoading(false);
    }
  }, [transport]);

  useEffect(() => {
    void refresh();
    return onGroupsChanged(() => void refresh());
  }, [refresh]);

  const order = entries.map((entry) => entry.path);
  const selectedEntries = entries.filter((entry) => selection.selected.includes(entry.path));
  const single = selectedEntries.length === 1 ? selectedEntries[0]! : null;
  const visibleIds = folderOpen ? [FOLDER_ID, ...order] : [FOLDER_ID];

  const focusItem = (id: string) => {
    setFocusId(id);
    itemRefs.current.get(id)?.focus();
  };

  const select = (path: string, modifiers: { toggle?: boolean; range?: boolean } = {}) => {
    setSelection((current) => applySelection(order, current, path, modifiers));
    setFocusId(path);
    setPending((current) => (current?.kind === 'delete' ? null : current));
  };

  const onRowClick = (event: MouseEvent, path: string) => {
    select(path, { toggle: event.ctrlKey || event.metaKey, range: event.shiftKey });
  };

  const runChange = async (work: () => Promise<string[] | void>, done: string) => {
    setBusy(true);
    setError('');
    setStatus('');
    try {
      const nextSelected = await work();
      setPending(null);
      await refresh();
      if (nextSelected) setSelection({ selected: nextSelected, anchor: nextSelected[0] ?? null });
      setStatus(done);
      onDataChanged?.();
    } catch (cause) {
      setError(message(cause));
      // Revisions may be stale after a partial failure; rescan.
      void refresh();
    } finally {
      setBusy(false);
    }
  };

  const submitDuplicate = (event: FormEvent) => {
    event.preventDefault();
    if (pending?.kind !== 'duplicate' || !single?.revision) return;
    const name = pending.name.trim();
    if (!name) return;
    void runChange(async () => {
      const result = await transport.groups.duplicateGroup({ source_path: single.path, expected_revision: single.revision!, new_group_name: name });
      return result.outputs.map((output) => output.path);
    }, `Duplicated ${single.name} as ${name}.`);
  };

  const submitMerge = (event: FormEvent) => {
    event.preventDefault();
    if (pending?.kind !== 'merge' || selectedEntries.length < 2) return;
    const name = pending.name.trim();
    if (!name) return;
    const sources = selectedEntries.map((entry) => entry.path);
    void runChange(async () => {
      const result = await transport.groups.mergeGroups({ sources, new_group_name: name });
      return result.outputs.map((output) => output.path);
    }, `Merged ${sources.length} files into ${name}.`);
  };

  const confirmDelete = () => {
    const targets = selectedEntries.filter((entry) => entry.revision);
    if (!targets.length) return;
    void runChange(async () => {
      for (const entry of targets) {
        await transport.groups.deleteGroup({ path: entry.path, expected_revision: entry.revision!, confirm_path: entry.path });
      }
      return [];
    }, `Deleted ${targets.map((entry) => entry.name).join(', ')}.`);
  };

  const canDuplicate = !busy && single !== null && !!single.revision;
  const canMerge = !busy && selectedEntries.length >= 2 && selectedEntries.every((entry) => !entry.error);
  const canDelete = !busy && selectedEntries.length > 0 && selectedEntries.every((entry) => entry.revision);

  const onTreeKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const index = visibleIds.indexOf(focusId);
    const isFolder = focusId === FOLDER_ID;
    const move = (to: number) => {
      const id = visibleIds[Math.max(0, Math.min(visibleIds.length - 1, to))]!;
      focusItem(id);
      if (event.shiftKey && id !== FOLDER_ID) select(id, { range: true });
    };
    switch (event.key) {
      case 'ArrowDown': event.preventDefault(); move(index + 1); break;
      case 'ArrowUp': event.preventDefault(); move(index - 1); break;
      case 'Home': event.preventDefault(); move(0); break;
      case 'End': event.preventDefault(); move(visibleIds.length - 1); break;
      case 'ArrowRight':
        event.preventDefault();
        if (isFolder && !folderOpen) setFolderOpen(true);
        else if (isFolder && order.length) focusItem(order[0]!);
        break;
      case 'ArrowLeft':
        event.preventDefault();
        if (isFolder) setFolderOpen(false);
        else focusItem(FOLDER_ID);
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        if (isFolder) setFolderOpen((open) => !open);
        else select(focusId, { toggle: event.key === ' ' || event.ctrlKey || event.metaKey, range: event.shiftKey });
        break;
      case 'a':
        if (event.ctrlKey || event.metaKey) {
          event.preventDefault();
          setSelection({ selected: order, anchor: order[0] ?? null });
        }
        break;
      case 'Delete':
        if (canDelete) { event.preventDefault(); setPending({ kind: 'delete' }); }
        break;
      case 'Escape':
        setSelection({ selected: [], anchor: null });
        setPending(null);
        break;
    }
  };

  const setRef = (id: string) => (element: HTMLLIElement | null) => {
    if (element) itemRefs.current.set(id, element);
    else itemRefs.current.delete(id);
  };

  return (
    <div className="file-manager">
      <header className="fm-header">
        <span className="fm-project" title={projectName || undefined}>
          <FolderIcon open />
          <span className="fm-project-name" aria-label="Current project">{projectName || 'Project'}</span>
        </span>
        <div className="fm-toolbar" role="toolbar" aria-label="File actions">
          <button type="button" className="fm-tool fm-tool-primary" disabled={busy} onClick={() => setImportOpen(true)}>Import…</button>
          <button type="button" className="fm-tool" disabled={busy || loading} onClick={() => { setStatus(''); void refresh(); }} aria-label="Refresh" title="Refresh">⟳</button>
          <span className="fm-tool-sep" aria-hidden="true" />
          <button type="button" className="fm-tool" disabled={!canDuplicate} aria-pressed={pending?.kind === 'duplicate'} onClick={() => single && setPending({ kind: 'duplicate', name: `${single.name} copy` })}>Duplicate</button>
          <button type="button" className="fm-tool" disabled={!canMerge} aria-pressed={pending?.kind === 'merge'} onClick={() => setPending({ kind: 'merge', name: 'Merged' })}>Merge</button>
          <button type="button" className="fm-tool fm-tool-danger" disabled={!canDelete} aria-pressed={pending?.kind === 'delete'} onClick={() => setPending({ kind: 'delete' })}>Delete</button>
        </div>
      </header>

      {pending?.kind === 'duplicate' && single && (
        <form className="fm-inline" onSubmit={submitDuplicate} aria-label="Duplicate file">
          <label>Name for the copy of {single.name}
            <input autoFocus value={pending.name} disabled={busy} onChange={(event) => setPending({ kind: 'duplicate', name: event.target.value })} onKeyDown={(event) => { if (event.key === 'Escape') setPending(null); }} />
          </label>
          <div className="fm-inline-actions">
            <button type="submit" disabled={busy || !pending.name.trim()}>Duplicate</button>
            <button type="button" disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </form>
      )}
      {pending?.kind === 'merge' && selectedEntries.length >= 2 && (
        <form className="fm-inline" onSubmit={submitMerge} aria-label="Merge files">
          <label>Merge {selectedEntries.map((entry) => entry.name).join(', ')} into
            <input autoFocus value={pending.name} disabled={busy} onChange={(event) => setPending({ kind: 'merge', name: event.target.value })} onKeyDown={(event) => { if (event.key === 'Escape') setPending(null); }} />
          </label>
          <div className="fm-inline-actions">
            <button type="submit" disabled={busy || !pending.name.trim()}>Merge</button>
            <button type="button" disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </form>
      )}
      {pending?.kind === 'delete' && selectedEntries.length > 0 && (
        <div className="fm-inline fm-inline-danger" role="group" aria-label="Confirm delete">
          <p>Delete {selectedEntries.map((entry) => entry.name).join(', ')}? This removes {selectedEntries.length === 1 ? 'the group file' : `${selectedEntries.length} group files`} from the project.</p>
          <div className="fm-inline-actions">
            <button type="button" className="fm-danger" disabled={busy} onClick={confirmDelete}>Confirm</button>
            <button type="button" autoFocus disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </div>
      )}

      <GroupFileTree
        entries={entries}
        selected={selection.selected}
        folderOpen={folderOpen}
        focusId={focusId}
        loading={loading}
        onToggleFolder={() => { setFolderOpen((open) => !open); setFocusId(FOLDER_ID); }}
        onRowClick={onRowClick}
        onCheck={(path) => select(path, { toggle: true })}
        onKeyDown={onTreeKeyDown}
        onFocusItem={setFocusId}
        setRef={setRef}
      />

      <div className="fm-messages">
        {status && <p role="status">{status}</p>}
        {error && <p role="alert">{error}</p>}
      </div>

      <GroupDetails entries={selectedEntries} />

      {importOpen && (
        <ImportDialog title="Import data" onClose={() => setImportOpen(false)}>
          <ImportWizard
            transport={transport}
            onClose={() => setImportOpen(false)}
            onImported={async () => {
              await refresh();
              onDataChanged?.();
            }}
          />
        </ImportDialog>
      )}
    </div>
  );
}

export interface GroupFileTreeProps {
  readonly entries: readonly GroupFileEntry[];
  readonly selected: readonly string[];
  readonly folderOpen?: boolean;
  readonly focusId?: string;
  readonly loading?: boolean;
  readonly onToggleFolder?: () => void;
  readonly onRowClick?: (event: MouseEvent, path: string) => void;
  readonly onCheck?: (path: string) => void;
  readonly onKeyDown?: (event: KeyboardEvent<HTMLUListElement>) => void;
  readonly onFocusItem?: (id: string) => void;
  readonly setRef?: (id: string) => (element: HTMLLIElement | null) => void;
}

/** Presentational tree: a collapsible "groups" folder with one row per group file. */
export function GroupFileTree({
  entries,
  selected,
  folderOpen = true,
  focusId = FOLDER_ID,
  loading = false,
  onToggleFolder,
  onRowClick,
  onCheck,
  onKeyDown,
  onFocusItem,
  setRef,
}: GroupFileTreeProps): ReactElement {
  const rovingFocus = entries.some((entry) => entry.path === focusId) && folderOpen ? focusId : FOLDER_ID;
  return (
    <ul className="fm-tree" role="tree" aria-label="Project files" aria-multiselectable="true" aria-busy={loading} onKeyDown={onKeyDown}>
      <li
        role="treeitem"
        aria-expanded={folderOpen}
        aria-level={1}
        aria-label={`groups, ${entries.length} file${entries.length === 1 ? '' : 's'}`}
        tabIndex={rovingFocus === FOLDER_ID ? 0 : -1}
        ref={setRef?.(FOLDER_ID)}
        onFocus={(event) => { if (event.target === event.currentTarget) onFocusItem?.(FOLDER_ID); }}
        className="fm-item fm-item-folder"
      >
        <div className="fm-row" onClick={onToggleFolder}>
          <span className="fm-twisty" aria-hidden="true">{folderOpen ? '▾' : '▸'}</span>
          <FolderIcon open={folderOpen} />
          <span className="fm-name">groups</span>
          <span className="fm-meta">{loading && !entries.length ? 'Loading…' : entries.length}</span>
        </div>
        {folderOpen && (
          <ul role="group" className="fm-group">
            {!entries.length && !loading && <li role="none" className="fm-empty">No group files yet. Use Import… to add data.</li>}
            {entries.map((entry) => {
              const isSelected = selected.includes(entry.path);
              const rows = entry.summary?.row_count;
              return (
                <li
                  key={entry.path}
                  role="treeitem"
                  aria-level={2}
                  aria-selected={isSelected}
                  aria-invalid={entry.error ? true : undefined}
                  aria-label={`${entry.name}${rows === undefined ? '' : `, ${plural(rows, 'row')}`}${entry.error ? `, needs attention: ${entry.error}` : ''}`}
                  tabIndex={rovingFocus === entry.path ? 0 : -1}
                  ref={setRef?.(entry.path)}
                  title={entry.error ? `Needs attention: ${entry.error}` : entry.path}
                  className={`fm-item fm-item-file${isSelected ? ' is-selected' : ''}${entry.error ? ' is-invalid' : ''}`}
                  onFocus={(event) => { if (event.target === event.currentTarget) onFocusItem?.(entry.path); }}
                  data-path={entry.path}
                >
                  <div className="fm-row" onClick={(event) => onRowClick?.(event, entry.path)}>
                    <span
                      className="fm-check"
                      aria-hidden="true"
                      onClick={(event) => { event.stopPropagation(); onCheck?.(entry.path); }}
                    >
                      {isSelected ? '✓' : ''}
                    </span>
                    <FileIcon />
                    <span className="fm-name">{entry.name}</span>
                    <span className="fm-meta">{rows === undefined ? '' : plural(rows, 'row')}</span>
                    {entry.error && (
                      <span className="fm-warn" role="img" aria-label={`Needs attention: ${entry.error}`}>⚠</span>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </li>
    </ul>
  );
}

/** Compact details for the current selection. */
export function GroupDetails({ entries }: { entries: readonly GroupFileEntry[] }): ReactElement {
  if (!entries.length) {
    return <section className="fm-details" aria-label="File details"><p className="fm-details-hint">Select a file to see its details. Ctrl/Cmd-click or Shift-click to select several.</p></section>;
  }
  if (entries.length > 1) {
    const rows = entries.reduce((sum, entry) => sum + (entry.summary?.row_count ?? 0), 0);
    return (
      <section className="fm-details" aria-label="File details">
        <h3>{entries.length} files selected</h3>
        <dl><dt>Total rows</dt><dd>{rows}</dd></dl>
      </section>
    );
  }
  const entry = entries[0]!;
  const summary = entry.summary;
  return (
    <section className="fm-details" aria-label="File details">
      <h3>{entry.name}</h3>
      <dl>
        <dt>Path</dt><dd className="fm-path">{entry.path}</dd>
        <dt>Rows</dt><dd>{summary?.row_count ?? '—'}</dd>
        <dt>Measured</dt><dd>{summary ? plural(summary.elemental_columns.length, 'column') : '—'}</dd>
        <dt>Descriptive</dt><dd>{summary ? plural(summary.descriptive_columns.length, 'column') : '—'}</dd>
        <dt>Source</dt><dd className="fm-path" title={summary?.source_path ?? undefined}>{summary?.source_path ? basename(summary.source_path) : '—'}</dd>
      </dl>
      {entry.error && <p className="fm-details-error">Needs attention: {entry.error}</p>}
    </section>
  );
}

function FileIcon(): ReactElement {
  return (
    <svg className="fm-icon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false">
      <path d="M3.5 1.5h6l3 3v10h-9z" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
      <path d="M9.5 1.5v3h3M5.5 8h5M5.5 10.5h5M5.5 13h3" fill="none" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
    </svg>
  );
}

function FolderIcon({ open = false }: { open?: boolean }): ReactElement {
  return (
    <svg className="fm-icon fm-icon-folder" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false">
      {open
        ? <path d="M1.5 3.5h4.5l1.5 1.5h6.5v2H4l-2.5 6.5z M4 7h11l-2.5 6.5h-11z" fill="currentColor" fillOpacity="0.35" stroke="currentColor" strokeWidth="1.1" strokeLinejoin="round" />
        : <path d="M1.5 3.5h4.5l1.5 1.5h7v8.5h-13z" fill="currentColor" fillOpacity="0.35" stroke="currentColor" strokeWidth="1.1" strokeLinejoin="round" />}
    </svg>
  );
}
