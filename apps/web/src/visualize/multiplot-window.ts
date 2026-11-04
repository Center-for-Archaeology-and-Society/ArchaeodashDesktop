/**
 * Multiplot pop-out windows: the plotted data is saved to IndexedDB under a
 * fresh key and each window (a browser popup, or a desktop window) loads
 * the app at `/multiplot-window/<key>[/<x>-<y>]` and reads it back. Windows
 * need no backend. Lasso selections made in a window come back over a
 * BroadcastChannel to the page that opened it.
 */
import { BASE_PATH } from '../basePath.ts';
import { isTauri } from '../transport.ts';
import type { MultiplotMode } from './Multiplot.tsx';

export type PopoutLayout = 'all' | 'each';

/** Route prefix of the pop-out view (no shell, no transport). */
export const MULTIPLOT_WINDOW_PATH = '/multiplot-window';
/** "One window per plot" opens at most this many windows. */
export const MAX_PLOT_WINDOWS = 36;
/** Saved snapshots older than this are removed when a new one is saved. */
const SNAPSHOT_TTL_MS = 24 * 60 * 60 * 1000;
const DB_NAME = 'archaeodash-multiplot';
const STORE = 'snapshots';
const SELECTION_CHANNEL = 'archaeodash-multiplot-selection';

/** What a pop-out window draws: only the plotted columns and rows. */
export interface MultiplotSnapshot {
  readonly createdAt: number;
  readonly columns: string[];
  readonly values: (number | null)[][];
  readonly groupLabels: string[];
  readonly groupNames: string[];
  readonly palette: string[];
  readonly rowUuids: string[];
  readonly x: string[];
  readonly y: string[];
  readonly mode: MultiplotMode;
  readonly pointSize: number;
  readonly height: number;
}

/** Stored form: columns as Float64Array (NaN for missing) to keep it small. */
interface StoredSnapshot extends Omit<MultiplotSnapshot, 'values'> {
  readonly values: Float64Array[];
}

export interface SnapshotSource {
  readonly columns: readonly string[];
  readonly values: readonly (readonly (number | null)[])[];
  readonly groupLabels: readonly string[];
  readonly groupNames: readonly string[];
  readonly palette: readonly string[];
  readonly rowIndices: readonly number[];
  readonly rowUuids?: readonly string[];
}

/** The plotted subset: columns used on either axis, rows that pass the filter. */
export function buildSnapshot(
  source: SnapshotSource,
  view: { x: readonly string[]; y: readonly string[]; mode: MultiplotMode; pointSize: number; height: number },
): MultiplotSnapshot {
  const used = new Set([...view.x, ...view.y]);
  const columns = source.columns.filter((c) => used.has(c));
  const rows = source.rowIndices;
  return {
    createdAt: Date.now(),
    columns,
    values: columns.map((c) => {
      const col = source.values[source.columns.indexOf(c)] ?? [];
      return rows.map((r) => col[r] ?? null);
    }),
    groupLabels: rows.map((r) => source.groupLabels[r] ?? 'All'),
    groupNames: [...source.groupNames],
    palette: [...source.palette],
    rowUuids: rows.map((r) => source.rowUuids?.[r] ?? ''),
    x: view.x.filter((c) => used.has(c)),
    y: view.y.filter((c) => used.has(c)),
    mode: view.mode,
    pointSize: view.pointSize,
    height: view.height,
  };
}

function toStored(s: MultiplotSnapshot): StoredSnapshot {
  return { ...s, values: s.values.map((col) => Float64Array.from(col, (v) => (v === null ? Number.NaN : v))) };
}

function fromStored(s: StoredSnapshot): MultiplotSnapshot {
  return { ...s, values: s.values.map((col) => Array.from(col, (v) => (Number.isNaN(v) ? null : v))) };
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error('could not open the plot window store'));
  });
}

function done(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error ?? new Error('plot window store failed'));
    tx.onabort = () => reject(tx.error ?? new Error('plot window store aborted'));
  });
}

/** Saves a snapshot under `key` and drops ones older than a day. */
export async function saveSnapshot(key: string, snapshot: MultiplotSnapshot): Promise<void> {
  const db = await openDb();
  try {
    const tx = db.transaction(STORE, 'readwrite');
    const store = tx.objectStore(STORE);
    const cutoff = Date.now() - SNAPSHOT_TTL_MS;
    const cursorReq = store.openCursor();
    cursorReq.onsuccess = () => {
      const cursor = cursorReq.result;
      if (!cursor) return;
      const value = cursor.value as Partial<StoredSnapshot> | undefined;
      if (!value || typeof value.createdAt !== 'number' || value.createdAt < cutoff) cursor.delete();
      cursor.continue();
    };
    store.put(toStored(snapshot), key);
    await done(tx);
  } finally {
    db.close();
  }
}

async function readSnapshot(key: string): Promise<MultiplotSnapshot | null> {
  const db = await openDb();
  try {
    const tx = db.transaction(STORE, 'readonly');
    const req = tx.objectStore(STORE).get(key);
    const value = await new Promise<unknown>((resolve, reject) => {
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error ?? new Error('could not read the plot data'));
    });
    return value ? fromStored(value as StoredSnapshot) : null;
  } finally {
    db.close();
  }
}

/** Reads `key`, waiting a little while the opener may still be saving it. */
export async function loadSnapshot(key: string, waitMs = 15_000): Promise<MultiplotSnapshot | null> {
  const until = Date.now() + waitMs;
  for (;;) {
    const found = await readSnapshot(key);
    if (found || Date.now() >= until) return found;
    await new Promise((r) => setTimeout(r, 200));
  }
}

/** A new snapshot key (letters and digits only; it goes into a URL path). */
export function newSnapshotKey(): string {
  const random = Array.from({ length: 4 }, () => Math.floor(Math.random() * 36 ** 6).toString(36).padStart(6, '0')).join('');
  return `${Date.now().toString(36)}${random}`;
}

/** App route of a pop-out window; `pair` holds indices into the snapshot columns. */
export function popoutRoute(key: string, pair?: { x: number; y: number }): string {
  return `${MULTIPLOT_WINDOW_PATH}/${key}${pair ? `/${pair.x}-${pair.y}` : ''}`;
}

/** Parses `/multiplot-window/<key>[/<x>-<y>]` (after the base path), or null. */
export function parsePopoutRoute(pathname: string): { key: string; pair: { x: number; y: number } | null } | null {
  const rest = pathname.startsWith(`${BASE_PATH}${MULTIPLOT_WINDOW_PATH}/`)
    ? pathname.slice(`${BASE_PATH}${MULTIPLOT_WINDOW_PATH}/`.length)
    : null;
  if (rest === null) return null;
  const m = /^([a-z0-9]{8,64})(?:\/(\d{1,4})-(\d{1,4}))?\/?$/.exec(rest);
  if (!m) return null;
  return { key: m[1]!, pair: m[2] !== undefined ? { x: Number(m[2]), y: Number(m[3]) } : null };
}

export interface PopoutTarget {
  readonly title: string;
  readonly pair?: { x: number; y: number };
  readonly width: number;
  readonly height: number;
}

/**
 * Opens the windows and saves the data. In a browser the windows open first
 * (still inside the click, so pop-up blockers allow them) and read the data
 * once it is saved; returns how many the browser blocked.
 */
export async function openPopouts(key: string, snapshot: MultiplotSnapshot, targets: readonly PopoutTarget[]): Promise<{ blocked: number }> {
  if (isTauri()) {
    await saveSnapshot(key, snapshot);
    const { invoke } = await import('@tauri-apps/api/core');
    for (const [i, t] of targets.entries()) {
      await invoke('open_multiplot_window', {
        label: `multiplot-${key}-${i}`,
        path: popoutRoute(key, t.pair).slice(1),
        title: t.title,
        width: t.width,
        height: t.height,
      });
    }
    return { blocked: 0 };
  }
  let blocked = 0;
  for (const [i, t] of targets.entries()) {
    const offset = 30 * (i % 10);
    const features = `popup=yes,width=${Math.round(t.width)},height=${Math.round(t.height)},left=${80 + offset},top=${60 + offset},resizable=yes,scrollbars=yes`;
    const win = window.open(`${BASE_PATH}${popoutRoute(key, t.pair)}`, `archaeodash-multiplot-${key}-${i}`, features);
    if (!win) blocked++;
  }
  if (blocked < targets.length) await saveSnapshot(key, snapshot);
  return { blocked };
}

interface SelectionMessage {
  readonly key: string;
  readonly uuids: string[];
}

/** Sends a pop-out window's selection to the page that opened it. */
export function postSelection(key: string, uuids: readonly string[]): void {
  if (typeof BroadcastChannel === 'undefined') return;
  const channel = new BroadcastChannel(SELECTION_CHANNEL);
  channel.postMessage({ key, uuids: [...uuids] } satisfies SelectionMessage);
  channel.close();
}

/** Listens for selections from windows whose key `accept` returns true for. */
export function onPopoutSelection(accept: (key: string) => boolean, apply: (uuids: string[]) => void): () => void {
  if (typeof BroadcastChannel === 'undefined') return () => undefined;
  const channel = new BroadcastChannel(SELECTION_CHANNEL);
  channel.onmessage = (event: MessageEvent<unknown>) => {
    const msg = event.data as Partial<SelectionMessage> | null;
    if (!msg || typeof msg.key !== 'string' || !Array.isArray(msg.uuids) || !accept(msg.key)) return;
    apply(msg.uuids.filter((u): u is string => typeof u === 'string'));
  };
  return () => channel.close();
}
