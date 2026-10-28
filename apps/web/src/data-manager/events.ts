/**
 * Cross-page "project group files changed" signal. Pages that rewrite group
 * files outside the sidebar (e.g. Visualize & Assign) call
 * `notifyGroupsChanged()` so the Data Manager file tree and every page's
 * group list (`useGroupFiles`) rescan. Nothing is remounted, so results that
 * do not depend on group membership are kept.
 *
 * A page moving samples also holds group-dependent updates (`holdGroupUpdates`)
 * from before the move until every listener has refreshed: plots keep their
 * last picture and ordinations and precompute wait, then update once.
 */
export const GROUPS_CHANGED_EVENT = 'archaeodash:groups-changed';

/** Refreshes started by the notice being dispatched (collected while it runs). */
let collecting: Promise<unknown>[] | null = null;

/** Increments on each notice; scans started earlier may predate the change. */
let epoch = 0;

/** Change epoch: a scan may be shared only within one epoch (see `sharedScan`). */
export function groupsChangeEpoch(): number {
  return epoch;
}

/**
 * Tells every page and the Data Manager that group files changed. Resolves
 * once each listener's refresh (when it returns a promise) has settled.
 */
export function notifyGroupsChanged(): Promise<void> {
  epoch += 1;
  if (typeof window === 'undefined') return Promise.resolve();
  const started: Promise<unknown>[] = [];
  const outer = collecting;
  collecting = started;
  try {
    window.dispatchEvent(new Event(GROUPS_CHANGED_EVENT));
  } finally {
    collecting = outer;
  }
  return Promise.allSettled(started).then(() => undefined);
}

/** Subscribe to the signal; returns an unsubscribe function. A returned
 *  promise is awaited by `notifyGroupsChanged`. */
export function onGroupsChanged(listener: () => void | Promise<unknown>): () => void {
  if (typeof window === 'undefined') return () => {};
  const handler = () => {
    const result = listener();
    if (result && collecting) collecting.push(result);
  };
  window.addEventListener(GROUPS_CHANGED_EVENT, handler);
  return () => window.removeEventListener(GROUPS_CHANGED_EVENT, handler);
}

let holds = 0;
const holdListeners = new Set<(held: boolean) => void>();

/** Whether a sample move (and the refresh after it) is under way. */
export function groupUpdatesHeld(): boolean {
  return holds > 0;
}

/**
 * Holds plot and ordination updates while samples move between groups.
 * Returns the release function; call it once, after the move and the
 * refreshes it triggers (`await notifyGroupsChanged()`) have finished.
 */
export function holdGroupUpdates(): () => void {
  holds += 1;
  if (holds === 1) for (const listener of [...holdListeners]) listener(true);
  let released = false;
  return () => {
    if (released) return;
    released = true;
    holds -= 1;
    if (holds === 0) for (const listener of [...holdListeners]) listener(false);
  };
}

/** Subscribe to hold changes; returns an unsubscribe function. */
export function onGroupUpdatesHeld(listener: (held: boolean) => void): () => void {
  holdListeners.add(listener);
  return () => {
    holdListeners.delete(listener);
  };
}
