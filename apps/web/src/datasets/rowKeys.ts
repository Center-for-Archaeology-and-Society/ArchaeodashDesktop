/**
 * Content keys for the loaded rows, so pages recompute only when the data a
 * result depends on changes.
 *
 * - `rowContentKey`: which samples are loaded (by hidden analytical UUID)
 *   and their measured values. PCA, UMAP, element transforms, and cluster
 *   fits depend only on this, so moving samples between groups, or loading
 *   the same groups in another order, keeps the key.
 * - `rowGroupKey`: the same plus each sample's group, for results that
 *   depend on group membership (LDA by group, membership probabilities).
 *
 * Both are order-independent (per-row hashes are summed), so results are
 * matched back to rows by UUID with `realignByUuid`, never by position.
 */
import type { CombinedRows } from '../visualize/combine.ts';

const float = new Float64Array(1);
const words = new Uint32Array(float.buffer);

function mix(h: number, word: number): number {
  return Math.imul(h ^ word, 16777619);
}

function mixText(h: number, text: string): number {
  let out = h;
  for (let i = 0; i < text.length; i += 1) out = mix(out, text.charCodeAt(i));
  // Separator, so "ab"+"c" and "a"+"bc" differ.
  return mix(out, 0x1f);
}

/** murmur3 finalizer: spreads every input bit over the result. */
function finish(h: number): number {
  let out = h ^ (h >>> 16);
  out = Math.imul(out, 0x85ebca6b);
  out ^= out >>> 13;
  out = Math.imul(out, 0xc2b2ae35);
  return (out ^ (out >>> 16)) >>> 0;
}

function rowHash(seed: number, uuid: string, elemental: readonly (number | null | undefined)[], group: string | null): number {
  let h = mixText(seed, uuid);
  for (const value of elemental) {
    if (value === null || value === undefined) {
      h = mix(h, 0x7ff8dead);
      continue;
    }
    float[0] = value;
    h = mix(mix(h, words[0]!), words[1]!);
  }
  if (group !== null) h = mixText(h, group);
  return finish(h);
}

function key(combined: CombinedRows, withGroups: boolean): string {
  let sumA = 0;
  let sumB = 0;
  let xorA = 0;
  combined.rows.forEach((row, i) => {
    const group = withGroups ? (combined.groupOf[i] ?? '') : null;
    const a = rowHash(0x811c9dc5, row.analytical_uuid, row.elemental, group);
    const b = rowHash(0x01000193, row.analytical_uuid, row.elemental, group);
    sumA = (sumA + a) >>> 0;
    sumB = (sumB + b) >>> 0;
    xorA = (xorA ^ a) >>> 0;
  });
  const columns = finish(mixText(0x811c9dc5, combined.elementalColumns.join('\u0000')));
  return [combined.rows.length, columns, sumA, sumB, xorA].map((n) => n.toString(36)).join('.');
}

/** Key of the loaded samples and their measured values (see module docs). */
export function rowContentKey(combined: CombinedRows): string {
  return key(combined, false);
}

/** `rowContentKey` plus each sample's group. */
export function rowGroupKey(combined: CombinedRows): string {
  return key(combined, true);
}

/**
 * Values computed for rows `fromUuids` (same order as `values`), realigned to
 * rows `toUuids`; rows the result does not cover get `undefined`. A UUID
 * listed more than once (a copied sample) matches its copies in order.
 */
export function realignByUuid<T>(
  fromUuids: readonly string[],
  values: readonly T[],
  toUuids: readonly string[],
): (T | undefined)[] {
  if (fromUuids === toUuids) return [...values];
  const slots = new Map<string, T[]>();
  fromUuids.forEach((uuid, i) => {
    if (i >= values.length) return;
    const value = values[i] as T;
    const list = slots.get(uuid);
    if (list) list.push(value);
    else slots.set(uuid, [value]);
  });
  const used = new Map<string, number>();
  return toUuids.map((uuid) => {
    const k = used.get(uuid) ?? 0;
    used.set(uuid, k + 1);
    return slots.get(uuid)?.[k];
  });
}

/** `realignByUuid` for numeric score rows; rows not covered become NaN. */
export function realignRowsByUuid(
  fromUuids: readonly string[],
  rows: readonly number[][],
  toUuids: readonly string[],
): number[][] {
  const width = rows[0]?.length ?? 0;
  return realignByUuid(fromUuids, rows, toUuids).map((row) => row ?? new Array<number>(width).fill(Number.NaN));
}
