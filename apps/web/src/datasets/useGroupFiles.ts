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
import { exclusionFor, includedAmong, useGroupInclusion } from './GroupInclusion.tsx';

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

  const ready = useMemo(
    () => candidates.filter((c) => c.ready && c.group !== null && c.group !== undefined),
    [candidates],
  );
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
      if (inclusion) inclusion.setExcluded(exclusionFor(readyPaths, paths));
      else setLocalSelected(paths);
    },
    [inclusion, readyPaths],
  );

  const rescan = useCallback(
    async (keep?: (allReady: string[]) => string[]) => {
      const list = await groups.scan();
      setCandidates(list);
      const all = list.filter((c) => c.ready && c.group).map((c) => c.path);
      const shared = inclusionRef.current;
      if (shared) {
        // Shared inclusion: new groups are included automatically; `keep`
        // (e.g. after moving rows into a new group) sets it explicitly.
        if (keep) shared.setExcluded(exclusionFor(all, keep(all)));
        return;
      }
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

  const orderedPaths = useMemo(
    () => ready.map((c) => c.path).filter((p) => selectedPaths.includes(p)),
    [ready, selectedPaths],
  );
  // Stable key so a rescan that changes nothing does not refetch rows.
  const selectionKey = orderedPaths.join('\u0000');

  useEffect(() => {
    const paths = selectionKey === '' ? [] : selectionKey.split('\u0000');
    if (paths.length === 0) {
      setFiles([]);
      setLoad({ kind: 'idle' });
      return;
    }
    let cancelled = false;
    setLoad({ kind: 'loading' });
    Promise.all(
      paths.map(async (path) => {
        const data = await groups.rows(path);
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
  }, [groups, selectionKey, token]);

  const combined = useMemo(() => combineGroupFiles(files), [files]);
  const reload = useCallback(() => setToken((t) => t + 1), []);

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
