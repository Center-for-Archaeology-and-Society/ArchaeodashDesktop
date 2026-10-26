/**
 * Cross-page "project group files changed" signal. Pages that rewrite group
 * files outside the sidebar (e.g. Visualize & Assign) call
 * `notifyGroupsChanged()` so the Data Manager file tree and every page's
 * group list (`useGroupFiles`) rescan. Nothing is remounted, so results that
 * do not depend on group membership are kept.
 */
export const GROUPS_CHANGED_EVENT = 'archaeodash:groups-changed';

export function notifyGroupsChanged(): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new Event(GROUPS_CHANGED_EVENT));
}

/** Subscribe to the signal; returns an unsubscribe function. */
export function onGroupsChanged(listener: () => void): () => void {
  if (typeof window === 'undefined') return () => {};
  window.addEventListener(GROUPS_CHANGED_EVENT, listener);
  return () => window.removeEventListener(GROUPS_CHANGED_EVENT, listener);
}
