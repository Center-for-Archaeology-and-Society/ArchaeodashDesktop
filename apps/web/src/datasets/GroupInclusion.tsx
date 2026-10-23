/**
 * Which group files are included in analyses, shared by the Data Manager
 * (checkboxes) and every analysis page's Dataset picker, so choosing groups
 * once applies everywhere and survives page changes.
 *
 * Stored as the set of *excluded* paths, per project, in the browser's local
 * storage: groups are included by default and newly imported groups join
 * automatically. Without a provider (page tests) pages keep their own
 * per-page selection as before.
 */
import { createContext, useCallback, useContext, useMemo, useState, type ReactElement, type ReactNode } from 'react';

export interface GroupInclusionState {
  readonly excluded: ReadonlySet<string>;
  /** Replaces the excluded set. */
  readonly setExcluded: (paths: Iterable<string>) => void;
}

const GroupInclusionContext = createContext<GroupInclusionState | null>(null);

export function useGroupInclusion(): GroupInclusionState | null {
  return useContext(GroupInclusionContext);
}

const storageKey = (projectKey: string) => `archaeodash.excludedGroups:${projectKey}`;

function readExcluded(projectKey: string): Set<string> {
  try {
    const raw = globalThis.localStorage?.getItem(storageKey(projectKey));
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(parsed) ? parsed.filter((p): p is string => typeof p === 'string') : []);
  } catch {
    return new Set();
  }
}

function writeExcluded(projectKey: string, excluded: ReadonlySet<string>): void {
  try {
    globalThis.localStorage?.setItem(storageKey(projectKey), JSON.stringify([...excluded]));
  } catch {
    /* private window or storage blocked: the choice still holds this session */
  }
}

export interface GroupInclusionProviderProps {
  /** Identifies the project (its path); each project remembers its own choice. */
  readonly projectKey: string;
  readonly children: ReactNode;
}

export function GroupInclusionProvider({ projectKey, children }: GroupInclusionProviderProps): ReactElement {
  const [excluded, setExcludedState] = useState<ReadonlySet<string>>(() => readExcluded(projectKey));
  const setExcluded = useCallback((paths: Iterable<string>) => {
    const next = new Set(paths);
    writeExcluded(projectKey, next);
    setExcludedState(next);
  }, [projectKey]);
  const value = useMemo(() => ({ excluded, setExcluded }), [excluded, setExcluded]);
  return <GroupInclusionContext.Provider value={value}>{children}</GroupInclusionContext.Provider>;
}

/** Included paths among `ready`, in `ready`'s order. */
export function includedAmong(ready: readonly string[], excluded: ReadonlySet<string>): string[] {
  return ready.filter((p) => !excluded.has(p));
}

/** The excluded set that leaves exactly `included` of `ready` included. */
export function exclusionFor(ready: readonly string[], included: readonly string[]): string[] {
  return ready.filter((p) => !included.includes(p));
}
