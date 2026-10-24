/**
 * Data Manager sidebar: a compact file manager for the project's group files.
 * Toolbar (Import…, Export…, Refresh, New folder…, Rename… (folder or one
 * group), Add column…, Copy to…, Move to…, Merge, Delete), a keyboard-accessible
 * tree (role="tree") with multi-select, inline name/confirm forms, a details
 * panel for the selected file, and the import wizard in a modal dialog.
 */
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent, type ReactElement } from 'react';
import type { ExportFormat, GroupFolder, GroupSummary } from '@archaeodash/contracts';
import { activeFolderAmong, exclusionWithin, folderLabel, folderOf, inFolder } from '../datasets/folders.ts';
import { downloadExportResult } from '../exports.ts';
import { BusyLabel, BusyNote, Spinner } from '../ui/Busy.tsx';
import { unionElements } from '../analysis-setup/setup.ts';
import { includedAmong, invertedAmong, shownByFilter, useGroupInclusion, type InclusionFilter } from '../datasets/GroupInclusion.tsx';
import { useDataCache } from '../datasets/DataCache.tsx';
import { AnalysisSetupPanel } from './AnalysisSetupPanel.tsx';
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
  /** Opens the project folder in the system file manager (desktop). */
  readonly onRevealProject?: () => void;
  /** Label for that action, e.g. "Show in File Explorer". */
  readonly revealLabel?: string;
}

type Pending =
  | { readonly kind: 'merge'; readonly name: string }
  | { readonly kind: 'delete' }
  | { readonly kind: 'delete-folder' }
  | { readonly kind: 'export'; readonly format: ExportFormat }
  | { readonly kind: 'new-folder'; readonly name: string }
  | { readonly kind: 'rename-folder'; readonly name: string }
  | { readonly kind: 'rename-group'; readonly name: string }
  | { readonly kind: 'add-column'; readonly folder: string; readonly column: string; readonly value: string; readonly confirmOverwrite: boolean }
  | { readonly kind: 'copy'; readonly move: boolean; readonly folder: string; readonly newName: string }
  | null;

/** Tree ids for folder rows (group rows use their path). */
const FOLDER_PREFIX = '\u0001folder:';
const folderId = (folder: string) => `${FOLDER_PREFIX}${folder}`;
const isFolderId = (id: string) => id.startsWith(FOLDER_PREFIX);
const folderFromId = (id: string) => id.slice(FOLDER_PREFIX.length);
const NEW_FOLDER = '\u0001new';
const message = (cause: unknown) => (cause instanceof Error ? cause.message : String(cause));
const basename = (path: string) => path.split(/[\\/]/).at(-1) ?? path;
const plural = (count: number, noun: string) => `${count} ${noun}${count === 1 ? '' : 's'}`;
const stripExtension = (name: string) => name.replace(/\.[^.]+$/, '');
const quoted = (folder: string) => `“${folderLabel(folder)}”`;

export interface FolderNode {
  readonly path: string;
  readonly name: string;
  readonly entries: readonly GroupFileEntry[];
}

/** Folders of the tree: listed folders (including empty ones) plus every folder that holds a group file. */
export function folderNodes(entries: readonly GroupFileEntry[], listed: readonly GroupFolder[] = []): FolderNode[] {
  const paths = [...new Set([...listed.map((f) => f.path), ...entries.map((e) => folderOf(e.path))])].sort((a, b) => a.localeCompare(b));
  return paths.map((path) => ({ path, name: folderLabel(path), entries: entries.filter((e) => folderOf(e.path) === path) }));
}

/** Groups of `folder` that already have a descriptive column named `column`. */
export function groupsWithColumn(entries: readonly GroupFileEntry[], folder: string, column: string): GroupFileEntry[] {
  const name = column.trim();
  return entries.filter((entry) => folderOf(entry.path) === folder && (entry.summary?.descriptive_columns ?? []).includes(name));
}

/** Why a new descriptive column name is refused before asking the backend ('' when fine). */
export function columnNameProblem(entries: readonly GroupFileEntry[], folder: string, column: string): string {
  const name = column.trim();
  if (!name) return 'Enter a column name.';
  if (['analytical_uuid', 'legacy_rowid', 'rowid'].includes(name.toLowerCase())) return `“${name}” is reserved for sample identity.`;
  const measured = entries.find((entry) => folderOf(entry.path) === folder && (entry.summary?.elemental_columns ?? []).includes(name));
  return measured ? `“${name}” is a measured column in ${measured.name}; measured values cannot be replaced.` : '';
}

/** Keeps exclusions with a renamed group file. */
export function renamedExclusions(excluded: Iterable<string>, from: string, to: string): string[] {
  return [...excluded].map((path) => (path === from ? to : path));
}

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

export function DataManager({ transport, projectName = '', onDataChanged, onRevealProject, revealLabel = 'Open project folder' }: DataManagerProps): ReactElement {
  const [entries, setEntries] = useState<GroupFileEntry[]>([]);
  const [listedFolders, setListedFolders] = useState<GroupFolder[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  /** What the current busy operation is doing, shown with a spinner. */
  const [busyText, setBusyText] = useState('');
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [selection, setSelection] = useState<SelectionState>({ selected: [], anchor: null });
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);
  const [closedFolders, setClosedFolders] = useState<ReadonlySet<string>>(new Set());
  const [focusId, setFocusId] = useState<string>('');
  const [pending, setPending] = useState<Pending>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [treeFilter, setTreeFilter] = useState<InclusionFilter>('all');
  const loadSeq = useRef(0);
  const itemRefs = useRef(new Map<string, HTMLLIElement>());
  const supportsFolders = Boolean(transport.groups.folders && transport.groups.copyGroups);

  const refresh = useCallback(async () => {
    const seq = ++loadSeq.current;
    setLoading(true);
    try {
      const [next, folders] = await Promise.all([
        loadGroupFiles(transport.groups),
        transport.groups.folders ? transport.groups.folders().catch(() => [] as GroupFolder[]) : Promise.resolve([] as GroupFolder[]),
      ]);
      if (seq !== loadSeq.current) return;
      setEntries(next);
      setListedFolders(folders);
      const order = next.map((entry) => entry.path);
      const folderPaths = new Set([...folders.map((f) => f.path), ...order.map(folderOf)]);
      setSelection((current) => pruneSelection(order, current));
      setSelectedFolder((current) => (current !== null && folderPaths.has(current) ? current : null));
      setFocusId((current) => (order.includes(current) || (isFolderId(current) && folderPaths.has(folderFromId(current))) ? current : ''));
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

  const allFolders = useMemo(() => folderNodes(entries, listedFolders), [entries, listedFolders]);

  // Groups included in analyses (checkboxes), shared with every page's
  // Dataset picker. Analyses use one folder at a time; files that need
  // attention cannot be included.
  const inclusion = useGroupInclusion();
  const readyPaths = entries.filter((entry) => entry.summary && !entry.error).map((entry) => entry.path);
  const activeFolder = inclusion ? activeFolderAmong(readyPaths, inclusion.activeFolder) : null;
  const activeReady = inFolder(readyPaths, activeFolder);
  const includedPaths = inclusion ? includedAmong(activeReady, inclusion.excluded) : null;
  const includedSet = includedPaths ? new Set(includedPaths) : null;
  // The tree can show all groups or only those included in (or left out of)
  // analyses; folders stay listed so the folder switch still works.
  const filtering = treeFilter !== 'all' && includedSet !== null;
  const folders = filtering
    ? allFolders.map((folder) => ({ ...folder, entries: folder.entries.filter((entry) => shownByFilter(entry.path, treeFilter, includedSet, activeFolder === null || folder.path === activeFolder)) }))
    : allFolders;
  const order = folders.flatMap((folder) => folder.entries.map((entry) => entry.path));
  const selectedEntries = entries.filter((entry) => selection.selected.includes(entry.path) && order.includes(entry.path));
  const chooseFolder = (folder: string) => inclusion?.setActiveFolder?.(folder);
  // Remember the folder analyses use as soon as it is known, so a folder
  // created or copied into later (which may sort first) never takes over.
  useEffect(() => {
    if (inclusion?.setActiveFolder && (inclusion.activeFolder === null || inclusion.activeFolder === undefined) && activeFolder !== null) {
      inclusion.setActiveFolder(activeFolder);
    }
  }, [inclusion, activeFolder]);
  /** Includes exactly `paths` (all in one folder), switching analyses to that folder. */
  const setIncluded = (paths: readonly string[], folder: string | null = activeFolder) => {
    if (!inclusion) return;
    if (folder !== null && folder !== activeFolder) chooseFolder(folder);
    inclusion.setExcluded(exclusionWithin(inclusion.excluded, inFolder(readyPaths, folder), paths));
  };
  const toggleIncluded = (path: string) => {
    if (!includedPaths || !readyPaths.includes(path)) return;
    const folder = folderOf(path);
    if (folder !== activeFolder) {
      // A group in another folder: analyses switch to that folder.
      chooseFolder(folder);
      inclusion?.setExcluded([...inclusion.excluded].filter((p) => p !== path));
      setStatus(`Analyses now use the folder ${quoted(folder)}.`);
      return;
    }
    setIncluded(includedPaths.includes(path) ? includedPaths.filter((p) => p !== path) : [...includedPaths, path]);
  };
  const includeOnly = (path: string) => { if (readyPaths.includes(path)) setIncluded([path], folderOf(path)); };
  const selectedReady = selectedEntries.map((entry) => entry.path).filter((path) => readyPaths.includes(path));
  const selectedFolders = [...new Set(selectedReady.map(folderOf))];

  // Preload the active folder's rows in the background (included groups
  // first) so analysis pages open without waiting for the data.
  const cache = useDataCache();
  const preloadKey = `${activeFolder ?? ''}|${entries.map((entry) => `${entry.path}@${entry.revision ?? ''}`).join('|')}`;
  useEffect(() => {
    if (!cache) return;
    const ready = entries.filter((entry) => entry.summary && !entry.error && entry.revision && (activeFolder === null || folderOf(entry.path) === activeFolder));
    const excluded = inclusion?.excluded;
    const ordered = [...ready.filter((e) => !excluded?.has(e.path)), ...ready.filter((e) => excluded?.has(e.path))];
    void cache.preload(transport.groups, ordered.map((e) => ({ path: e.path, revision: e.revision, rowCount: e.summary?.row_count })));
    // `preloadKey` captures the folder, file list, and revisions.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cache, preloadKey, transport.groups]);
  const visibleIds = folders.flatMap((folder) => [folderId(folder.path), ...(closedFolders.has(folder.path) ? [] : folder.entries.map((e) => e.path))]);
  const currentFocus = visibleIds.includes(focusId) ? focusId : (visibleIds[0] ?? '');

  const focusItem = (id: string) => {
    setFocusId(id);
    itemRefs.current.get(id)?.focus();
  };

  const select = (path: string, modifiers: { toggle?: boolean; range?: boolean } = {}) => {
    setSelection((current) => applySelection(order, current, path, modifiers));
    setSelectedFolder(null);
    setFocusId(path);
    setPending((current) => (current?.kind === 'delete' || current?.kind === 'delete-folder' || current?.kind === 'rename-folder' || current?.kind === 'rename-group' ? null : current));
  };

  const selectFolder = (folder: string) => {
    setSelectedFolder(folder);
    setSelection({ selected: [], anchor: null });
    setFocusId(folderId(folder));
    setPending((current) => (current?.kind === 'delete' || current?.kind === 'copy' || current?.kind === 'merge' || current?.kind === 'rename-group' ? null : current));
  };

  const toggleFolderOpen = (folder: string) => {
    setClosedFolders((current) => {
      const next = new Set(current);
      if (next.has(folder)) next.delete(folder);
      else next.add(folder);
      return next;
    });
  };

  const onRowClick = (event: MouseEvent, path: string) => {
    select(path, { toggle: event.ctrlKey || event.metaKey, range: event.shiftKey });
  };

  const runChange = async (work: () => Promise<string[] | void>, done: string, doing = 'Working…') => {
    setBusyText(doing);
    setBusy(true);
    setError('');
    setStatus('');
    try {
      const nextSelected = await work();
      setPending(null);
      await refresh();
      if (nextSelected) { setSelection({ selected: nextSelected, anchor: nextSelected[0] ?? null }); setSelectedFolder(null); }
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

  const submitMerge = (event: FormEvent) => {
    event.preventDefault();
    if (pending?.kind !== 'merge' || selectedEntries.length < 2) return;
    const name = pending.name.trim();
    if (!name) return;
    const sources = selectedEntries.map((entry) => entry.path);
    void runChange(async () => {
      const result = await transport.groups.mergeGroups({ sources, new_group_name: name });
      return result.outputs.map((output) => output.path);
    }, `Merged ${sources.length} files into ${name}.`, `Merging ${sources.length} groups into ${name}…`);
  };

  const confirmDelete = () => {
    const targets = selectedEntries.filter((entry) => entry.revision);
    if (!targets.length) return;
    void runChange(async () => {
      for (const entry of targets) {
        await transport.groups.deleteGroup({ path: entry.path, expected_revision: entry.revision!, confirm_path: entry.path });
      }
      return [];
    }, `Deleted ${targets.map((entry) => entry.name).join(', ')}.`, `Deleting ${plural(targets.length, 'group')}…`);
  };

  const submitNewFolder = (event: FormEvent) => {
    event.preventDefault();
    const create = transport.groups.createFolder;
    if (pending?.kind !== 'new-folder' || !create) return;
    const name = pending.name.trim();
    if (!name) return;
    void runChange(async () => {
      const made = await create(name);
      setSelectedFolder(made.path);
    }, `Created the folder ${quoted(name)}. Copy groups into it with Copy to….`, `Creating ${quoted(name)}…`);
  };

  const submitRename = (event: FormEvent) => {
    event.preventDefault();
    const rename = transport.groups.renameFolder;
    if (pending?.kind !== 'rename-folder' || selectedFolder === null || !rename) return;
    const name = pending.name.trim();
    if (!name) return;
    const from = selectedFolder;
    void runChange(async () => {
      const renamed = await rename({ path: from, new_name: name });
      // Keep the analysis choice and exclusions with the renamed folder.
      if (inclusion) {
        // Keep each path's own separator (Windows paths use backslashes).
        const moved = (p: string) => (folderOf(p) === from ? `${renamed.path}${p.includes('\\') ? '\\' : '/'}${basename(p)}` : p);
        inclusion.setExcluded([...inclusion.excluded].map(moved));
        if (activeFolder === from) chooseFolder(renamed.path);
      }
      setSelectedFolder(renamed.path);
    }, `Renamed ${quoted(from)} to ${quoted(name)}.`, `Renaming ${quoted(from)}…`);
  };

  const submitRenameGroup = (event: FormEvent) => {
    event.preventDefault();
    const rename = transport.groups.renameGroup;
    const entry = selectedEntries.length === 1 ? selectedEntries[0] : undefined;
    if (pending?.kind !== 'rename-group' || !rename || !entry?.revision) return;
    const name = pending.name.trim();
    if (!name || name === entry.name) return;
    void runChange(async () => {
      const result = await rename({ path: entry.path, expected_revision: entry.revision!, new_name: name });
      const renamed = result.outputs[0]?.path ?? entry.path;
      // Keep the group's inclusion in analyses with its new file name.
      if (inclusion && renamed !== entry.path) inclusion.setExcluded(renamedExclusions(inclusion.excluded, entry.path, renamed));
      return [renamed];
    }, `Renamed ${entry.name} to ${name}.`, `Renaming ${entry.name}…`);
  };

  const submitAddColumn = (event: FormEvent) => {
    event.preventDefault();
    const add = transport.groups.addColumn;
    if (pending?.kind !== 'add-column' || !add) return;
    const { folder, value, confirmOverwrite } = pending;
    const column = pending.column.trim();
    if (columnNameProblem(entries, folder, column)) return;
    const existing = groupsWithColumn(entries, folder, column);
    // A column that already exists needs a second, explicit confirmation.
    if (existing.length > 0 && !confirmOverwrite) {
      setPending({ ...pending, confirmOverwrite: true });
      return;
    }
    setBusyText(`Adding ${column} to ${quoted(folder)}…`);
    setBusy(true);
    setError('');
    setStatus('');
    void (async () => {
      try {
        await add({ folder, column, value: value === '' ? null : value, overwrite: confirmOverwrite });
        setPending(null);
        await refresh();
        setStatus(`${confirmOverwrite ? 'Replaced' : 'Added'} the column ${column} in every group of ${quoted(folder)}.`);
        onDataChanged?.();
      } catch (cause) {
        const text = message(cause);
        // Another window may have added the column meanwhile: ask again.
        if (!confirmOverwrite && /already exists/.test(text)) setPending({ ...pending, confirmOverwrite: true });
        else setError(text);
        void refresh();
      } finally {
        setBusy(false);
      }
    })();
  };

  const confirmDeleteFolder = () => {
    const remove = transport.groups.deleteFolder;
    if (selectedFolder === null || !remove) return;
    const folder = selectedFolder;
    void runChange(async () => {
      await remove(folder);
      setSelectedFolder(null);
    }, `Deleted the empty folder ${quoted(folder)}.`, `Deleting ${quoted(folder)}…`);
  };

  const submitCopy = (event: FormEvent) => {
    event.preventDefault();
    const copy = transport.groups.copyGroups;
    if (pending?.kind !== 'copy' || !copy) return;
    const targets = selectedEntries.filter((entry) => entry.revision && !entry.error);
    const destination = pending.folder === NEW_FOLDER ? pending.newName.trim() : pending.folder;
    if (!targets.length || !destination) return;
    const verb = pending.move ? 'Moved' : 'Copied';
    void runChange(async () => {
      const result = await copy({
        sources: targets.map((entry) => ({ path: entry.path, expected_revision: entry.revision! })),
        destination_folder: destination,
        remove_sources: pending.move,
      });
      return result.outputs.map((output) => output.path);
    }, `${verb} ${plural(targets.length, 'group')} to ${quoted(destination)}.${destination === activeFolder ? '' : ' Click the folder’s circle to use it in analyses.'}`,
    `${pending.move ? 'Moving' : 'Copying'} ${plural(targets.length, 'group')} to ${quoted(destination)}…`);
  };

  // Export: the highlighted groups, or every group included in analyses
  // when nothing is highlighted. Rows follow project order.
  const exportPaths = selectedReady.length > 0 ? selectedReady : (includedPaths ?? readyPaths);
  const exportNames = entries.filter((entry) => exportPaths.includes(entry.path)).map((entry) => entry.name);
  const exportScope = selectedReady.length > 0 ? 'selected' : includedPaths ? 'included in analyses' : 'in the project';
  const canExport = !busy && !!transport.exports && exportPaths.length > 0;

  const submitExport = async (event: FormEvent) => {
    event.preventDefault();
    const exportsService = transport.exports;
    if (pending?.kind !== 'export' || !exportsService) return;
    const ordered = order.filter((path) => exportPaths.includes(path));
    const [path, ...extra] = ordered;
    if (!path) return;
    const format = pending.format;
    setBusyText(`Preparing ${format === 'xlsx' ? 'Excel workbook' : 'CSV'} for ${plural(ordered.length, 'group')}…`);
    setBusy(true);
    setError('');
    setStatus('');
    try {
      const result = await exportsService.measuredData({ path, extra_paths: extra, format });
      setBusyText('Saving export…');
      const saved = await downloadExportResult(result);
      setPending(null);
      setStatus(saved ? `Exported ${plural(ordered.length, 'group')} to ${result.file_name}.` : 'Export cancelled.');
    } catch (cause) {
      setError(`Export failed: ${message(cause)}`);
    } finally {
      setBusy(false);
    }
  };

  const selectedFolderNode = selectedFolder === null ? null : allFolders.find((f) => f.path === selectedFolder) ?? null;
  const renameTarget = selectedEntries.length === 1 && !selectedEntries[0]!.error && selectedEntries[0]!.revision ? selectedEntries[0]! : null;
  const canRenameGroup = !busy && !!transport.groups.renameGroup && renameTarget !== null;
  const canRenameFolder = !busy && supportsFolders && !!selectedFolderNode && selectedFolderNode.path !== '' && !!transport.groups.renameFolder;
  const startRename = () => {
    if (selectedFolderNode) setPending({ kind: 'rename-folder', name: selectedFolderNode.name });
    else if (renameTarget) setPending({ kind: 'rename-group', name: renameTarget.name });
  };
  // Add column… applies to one folder: the selected folder, or the folder of
  // the highlighted groups, or the folder analyses use.
  const columnFolder = selectedFolderNode ? selectedFolderNode.path : selectedFolders.length === 1 ? selectedFolders[0]! : selectedEntries.length === 0 ? activeFolder : null;
  const columnFolderGroups = columnFolder === null ? 0 : entries.filter((entry) => folderOf(entry.path) === columnFolder && entry.summary && !entry.error).length;
  const canAddColumn = !busy && !!transport.groups.addColumn && columnFolder !== null && columnFolderGroups > 0;
  const canCopy = !busy && supportsFolders && selectedEntries.length > 0 && selectedEntries.every((entry) => entry.revision && !entry.error);
  const canMerge = !busy && selectedEntries.length >= 2 && selectedEntries.every((entry) => !entry.error) && new Set(selectedEntries.map((e) => folderOf(e.path))).size === 1;
  const canDeleteGroups = !busy && selectedEntries.length > 0 && selectedEntries.every((entry) => entry.revision);
  const canDeleteFolder = !busy && !!transport.groups.deleteFolder && selectedFolderNode !== null && selectedFolderNode.entries.length === 0 && selectedFolderNode.path !== '';
  const canDelete = canDeleteGroups || canDeleteFolder;
  const startDelete = () => setPending(canDeleteGroups ? { kind: 'delete' } : { kind: 'delete-folder' });
  const copyChoices = allFolders.map((f) => f.path).filter((p) => p !== '');
  const startCopy = (move: boolean) => {
    const from = new Set(selectedEntries.map((e) => folderOf(e.path)));
    const first = copyChoices.find((p) => !from.has(p));
    setPending({ kind: 'copy', move, folder: first ?? NEW_FOLDER, newName: 'New grouping' });
  };

  const onTreeKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const index = visibleIds.indexOf(currentFocus);
    const onFolder = isFolderId(currentFocus);
    const folder = onFolder ? folderFromId(currentFocus) : folderOf(currentFocus);
    const move = (to: number) => {
      const id = visibleIds[Math.max(0, Math.min(visibleIds.length - 1, to))];
      if (id === undefined) return;
      focusItem(id);
      if (event.shiftKey && !isFolderId(id)) select(id, { range: true });
    };
    switch (event.key) {
      case 'ArrowDown': event.preventDefault(); move(index + 1); break;
      case 'ArrowUp': event.preventDefault(); move(index - 1); break;
      case 'Home': event.preventDefault(); move(0); break;
      case 'End': event.preventDefault(); move(visibleIds.length - 1); break;
      case 'ArrowRight': {
        event.preventDefault();
        if (!onFolder) break;
        const node = folders.find((f) => f.path === folder);
        if (closedFolders.has(folder)) toggleFolderOpen(folder);
        else if (node?.entries.length) focusItem(node.entries[0]!.path);
        break;
      }
      case 'ArrowLeft':
        event.preventDefault();
        if (onFolder) { if (!closedFolders.has(folder)) toggleFolderOpen(folder); }
        else focusItem(folderId(folder));
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        // Space on a folder makes it the analysis folder; Enter selects it.
        if (onFolder) { if (event.key === ' ' && inclusion) chooseFolder(folder); else selectFolder(folder); }
        // Space ticks the focused group's box (WAI-ARIA checkbox tree);
        // Ctrl/Cmd+Space and Enter change the highlight selection.
        else if (event.key === ' ' && includedSet && !event.ctrlKey && !event.metaKey && !event.shiftKey) toggleIncluded(currentFocus);
        else select(currentFocus, { toggle: event.key === ' ' || event.ctrlKey || event.metaKey, range: event.shiftKey });
        break;
      case 'a':
        if (event.ctrlKey || event.metaKey) {
          event.preventDefault();
          const inThis = folders.find((f) => f.path === folder)?.entries.map((e) => e.path) ?? order;
          setSelection({ selected: inThis, anchor: inThis[0] ?? null });
          setSelectedFolder(null);
        }
        break;
      case 'Delete':
        if (canDelete) { event.preventDefault(); startDelete(); }
        break;
      case 'F2':
        if (onFolder && supportsFolders && folder !== '') { event.preventDefault(); selectFolder(folder); setPending({ kind: 'rename-folder', name: folderLabel(folder) }); }
        else if (!onFolder && canRenameGroup && renameTarget?.path === currentFocus) { event.preventDefault(); setPending({ kind: 'rename-group', name: renameTarget.name }); }
        break;
      case 'Escape':
        setSelection({ selected: [], anchor: null });
        setSelectedFolder(null);
        setPending(null);
        break;
    }
  };

  // Every measured column across the project's files, for the Analysis setup.
  const projectElements = useMemo(() => unionElements(entries.map((entry) => entry.summary?.elemental_columns ?? [])), [entries]);

  const setRef = (id: string) => (element: HTMLLIElement | null) => {
    if (element) itemRefs.current.set(id, element);
    else itemRefs.current.delete(id);
  };

  const escapeCancels = (event: KeyboardEvent<HTMLInputElement>) => { if (event.key === 'Escape') setPending(null); };

  return (
    <div className="file-manager">
      <header className="fm-header">
        <span className="fm-project" title={projectName || undefined}>
          <FolderIcon open />
          <span className="fm-project-name" aria-label="Current project">{projectName || 'Project'}</span>
          {onRevealProject && (
            <button type="button" className="fm-reveal" onClick={onRevealProject} title={revealLabel} aria-label={revealLabel}>
              <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false"><path d="M9 2h5v5M14 2 7.5 8.5M12 9.5V13a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3.5" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" /></svg>
            </button>
          )}
        </span>
        <div className="fm-toolbar" role="toolbar" aria-label="File actions">
          <span className="fm-tool-group">
            <button type="button" className="fm-tool fm-tool-primary" disabled={busy} onClick={() => setImportOpen(true)}>Import…</button>
            {transport.exports && (
              <button
                type="button"
                className="fm-tool"
                disabled={!canExport}
                aria-pressed={pending?.kind === 'export'}
                title="Export the highlighted groups (or all groups in analyses) to Excel or CSV"
                onClick={() => setPending({ kind: 'export', format: 'xlsx' })}
              >
                Export…
              </button>
            )}
            <button type="button" className="fm-tool fm-tool-icon" disabled={busy || loading} onClick={() => { setStatus(''); void refresh(); }} aria-label="Refresh" title="Refresh">⟳</button>
          </span>
          {supportsFolders && (
            <span className="fm-tool-group">
              <button type="button" className="fm-tool" disabled={busy || !transport.groups.createFolder} aria-pressed={pending?.kind === 'new-folder'} onClick={() => setPending({ kind: 'new-folder', name: 'New grouping' })} title="Create an empty folder for a new grouping of the samples">New folder…</button>
              <button type="button" className="fm-tool" disabled={!(selectedFolderNode ? canRenameFolder : canRenameGroup)} aria-pressed={pending?.kind === 'rename-folder' || pending?.kind === 'rename-group'} onClick={startRename} title="Rename the selected folder or the one highlighted group (F2)">Rename…</button>
            </span>
          )}
          {transport.groups.addColumn && (
            <span className="fm-tool-group">
              <button type="button" className="fm-tool" disabled={!canAddColumn} aria-pressed={pending?.kind === 'add-column'} onClick={() => columnFolder !== null && setPending({ kind: 'add-column', folder: columnFolder, column: '', value: '', confirmOverwrite: false })} title="Add a descriptive column with one value to every group in the folder">Add column…</button>
            </span>
          )}
          <span className="fm-tool-group">
            {supportsFolders && (
              <>
                <button type="button" className="fm-tool" disabled={!canCopy} aria-pressed={pending?.kind === 'copy' && !pending.move} onClick={() => startCopy(false)} title="Copy the highlighted groups into another folder">Copy to…</button>
                <button type="button" className="fm-tool" disabled={!canCopy} aria-pressed={pending?.kind === 'copy' && pending.move} onClick={() => startCopy(true)} title="Move the highlighted groups into another folder">Move to…</button>
              </>
            )}
            <button type="button" className="fm-tool" disabled={!canMerge} aria-pressed={pending?.kind === 'merge'} onClick={() => setPending({ kind: 'merge', name: 'Merged' })}>Merge</button>
            <button type="button" className="fm-tool fm-tool-danger" disabled={!canDelete} aria-pressed={pending?.kind === 'delete' || pending?.kind === 'delete-folder'} onClick={startDelete}>Delete</button>
          </span>
        </div>
      </header>

      {pending?.kind === 'new-folder' && (
        <form className="fm-inline" onSubmit={submitNewFolder} aria-label="New folder">
          <label>Name of the new folder
            <input autoFocus value={pending.name} disabled={busy} onChange={(event) => setPending({ kind: 'new-folder', name: event.target.value })} onKeyDown={escapeCancels} />
          </label>
          <p className="fm-hint">A folder is one grouping of the samples. Copy groups into it, then reassign samples there without changing the original grouping.</p>
          <div className="fm-inline-actions">
            <button type="submit" disabled={busy || !pending.name.trim()}><BusyLabel busy={busy}>Create folder</BusyLabel></button>
            <button type="button" disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </form>
      )}
      {pending?.kind === 'rename-folder' && selectedFolderNode && (
        <form className="fm-inline" onSubmit={submitRename} aria-label="Rename folder">
          <label>New name for {quoted(selectedFolderNode.path)}
            <input autoFocus value={pending.name} disabled={busy} onChange={(event) => setPending({ kind: 'rename-folder', name: event.target.value })} onKeyDown={escapeCancels} />
          </label>
          <div className="fm-inline-actions">
            <button type="submit" disabled={busy || !pending.name.trim() || pending.name.trim() === selectedFolderNode.name}><BusyLabel busy={busy}>Rename</BusyLabel></button>
            <button type="button" disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </form>
      )}
      {pending?.kind === 'rename-group' && renameTarget && (
        <form className="fm-inline" onSubmit={submitRenameGroup} aria-label="Rename group">
          <label>New name for {renameTarget.name}
            <input autoFocus value={pending.name} disabled={busy} onChange={(event) => setPending({ kind: 'rename-group', name: event.target.value })} onKeyDown={escapeCancels} />
          </label>
          <p className="fm-hint">The group file is renamed too. Its samples and their identities stay the same.</p>
          <div className="fm-inline-actions">
            <button type="submit" disabled={busy || !pending.name.trim() || pending.name.trim() === renameTarget.name}><BusyLabel busy={busy}>Rename</BusyLabel></button>
            <button type="button" disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </form>
      )}
      {pending?.kind === 'add-column' && (
        <AddColumnForm
          pending={pending}
          problem={columnNameProblem(entries, pending.folder, pending.column)}
          existing={groupsWithColumn(entries, pending.folder, pending.column).map((entry) => entry.name)}
          groupCount={entries.filter((entry) => folderOf(entry.path) === pending.folder && entry.summary && !entry.error).length}
          busy={busy}
          onChange={(next) => setPending({ ...pending, ...next, confirmOverwrite: false })}
          onSubmit={submitAddColumn}
          onBack={() => setPending({ ...pending, confirmOverwrite: false })}
          onCancel={() => setPending(null)}
        />
      )}
      {pending?.kind === 'copy' && selectedEntries.length > 0 && (
        <form className="fm-inline" onSubmit={submitCopy} aria-label={pending.move ? 'Move groups' : 'Copy groups'}>
          <p className="fm-export-what">{pending.move ? 'Move' : 'Copy'} {selectedEntries.map((entry) => entry.name).join(', ')} to</p>
          <label>Folder
            <select value={pending.folder} disabled={busy} onChange={(event) => setPending({ ...pending, folder: event.target.value })}>
              {copyChoices.map((path) => (
                <option key={path} value={path}>{folderLabel(path)}{selectedFolders.includes(path) ? ' (current folder)' : ''}</option>
              ))}
              <option value={NEW_FOLDER}>New folder…</option>
            </select>
          </label>
          {pending.folder === NEW_FOLDER && (
            <label>New folder name
              <input autoFocus value={pending.newName} disabled={busy} onChange={(event) => setPending({ ...pending, newName: event.target.value })} onKeyDown={escapeCancels} />
            </label>
          )}
          <p className="fm-hint">{pending.move ? 'The groups leave their current folder.' : 'The originals stay where they are.'} A folder holds each sample once, so copying into a folder that already has these samples is refused.</p>
          <div className="fm-inline-actions">
            <button type="submit" disabled={busy || (pending.folder === NEW_FOLDER && !pending.newName.trim())}><BusyLabel busy={busy}>{pending.move ? 'Move' : 'Copy'}</BusyLabel></button>
            <button type="button" disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </form>
      )}
      {pending?.kind === 'merge' && selectedEntries.length >= 2 && (
        <form className="fm-inline" onSubmit={submitMerge} aria-label="Merge files">
          <label>Merge {selectedEntries.map((entry) => entry.name).join(', ')} into
            <input autoFocus value={pending.name} disabled={busy} onChange={(event) => setPending({ kind: 'merge', name: event.target.value })} onKeyDown={escapeCancels} />
          </label>
          <div className="fm-inline-actions">
            <button type="submit" disabled={busy || !pending.name.trim()}><BusyLabel busy={busy}>Merge</BusyLabel></button>
            <button type="button" disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </form>
      )}
      {pending?.kind === 'delete' && selectedEntries.length > 0 && (
        <div className="fm-inline fm-inline-danger" role="group" aria-label="Confirm delete">
          <p>Delete {selectedEntries.map((entry) => entry.name).join(', ')}? This removes {selectedEntries.length === 1 ? 'the group file' : `${selectedEntries.length} group files`} from the project.</p>
          <div className="fm-inline-actions">
            <button type="button" className="fm-danger" disabled={busy} onClick={confirmDelete}><BusyLabel busy={busy}>Confirm</BusyLabel></button>
            <button type="button" autoFocus disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </div>
      )}
      {pending?.kind === 'delete-folder' && selectedFolderNode && (
        <div className="fm-inline fm-inline-danger" role="group" aria-label="Confirm folder delete">
          <p>Delete the empty folder {quoted(selectedFolderNode.path)}?</p>
          <div className="fm-inline-actions">
            <button type="button" className="fm-danger" disabled={busy} onClick={confirmDeleteFolder}><BusyLabel busy={busy}>Delete folder</BusyLabel></button>
            <button type="button" autoFocus disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </div>
      )}
      {pending?.kind === 'export' && (
        <form className="fm-inline" onSubmit={(event) => void submitExport(event)} aria-label="Export groups">
          <p className="fm-export-what">
            Export {plural(exportNames.length, 'group')} {exportScope}: {exportNames.join(', ')}
          </p>
          <fieldset className="fm-export-format" disabled={busy}>
            <legend>Format</legend>
            <label>
              <input type="radio" name="fm-export-format" checked={pending.format === 'xlsx'} onChange={() => setPending({ kind: 'export', format: 'xlsx' })} />{' '}
              Excel workbook (.xlsx){exportNames.length > 1 ? ': all rows plus one sheet per group' : ''}
            </label>
            <label>
              <input type="radio" name="fm-export-format" checked={pending.format === 'csv'} onChange={() => setPending({ kind: 'export', format: 'csv' })} />{' '}
              CSV (.csv){exportNames.length > 1 ? ': all rows with a Group column' : ''}
            </label>
          </fieldset>
          <div className="fm-inline-actions">
            <button type="submit" disabled={busy || exportPaths.length === 0}><BusyLabel busy={busy} busyText="Exporting…">Export</BusyLabel></button>
            <button type="button" disabled={busy} onClick={() => setPending(null)}>Cancel</button>
          </div>
        </form>
      )}

      {includedSet && activeReady.length > 0 && (
        <div className="fm-include-bar" role="group" aria-label="Groups in analyses">
          <span className="fm-include-count" aria-live="polite">
            In analyses: {includedSet.size} of {activeReady.length}
            {folders.length > 1 && activeFolder !== null && <> in {quoted(activeFolder)}</>}
          </span>
          <button type="button" className="fm-tool" disabled={includedSet.size === activeReady.length} onClick={() => setIncluded(activeReady)}>All</button>
          <button type="button" className="fm-tool" disabled={includedSet.size === 0} onClick={() => setIncluded([])}>None</button>
          <button type="button" className="fm-tool" title="Include the groups that are left out, and leave out the included ones" onClick={() => setIncluded(invertedAmong(activeReady, includedSet))}>Invert</button>
          <button
            type="button"
            className="fm-tool"
            disabled={selectedReady.length === 0 || selectedFolders.length !== 1}
            title="Include only the highlighted groups (Ctrl/Cmd-click or Shift-click to highlight several)"
            onClick={() => setIncluded(selectedReady, selectedFolders[0] ?? activeFolder)}
          >
            Only selected
          </button>
          <label className="fm-include-filter">Show
            <select value={treeFilter} onChange={(event) => { const next = event.target.value as InclusionFilter; setTreeFilter(next); setSelection({ selected: [], anchor: null }); }}>
              <option value="all">All groups</option>
              <option value="included">Included</option>
              <option value="excluded">Left out</option>
            </select>
          </label>
        </div>
      )}

      <GroupFileTree
        entries={entries}
        folders={folders}
        selected={selection.selected}
        selectedFolder={selectedFolder}
        activeFolder={inclusion ? activeFolder : undefined}
        included={includedSet}
        onToggleInclude={toggleIncluded}
        onIncludeOnly={includeOnly}
        closedFolders={closedFolders}
        focusId={currentFocus}
        loading={loading}
        filtered={filtering}
        onToggleFolder={toggleFolderOpen}
        onSelectFolder={selectFolder}
        onUseFolder={inclusion?.setActiveFolder ? (folder) => { chooseFolder(folder); setStatus(`Analyses now use the folder ${quoted(folder)}.`); } : undefined}
        onRowClick={onRowClick}
        onCheck={(path) => select(path, { toggle: true })}
        onKeyDown={onTreeKeyDown}
        onFocusItem={setFocusId}
        setRef={setRef}
      />

      <div className="fm-messages">
        {busy && busyText && <BusyNote>{busyText}</BusyNote>}
        {!busy && status && <p role="status">{status}</p>}
        {error && <p role="alert">{error}</p>}
      </div>

      <AnalysisSetupPanel
        elements={projectElements}
        groups={transport.groups}
        ordination={transport.ordination}
        dataKey={entries.map((entry) => `${entry.path}@${entry.revision ?? ''}`).join('|')}
      />

      {selectedFolderNode ? (
        <FolderDetails folder={selectedFolderNode} inAnalyses={activeFolder === selectedFolderNode.path} />
      ) : (
        <GroupDetails entries={selectedEntries} />
      )}

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
  /** Folders to show (default: the folders of `entries`). */
  readonly folders?: readonly FolderNode[];
  readonly selected: readonly string[];
  readonly selectedFolder?: string | null;
  /** Folder analyses use; `undefined` hides the folder switch. */
  readonly activeFolder?: string | null;
  /** All folders open/closed (tests); `closedFolders` wins when given. */
  readonly folderOpen?: boolean;
  readonly closedFolders?: ReadonlySet<string>;
  readonly focusId?: string;
  readonly loading?: boolean;
  /** The rows are filtered by inclusion (empty folders say so). */
  readonly filtered?: boolean;
  readonly onToggleFolder?: (folder: string) => void;
  readonly onSelectFolder?: (folder: string) => void;
  readonly onUseFolder?: (folder: string) => void;
  readonly onRowClick?: (event: MouseEvent, path: string) => void;
  readonly onCheck?: (path: string) => void;
  /**
   * Groups included in analyses. When given, each row's box shows inclusion
   * (click toggles, double-click keeps only that group) instead of selection.
   */
  readonly included?: ReadonlySet<string> | null;
  readonly onToggleInclude?: (path: string) => void;
  readonly onIncludeOnly?: (path: string) => void;
  readonly onKeyDown?: (event: KeyboardEvent<HTMLUListElement>) => void;
  readonly onFocusItem?: (id: string) => void;
  readonly setRef?: (id: string) => (element: HTMLLIElement | null) => void;
}

/** Presentational tree: one collapsible row per group folder, one row per group file. */
export function GroupFileTree({
  entries,
  folders: givenFolders,
  selected,
  selectedFolder = null,
  activeFolder,
  folderOpen = true,
  closedFolders,
  focusId,
  loading = false,
  filtered = false,
  onToggleFolder,
  onSelectFolder,
  onUseFolder,
  onRowClick,
  onCheck,
  included = null,
  onToggleInclude,
  onIncludeOnly,
  onKeyDown,
  onFocusItem,
  setRef,
}: GroupFileTreeProps): ReactElement {
  const folders = givenFolders ?? folderNodes(entries);
  const isOpen = (folder: string) => (closedFolders ? !closedFolders.has(folder) : folderOpen);
  const ids = folders.flatMap((folder) => [folderId(folder.path), ...(isOpen(folder.path) ? folder.entries.map((e) => e.path) : [])]);
  const rovingFocus = focusId !== undefined && ids.includes(focusId) ? focusId : (ids[0] ?? '');
  const showSwitch = activeFolder !== undefined && folders.length > 1;
  return (
    <ul className="fm-tree" role="tree" aria-label="Project files" aria-multiselectable="true" aria-busy={loading} onKeyDown={onKeyDown}>
      {!folders.length && (
        <li role="none" className="fm-empty">{loading ? <><Spinner />Loading…</> : 'No group files yet. Use Import… to add data.'}</li>
      )}
      {folders.map((folder) => {
        const open = isOpen(folder.path);
        const id = folderId(folder.path);
        const active = activeFolder === folder.path;
        return (
          <li
            key={id}
            role="treeitem"
            aria-expanded={open}
            aria-level={1}
            aria-selected={selectedFolder === folder.path}
            aria-label={`${folder.name}, ${plural(folder.entries.length, 'file')}${active && showSwitch ? ', used in analyses' : ''}`}
            tabIndex={rovingFocus === id ? 0 : -1}
            ref={setRef?.(id)}
            onFocus={(event) => { if (event.target === event.currentTarget) onFocusItem?.(id); }}
            className={`fm-item fm-item-folder${selectedFolder === folder.path ? ' is-selected' : ''}${active && showSwitch ? ' is-active' : ''}${!active && showSwitch ? ' is-inactive' : ''}`}
          >
            <div className="fm-row" onClick={() => (onSelectFolder ? onSelectFolder(folder.path) : onToggleFolder?.(folder.path))}>
              <span className="fm-twisty" aria-hidden="true" onClick={(event) => { event.stopPropagation(); onToggleFolder?.(folder.path); }}>{open ? '▾' : '▸'}</span>
              {showSwitch && (
                <span
                  className={`fm-radio${active ? ' is-on' : ''}`}
                  aria-hidden="true"
                  title={active ? 'Analyses use this folder' : 'Use this folder in analyses'}
                  onClick={(event) => { event.stopPropagation(); onUseFolder?.(folder.path); }}
                />
              )}
              <FolderIcon open={open} />
              <span className="fm-name">{folder.name}</span>
              {active && showSwitch && <span className="fm-badge">In analyses</span>}
              <span className="fm-meta">{folder.entries.length}</span>
            </div>
            {open && (
              <ul role="group" className="fm-group">
                {!folder.entries.length && <li role="none" className="fm-empty">{filtered ? 'No groups match the filter.' : 'Empty folder. Copy groups into it with Copy to….'}</li>}
                {folder.entries.map((entry) => {
                  const isSelected = selected.includes(entry.path);
                  const rows = entry.summary?.row_count;
                  const includable = Boolean(entry.summary && !entry.error);
                  const inActive = activeFolder === undefined || activeFolder === null || folder.path === activeFolder;
                  const isIncluded = included ? inActive && included.has(entry.path) : undefined;
                  return (
                    <li
                      key={entry.path}
                      role="treeitem"
                      aria-level={2}
                      aria-selected={isSelected}
                      aria-checked={included && includable ? isIncluded : undefined}
                      aria-invalid={entry.error ? true : undefined}
                      aria-label={`${entry.name}${rows === undefined ? '' : `, ${plural(rows, 'row')}`}${entry.error ? `, needs attention: ${entry.error}` : ''}`}
                      tabIndex={rovingFocus === entry.path ? 0 : -1}
                      ref={setRef?.(entry.path)}
                      title={entry.error ? `Needs attention: ${entry.error}` : entry.path}
                      className={`fm-item fm-item-file${isSelected ? ' is-selected' : ''}${entry.error ? ' is-invalid' : ''}${included && includable && !isIncluded ? ' is-excluded' : ''}`}
                      onFocus={(event) => { if (event.target === event.currentTarget) onFocusItem?.(entry.path); }}
                      data-path={entry.path}
                    >
                      <div className="fm-row" onClick={(event) => onRowClick?.(event, entry.path)}>
                        {included ? (
                          <span
                            className={`fm-check fm-include${isIncluded ? ' is-on' : ''}${includable ? '' : ' is-disabled'}${inActive ? '' : ' is-other-folder'}`}
                            aria-hidden="true"
                            title={!includable ? 'Fix this file to include it in analyses' : inActive ? 'Include in analyses (double-click: only this group)' : 'Use this folder in analyses and include this group'}
                            onClick={(event) => { event.stopPropagation(); if (includable) onToggleInclude?.(entry.path); }}
                            onDoubleClick={(event) => { event.stopPropagation(); if (includable) onIncludeOnly?.(entry.path); }}
                          >
                            {isIncluded ? '✓' : ''}
                          </span>
                        ) : (
                          <span
                            className="fm-check"
                            aria-hidden="true"
                            onClick={(event) => { event.stopPropagation(); onCheck?.(entry.path); }}
                          >
                            {isSelected ? '✓' : ''}
                          </span>
                        )}
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
        );
      })}
    </ul>
  );
}

export interface AddColumnFormProps {
  readonly pending: { readonly folder: string; readonly column: string; readonly value: string; readonly confirmOverwrite: boolean };
  /** Why the name is refused ('' when fine). */
  readonly problem: string;
  /** Groups that already have the column. */
  readonly existing: readonly string[];
  readonly groupCount: number;
  readonly busy?: boolean;
  readonly onChange: (next: { column?: string; value?: string }) => void;
  readonly onSubmit: (event: FormEvent) => void;
  readonly onBack: () => void;
  readonly onCancel: () => void;
}

/**
 * Add a descriptive column with one value to every group in a folder. An
 * existing column needs a second confirmation before its values are
 * replaced (legacy "Add column" overwrite prompt).
 */
export function AddColumnForm({ pending, problem, existing, groupCount, busy = false, onChange, onSubmit, onBack, onCancel }: AddColumnFormProps): ReactElement {
  const column = pending.column.trim();
  const shownProblem = column ? problem : '';
  if (pending.confirmOverwrite) {
    return (
      <form className="fm-inline fm-inline-danger" onSubmit={onSubmit} aria-label="Confirm column overwrite">
        <p role="alert">The column {column} already exists in {existing.length === 1 ? existing[0] : `${existing.length} groups (${existing.join(', ')})`}. Replace its values with “{pending.value || '(empty)'}” in every group of {quoted(pending.folder)}?</p>
        <div className="fm-inline-actions">
          <button type="submit" className="fm-danger" disabled={busy}><BusyLabel busy={busy}>Replace values</BusyLabel></button>
          <button type="button" autoFocus disabled={busy} onClick={onBack}>Back</button>
          <button type="button" disabled={busy} onClick={onCancel}>Cancel</button>
        </div>
      </form>
    );
  }
  return (
    <form className="fm-inline" onSubmit={onSubmit} aria-label="Add column">
      <p className="fm-export-what">Add a descriptive column to the {plural(groupCount, 'group')} in {quoted(pending.folder)}</p>
      <label>Column name
        <input autoFocus value={pending.column} disabled={busy} onChange={(event) => onChange({ column: event.target.value })} onKeyDown={(event) => { if (event.key === 'Escape') onCancel(); }} />
      </label>
      <label>Value for every row
        <input value={pending.value} disabled={busy} placeholder="(empty)" onChange={(event) => onChange({ value: event.target.value })} onKeyDown={(event) => { if (event.key === 'Escape') onCancel(); }} />
      </label>
      {shownProblem && <p className="fm-hint" role="alert">{shownProblem}</p>}
      {!shownProblem && existing.length > 0 && <p className="fm-hint">{column} already exists in {plural(existing.length, 'group')}; you will be asked before its values are replaced.</p>}
      <div className="fm-inline-actions">
        <button type="submit" disabled={busy || !column || Boolean(problem)}><BusyLabel busy={busy}>Add column</BusyLabel></button>
        <button type="button" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}

/** Details for a selected folder. */
export function FolderDetails({ folder, inAnalyses }: { folder: FolderNode; inAnalyses: boolean }): ReactElement {
  const rows = folder.entries.reduce((sum, entry) => sum + (entry.summary?.row_count ?? 0), 0);
  return (
    <section className="fm-details" aria-label="Folder details">
      <h3>{folder.name}</h3>
      <dl>
        <dt>Path</dt><dd className="fm-path">{folder.path || '(project root)'}</dd>
        <dt>Groups</dt><dd>{folder.entries.length}</dd>
        <dt>Rows</dt><dd>{rows}</dd>
        <dt>Analyses</dt><dd>{inAnalyses ? 'Use this folder' : 'Not used (click its circle to use it)'}</dd>
      </dl>
    </section>
  );
}

/** Compact details for the current selection. */
export function GroupDetails({ entries }: { entries: readonly GroupFileEntry[] }): ReactElement {
  if (!entries.length) {
    return <section className="fm-details" aria-label="File details"><p className="fm-details-hint">Select a file to see its details. Ctrl/Cmd-click or Shift-click to select several. Tick a group&apos;s box to include it in analyses.</p></section>;
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
