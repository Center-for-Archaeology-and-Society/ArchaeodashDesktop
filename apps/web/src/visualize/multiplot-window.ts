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
  /** Slot in the window grid (the plot's place in the multiplot). */
  readonly cell?: TileCell;
}

/** A window's place in a grid of windows (0-based). */
export interface TileCell {
  readonly row: number;
  readonly col: number;
}

/** Part of the screen windows may cover (CSS pixels, screen coordinates). */
export interface ScreenArea {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

/** A window's outer box (frame included), in screen coordinates. */
export interface WindowRect {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

/** Tiles smaller than this overlap instead of shrinking further. */
export const MIN_TILE = { width: 360, height: 300 } as const;
/** Width-to-height ratio a single-plot window looks best at. */
const TILE_ASPECT = 1.15;

/** The available screen area (the browser's work area, taskbars excluded). */
export function screenArea(): ScreenArea {
  const s = (typeof window !== 'undefined' ? window.screen : undefined) as
    | (Screen & { availLeft?: number; availTop?: number })
    | undefined;
  return {
    left: Number.isFinite(s?.availLeft) ? s!.availLeft! : 0,
    top: Number.isFinite(s?.availTop) ? s!.availTop! : 0,
    width: s?.availWidth || 1600,
    height: s?.availHeight || 1000,
  };
}

/**
 * Columns for `count` windows on `area`: the grid whose tiles come closest
 * to a plot's shape, preferring grids with fewer empty slots.
 */
export function fitColumns(count: number, area: ScreenArea): number {
  let best = 1;
  let bestScore = Infinity;
  for (let columns = 1; columns <= Math.max(1, count); columns++) {
    const rows = Math.ceil(count / columns);
    const aspect = area.width / columns / (area.height / rows);
    const score = Math.abs(Math.log(aspect / TILE_ASPECT)) + (columns * rows - count) / Math.max(1, count);
    if (score < bestScore) {
      bestScore = score;
      best = columns;
    }
  }
  return best;
}

/**
 * Outer boxes that tile `area` with one window per cell. Without cells (or
 * with cells that collide) the windows fill a grid that fits the screen in
 * order. Tiles never get smaller than MIN_TILE: when they would, the
 * windows keep that size and overlap, spread evenly across the area.
 */
export function tileRects(count: number, area: ScreenArea, cells?: readonly (TileCell | undefined)[], min = MIN_TILE): WindowRect[] {
  if (count <= 0) return [];
  const given = cells && cells.length === count && cells.every((c) => c !== undefined) ? (cells as TileCell[]) : null;
  const unique = given && new Set(given.map((c) => `${c.row}:${c.col}`)).size === count;
  let placed: TileCell[];
  let columns: number;
  let rows: number;
  if (given && unique) {
    placed = given;
    columns = Math.max(...given.map((c) => c.col)) + 1;
    rows = Math.max(...given.map((c) => c.row)) + 1;
  } else {
    columns = fitColumns(count, area);
    rows = Math.ceil(count / columns);
    placed = Array.from({ length: count }, (_, i) => ({ row: Math.floor(i / columns), col: i % columns }));
  }
  const width = Math.min(area.width, Math.max(min.width, Math.floor(area.width / columns)));
  const height = Math.min(area.height, Math.max(min.height, Math.floor(area.height / rows)));
  const stepX = columns > 1 ? (area.width - width) / (columns - 1) : 0;
  const stepY = rows > 1 ? (area.height - height) / (rows - 1) : 0;
  return placed.map((c) => ({
    left: Math.round(area.left + c.col * stepX),
    top: Math.round(area.top + c.row * stepY),
    width,
    height,
  }));
}

/** Browser popups: the frame around the page, used until a window can be measured. */
const BROWSER_FRAME = { width: 8, height: 72 } as const;

/** Moves and resizes a popup this page opened (browsers may ignore it). */
function placeBrowserWindow(win: Window, rect: WindowRect): void {
  try {
    win.moveTo(rect.left, rect.top);
    win.resizeTo(rect.width, rect.height);
  } catch {
    // The window closed or the browser forbids it; it stays where it is.
  }
}

/** The windows of one "open" click, kept so they can be tiled again. */
export interface PopoutGroup {
  readonly key: string;
  readonly cells: readonly (TileCell | undefined)[];
  /** Browser windows, by target index (null where the browser blocked one). */
  readonly browser: readonly (Window | null)[];
  /** Desktop window labels, by target index. */
  readonly labels: readonly string[];
}

function windowLabel(key: string, i: number): string {
  return `multiplot-${key}-${i}`;
}

/**
 * Opens the windows, tiled across the screen, and saves the data. In a
 * browser the windows open first (still inside the click, so pop-up
 * blockers allow them) and read the data once it is saved; returns how many
 * the browser blocked and the group for tileWindows.
 */
export async function openPopouts(
  key: string,
  snapshot: MultiplotSnapshot,
  targets: readonly PopoutTarget[],
  options: { tile?: boolean } = {},
): Promise<{ blocked: number; group: PopoutGroup }> {
  const tile = options.tile ?? targets.length > 1;
  const cells = targets.map((t) => t.cell);
  const rects = tile ? tileRects(targets.length, screenArea(), cells) : null;
  const labels = targets.map((_, i) => windowLabel(key, i));
  if (isTauri()) {
    await saveSnapshot(key, snapshot);
    const { invoke } = await import('@tauri-apps/api/core');
    for (const [i, t] of targets.entries()) {
      const r = rects?.[i];
      await invoke('open_multiplot_window', {
        label: labels[i],
        path: popoutRoute(key, t.pair).slice(1),
        title: t.title,
        width: r ? r.width : t.width,
        height: r ? r.height : t.height,
        x: r ? r.left : null,
        y: r ? r.top : null,
      });
    }
    return { blocked: 0, group: { key, cells, browser: [], labels } };
  }
  let blocked = 0;
  const browser: (Window | null)[] = [];
  for (const [i, t] of targets.entries()) {
    const r = rects?.[i];
    const offset = 30 * (i % 10);
    const box = r
      ? { left: r.left, top: r.top, width: r.width - BROWSER_FRAME.width, height: r.height - BROWSER_FRAME.height }
      : { left: 80 + offset, top: 60 + offset, width: t.width, height: t.height };
    const features = `popup=yes,width=${Math.round(box.width)},height=${Math.round(box.height)},left=${Math.round(box.left)},top=${Math.round(box.top)},resizable=yes,scrollbars=yes`;
    const win = window.open(`${BASE_PATH}${popoutRoute(key, t.pair)}`, `archaeodash-multiplot-${key}-${i}`, features);
    if (!win) blocked++;
    // The features set the page size; set the outer box exactly where allowed.
    else if (r) placeBrowserWindow(win, r);
    browser.push(win);
  }
  if (blocked < targets.length) await saveSnapshot(key, snapshot);
  return { blocked, group: { key, cells, browser, labels } };
}

/**
 * Tiles the still-open windows of `groups` across the screen again. All of
 * one group open: they keep their grid slots. Otherwise the open ones fill
 * a grid that fits the screen. Returns how many windows were tiled. A
 * browser may ignore the move (Firefox and Safari can turn it off).
 */
export async function tileWindows(groups: readonly PopoutGroup[]): Promise<{ tiled: number }> {
  if (isTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    const all = groups.flatMap((g) => g.labels.map((label, i) => ({ label, cell: g.cells[i] })));
    if (all.length === 0) return { tiled: 0 };
    const open = await invoke<boolean[]>('multiplot_windows_open', { labels: all.map((w) => w.label) });
    const live = all.filter((_, i) => open[i] === true);
    const rects = tileRects(live.length, screenArea(), keepCells(groups, live.length, all.length, live.map((w) => w.cell)));
    for (const [i, w] of live.entries()) {
      const r = rects[i]!;
      await invoke('place_multiplot_window', { label: w.label, x: r.left, y: r.top, width: r.width, height: r.height });
    }
    return { tiled: live.length };
  }
  const all = groups.flatMap((g) => g.browser.map((win, i) => ({ win, cell: g.cells[i] })));
  const live = all.filter((w): w is { win: Window; cell: TileCell | undefined } => w.win !== null && !w.win.closed);
  const rects = tileRects(live.length, screenArea(), keepCells(groups, live.length, all.length, live.map((w) => w.cell)));
  for (const [i, w] of live.entries()) {
    placeBrowserWindow(w.win, rects[i]!);
    try {
      w.win.focus();
    } catch {
      // Focus is a courtesy; some browsers ignore it.
    }
  }
  return { tiled: live.length };
}

/** Grid slots are kept only for one group with every window still open. */
function keepCells(
  groups: readonly PopoutGroup[],
  liveCount: number,
  totalCount: number,
  cells: readonly (TileCell | undefined)[],
): readonly (TileCell | undefined)[] | undefined {
  return groups.length === 1 && liveCount === totalCount ? cells : undefined;
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
