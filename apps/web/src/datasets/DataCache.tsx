/**
 * Session cache of project data shared by every page, so moving between
 * pages and Explore tabs does not reload what is already in memory:
 *
 * - group rows, keyed by file path and revision (a save makes a new
 *   revision, so edited files reload and nothing stale is served);
 * - computed results (missing-value profiles, histograms, …) keyed by the
 *   request plus the revisions of the files it read;
 * - a background preload of every ready group's rows when a project opens.
 *
 * Without a provider (page tests) callers fetch directly, as before.
 */
import { createContext, useContext, useEffect, useState, type ReactElement, type ReactNode } from 'react';
import type { GroupRowsResponse, GroupsService } from '@archaeodash/client';

/** Rows preloaded in the background; larger projects load on demand. */
export const PRELOAD_ROW_LIMIT = 500_000;
/** Computed results kept (least recently used dropped first). */
const MAX_RESULTS = 200;

interface RowsEntry {
  revision: string | undefined;
  promise: Promise<GroupRowsResponse>;
}

export class DataCache {
  private readonly rows = new Map<string, RowsEntry>();
  private readonly results = new Map<string, Promise<unknown>>();
  private readonly settled = new Map<string, unknown>();
  private preloadRun = 0;

  /**
   * Rows of `path` at `revision`, fetching only when that revision is not
   * cached yet. An unknown revision always fetches.
   */
  rowsFor(path: string, revision: string | undefined, fetch: () => Promise<GroupRowsResponse>): Promise<GroupRowsResponse> {
    const hit = this.rows.get(path);
    if (hit && revision !== undefined && hit.revision === revision) return hit.promise;
    const entry: RowsEntry = { revision, promise: fetch() };
    this.rows.set(path, entry);
    entry.promise.then(
      (r) => { if (entry.revision === undefined) entry.revision = r.revision_id; },
      () => { if (this.rows.get(path) === entry) this.rows.delete(path); },
    );
    return entry.promise;
  }

  /** Forgets cached rows (e.g. a manual Reload). */
  dropRows(paths: Iterable<string>): void {
    for (const p of paths) this.rows.delete(p);
  }

  /** A computed result, shared by identical requests; failures are not kept. */
  result<T>(key: string, compute: () => Promise<T>): Promise<T> {
    const hit = this.results.get(key);
    if (hit) {
      // Refresh recency.
      this.results.delete(key);
      this.results.set(key, hit);
      return hit as Promise<T>;
    }
    const promise = compute();
    this.results.set(key, promise);
    promise.then(
      (value) => { if (this.results.get(key) === promise) this.settled.set(key, value); },
      () => { if (this.results.get(key) === promise) this.results.delete(key); },
    );
    while (this.results.size > MAX_RESULTS) {
      const oldest = this.results.keys().next().value as string;
      this.results.delete(oldest);
      this.settled.delete(oldest);
    }
    return promise;
  }

  /** A result that has already finished, for rendering without a spinner. */
  peek<T>(key: string): T | undefined {
    return this.results.has(key) ? (this.settled.get(key) as T | undefined) : undefined;
  }

  /**
   * Loads every listed group's rows in the background, one at a time, up to
   * `PRELOAD_ROW_LIMIT` rows. A newer call cancels an older one.
   */
  async preload(groups: Pick<GroupsService, 'rows'>, files: readonly { path: string; revision: string | null; rowCount?: number }[]): Promise<void> {
    const run = ++this.preloadRun;
    let budget = PRELOAD_ROW_LIMIT;
    for (const file of files) {
      if (run !== this.preloadRun || file.revision === null) return;
      budget -= file.rowCount ?? 0;
      if (budget < 0) return;
      await this.rowsFor(file.path, file.revision, () => groups.rows(file.path)).catch(() => undefined);
    }
  }
}

const DataCacheContext = createContext<DataCache | null>(null);

export function useDataCache(): DataCache | null {
  return useContext(DataCacheContext);
}

/** One cache per open project (remount with a new `key` to start fresh). */
export function DataCacheProvider({ children }: { children: ReactNode }): ReactElement {
  const [cache] = useState(() => new DataCache());
  return <DataCacheContext.Provider value={cache}>{children}</DataCacheContext.Provider>;
}

export type CachedState<T> =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'loaded'; data: T };

/**
 * Runs `run` for `key` through the cache: a finished result renders at once,
 * an in-flight one is shared. `key` must identify the request and the data
 * revisions it reads; `null` means "nothing to run".
 */
export function useCachedResult<T>(key: string | null, run: () => Promise<T>): CachedState<T> {
  const cache = useDataCache();
  const initial = (): CachedState<T> => {
    if (key === null) return { kind: 'idle' };
    const done = cache?.peek<T>(key);
    return done === undefined ? { kind: 'loading' } : { kind: 'loaded', data: done };
  };
  const [state, setState] = useState<CachedState<T>>(initial);
  useEffect(() => {
    if (key === null) {
      setState({ kind: 'idle' });
      return;
    }
    const done = cache?.peek<T>(key);
    if (done !== undefined) {
      setState({ kind: 'loaded', data: done });
      return;
    }
    let active = true;
    setState({ kind: 'loading' });
    (cache ? cache.result(key, run) : run())
      .then((data) => { if (active) setState({ kind: 'loaded', data }); })
      .catch((err: unknown) => {
        if (active) setState({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      });
    return () => { active = false; };
    // `run` is recreated every render; `key` identifies the work.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cache, key]);
  return state;
}
