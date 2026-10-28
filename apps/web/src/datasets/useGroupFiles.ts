/**
 * Shared Dataset state for analysis pages: scan the project's group files,
 * select all ready groups by default, load the selected files' rows, and
 * expose them combined (see `combineGroupFiles`). Selected files are always
 * kept in project order so combined rows line up with backend results.
 *
 * Inside a `GroupInclusionProvider` the selection is the project-wide set of
 * groups included in analyses (also edited from the Data Manager), so it is
 * the same on every page; otherwise each page keeps its own selection.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { GroupCandidate, GroupsService } from '@archaeodash/client';
import { combineGroupFiles, type CombinedRows, type LoadedGroupFile } from '../visualize/combine.ts';
import { includedAmong, useGroupInclusion } from './GroupInclusion.tsx';
import { activeFolderAmong, exclusionWithin, folderOf } from './folders.ts';
import { useDataCache } from './DataCache.tsx';
import { onGroupsChanged } from '../data-manager/events.ts';
import { sharedScan } from './groupUpdates.ts';

export type GroupFilesLoad =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'loaded' }
  | { kind: 'error'; message: string };

export interface GroupFiles {
  readonly candidates: GroupCandidate[];
  /** Ready group files in project order. */
  readonly ready: GroupCandidate[];
  /** Picker options: path + group name. */
  readonly options: { path: string; name: string }[];
  readonly selectedPaths: string[];
  readonly setSelectedPaths: (paths: string[]) => void;
  /** Selected paths in project order. */
  readonly orderedPaths: string[];
  readonly files: LoadedGroupFile[];
  readonly combined: CombinedRows;
  readonly load: GroupFilesLoad;
  /** Re-read the selected files' rows. */
  readonly reload: () => void;
  /**
   * Rescan the project. `keep` maps the new list of ready paths to the
   * selection; by default the current selection survives (minus vanished
   * files) and an empty selection becomes all groups.
   */
  readonly rescan: (keep?: (allReady: string[]) => string[]) => Promise<void>;
}

export function useGroupFiles(groups: GroupsService): GroupFiles {
  const [candidates, setCandidates] = useState<GroupCandidate[]>([]);
  const [localSelected, setLocalSelected] = useState<string[]>([]);
  const inclusion = useGroupInclusion();
  const inclusionRef = useRef(inclusion);
  inclusionRef.current = inclusion;
  const [files, setFiles] = useState<LoadedGroupFile[]>([]);
  const [load, setLoad] = useState<GroupFilesLoad>({ kind: 'idle' });
  const [token, setToken] = useState(0);
  const cache = useDataCache();
  // Set by `reload()` so the next load skips cached rows.
  const forceReload = useRef(false);

  // With shared inclusion, pages work on the active folder only (a folder
  // is one grouping of the samples; other folders may hold the same ones).
  const activeFolder = inclusion?.activeFolder;
  const ready = useMemo(() => {
    const all = candidates.filter((c) => c.ready && c.group !== null && c.group !== undefined);
    if (!inclusion) return all;
    const folder = activeFolderAmong(all.map((c) => c.path), activeFolder);
    return folder === null ? all : all.filter((c) => folderOf(c.path) === folder);
    // `inclusion` only matters through `activeFolder` here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [candidates, activeFolder, inclusion === null]);
  const options = useMemo(
    () => ready.map((c) => ({ path: c.path, name: c.group?.group_name ?? c.path })),
    [ready],
  );

  const readyPaths = useMemo(() => ready.map((c) => c.path), [ready]);
  const selectedPaths = useMemo(
    () => (inclusion ? includedAmong(readyPaths, inclusion.excluded) : localSelected),
    [inclusion, readyPaths, localSelected],
  );
  const setSelectedPaths = useCallback(
    (paths: string[]) => {
      if (inclusion) inclusion.setExcluded(exclusionWithin(inclusion.excluded, readyPaths, paths));
      else setLocalSelected(paths);
    },
    [inclusion, readyPaths],
  );

  const rescan = useCallback(
    async (keep?: (allReady: string[]) => string[], afterNotice = false) => {
      // A rescan answering a change notice shares one scan with the other
      // pages and the Data Manager; any other rescan scans for itself.
      const list = await (afterNotice ? sharedScan(groups) : groups.scan());
      setCandidates(list);
      const allFolders = list.filter((c) => c.ready && c.group).map((c) => c.path);
      const shared = inclusionRef.current;
      if (shared) {
        // Shared inclusion: new groups are included automatically; `keep`
        // (e.g. after moving rows into a new group) sets it explicitly,
        // within the active folder only.
        const folder = activeFolderAmong(allFolders, shared.activeFolder);
        const scope = folder === null ? allFolders : allFolders.filter((p) => folderOf(p) === folder);
        if (keep) shared.setExcluded(exclusionWithin(shared.excluded, scope, keep(scope)));
        return;
      }
      const all = allFolders;
      setLocalSelected((current) => {
        if (keep) return keep(all);
        const surviving = current.filter((p) => all.includes(p));
        return surviving.length > 0 ? surviving : all;
      });
    },
    [groups],
  );

  // Every ready group is selected by default.
  useEffect(() => {
    void rescan().catch(() => setCandidates([]));
  }, [rescan]);
  // Another page changed group files (e.g. moved samples): pick up the new
  // revisions. Pages stay mounted in the background, and without this they
  // would keep showing the old rows. Only changed files are re-read, and
  // results keyed by row content (rowKeys.ts) are not recomputed.
  useEffect(() => onGroupsChanged(() => rescan(undefined, true).catch(() => undefined)), [rescan]);

  const orderedPaths = useMemo(
    () => ready.map((c) => c.path).filter((p) => selectedPaths.includes(p)),
    [ready, selectedPaths],
  );
  // Stable key so a rescan that changes nothing does not refetch rows; it
  // includes each file's revision, so a saved change reloads that file.
  const selectionKey = orderedPaths
    .map((p) => `${p}\u0001${ready.find((c) => c.path === p)?.group?.revision_id ?? ''}`)
    .join('\u0000');

  useEffect(() => {
    const entries = selectionKey === '' ? [] : selectionKey.split('\u0000').map((e) => e.split('\u0001') as [string, string]);
    const paths = entries.map(([path]) => path);
    const revisionOf = new Map(entries);
    if (forceReload.current) {
      cache?.dropRows(paths);
      forceReload.current = false;
    }
    if (paths.length === 0) {
      setFiles([]);
      setLoad({ kind: 'idle' });
      return;
    }
    let cancelled = false;
    setLoad({ kind: 'loading' });
    Promise.all(
      paths.map(async (path) => {
        const revision = revisionOf.get(path) || undefined;
        const data = cache ? await cache.rowsFor(path, revision, () => groups.rows(path)) : await groups.rows(path);
        return {
          path,
          groupName: ready.find((c) => c.path === path)?.group?.group_name ?? path,
          data,
        };
      }),
    )
      .then((loaded) => {
        if (cancelled) return;
        setFiles(loaded);
        setLoad({ kind: 'loaded' });
      })
      .catch((err: unknown) => {
        if (!cancelled) setLoad({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      });
    return () => {
      cancelled = true;
    };
    // `ready` only supplies group names; the selection key drives reloads.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groups, selectionKey, token, cache]);

  const combined = useMemo(() => combineGroupFiles(files), [files]);
  const reload = useCallback(() => {
    forceReload.current = true;
    setToken((t) => t + 1);
  }, []);

  return {
    candidates,
    ready,
    options,
    selectedPaths,
    setSelectedPaths,
    orderedPaths,
    files,
    combined,
    load,
    reload,
    rescan,
  };
}
