import type { EuclideanMatchDto, TransferUnitsRequest } from '@archaeodash/client';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { layoutDendrogram } from './dendrogram-model.ts';

export interface SelectableRow {
  /**
   * Selection key: the analytical UUID, or for a Euclidean match row the
   * sample's UUID plus the match rank (see `euclideanMatchKeys`).
   */
  key: string;
  analyticalUuid: string;
  label: string;
  detail: string;
  /** Euclidean rows: the matched sample's group (the "Assign match group" destination). */
  matchGroup?: string | null;
}

const isIdentity = (value: unknown): value is string =>
  typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value);

/**
 * Selection key of every Euclidean match row, aligned with `rows`: the
 * sample's UUID and the row's 1-based rank among that sample's matches
 * (rows arrive ordered by sample, then distance). Rows without a valid
 * sample identity get null.
 */
export function euclideanMatchKeys(rows: readonly { analytical_uuid: string }[]): (string | null)[] {
  const rank = new Map<string, number>();
  return rows.map(row => {
    if (!isIdentity(row.analytical_uuid)) return null;
    const next = (rank.get(row.analytical_uuid) ?? 0) + 1;
    rank.set(row.analytical_uuid, next);
    return `${row.analytical_uuid}#${next}`;
  });
}

/** The distinct analytical units behind a selection of row keys (in selection order). */
export function selectedUnits(rows: readonly SelectableRow[], keys: Iterable<string>): string[] {
  const byKey = new Map(rows.map(row => [row.key, row.analyticalUuid]));
  const units: string[] = [];
  for (const key of keys) {
    const uuid = byKey.get(key);
    if (uuid !== undefined && !units.includes(uuid)) units.push(uuid);
  }
  return units;
}

/** Return rows that can be safely addressed by identity in an assignment. */
export function selectableRows(
  result: AnalysisResult,
  cutK = 2,
  rowLabel: (uuid: string | undefined, index: number) => string = (_uuid, i) => `Analytical unit ${i + 1}`,
): SelectableRow[] {
  if (result.kind === 'diagnostics') return [];
  if (result.kind === 'fit') {
    const data = result.data;
    const ids = data.analytical_uuids;
    if (!Array.isArray(ids) || ids.length !== data.n_rows || ids.some(id => !isIdentity(id)) || new Set(ids).size !== ids.length) return [];
    if (data.cluster) {
      if (data.cluster.length !== data.n_rows || data.cluster.some(label => !Number.isInteger(label) || label < 1) || (data.silhouette && data.silhouette.length !== data.n_rows)) return [];
      return ids.map((analyticalUuid, i) => ({ key: analyticalUuid, analyticalUuid, label: rowLabel(analyticalUuid, i), detail: `Cluster ${data.cluster![i]}` }));
    }
    const layout = layoutDendrogram(data, cutK);
    if (!layout || layout.clusterOfRow.length !== data.n_rows) return [];
    const groups = layout.clusterOfRow.map(group => group + 1);
    if (groups.some(group => !Number.isInteger(group))) return [];
    return ids.map((analyticalUuid, i) => ({ key: analyticalUuid, analyticalUuid, label: rowLabel(analyticalUuid, i), detail: `Cluster ${groups[i]}` }));
  }
  if (result.kind === 'membership') {
    const data = result.data;
    const ids = data.analytical_uuids;
    if (!Array.isArray(ids)) return [];
    const n = ids.length;
    if (ids.some(id => !isIdentity(id)) || new Set(ids).size !== n ||
        data.ids.length !== n || data.groups.length !== n || data.probabilities.length !== n || data.best_group.length !== n ||
        data.best_value.length !== n || data.in_group.length !== n ||
        data.probabilities.some(row => row.length !== data.eligible_groups.length)) return [];
    return ids.map((analyticalUuid, i) => ({
      key: analyticalUuid,
      analyticalUuid,
      label: data.ids[i] || `Analytical unit ${i + 1}`,
      detail: `${data.best_group[i] ?? 'No best group'} · ${data.best_value[i] == null ? 'Unavailable' : data.best_value[i]}`,
    }));
  }
  // Euclidean: one selectable row per match, so a specific match (not just
  // the nearest) can supply the destination group.
  const rows = result.data.rows;
  const keys = euclideanMatchKeys(rows);
  const selectable: SelectableRow[] = [];
  rows.forEach((row, i) => {
    const key = keys[i];
    if (!key) return;
    selectable.push({
      key, analyticalUuid: row.analytical_uuid, label: `${row.id || 'Unlabeled observation'} → ${row.match_id || 'unlabeled match'}`,
      detail: `Match group: ${row.match_group || 'none'} · ${row.distance ?? 'Unavailable'}`, matchGroup: row.match_group,
    });
  });
  return selectable;
}

/**
 * Build a manual move. With `multiSource`, the result spans several group
 * files: the request's source is a placeholder that the caller splits per
 * unit's own file, so any group (including the first file) may be the
 * destination.
 */
export function buildAssignmentRequest(
  result: AnalysisResult,
  selectedKeys: readonly string[],
  destinationPath: string,
  destinationGroupName?: string,
  multiSource = false,
  cutK = 2,
): TransferUnitsRequest {
  const validRows = selectableRows(result, cutK);
  const valid = new Set(validRows.map(row => row.key));
  const keys = [...new Set(selectedKeys)];
  if (!validRows.length || !keys.length || keys.some(key => !valid.has(key))) {
    throw new Error('Selection must contain valid analytical units from this result.');
  }
  const selected = selectedUnits(validRows, keys);
  const path = result.data.path;
  if (!path.trim() || !destinationPath.trim() || (!multiSource && path === destinationPath)) throw new Error('Choose a distinct, non-empty destination path.');
  if (!result.data.revision_id.trim()) throw new Error('The analysis result has no source revision.');
  if (destinationGroupName !== undefined && !destinationGroupName.trim()) throw new Error('Destination group name cannot be empty.');
  return {
    action: 'move', source_path: path, destination_path: destinationPath,
    ...(destinationGroupName === undefined ? {} : { destination_group_name: destinationGroupName }),
    selected_uuids: selected, expected_source_revision: result.data.revision_id,
  };
}

/**
 * For each sample in a Euclidean result, the key of its nearest usable
 * match row (finite distance, another sample, a non-blank group): the
 * legacy "nearest match" choice, as a starting selection.
 */
export function nearestMatchKeys(rows: readonly EuclideanMatchDto[]): string[] {
  const keys = euclideanMatchKeys(rows);
  const best = new Map<string, { key: string; distance: number }>();
  rows.forEach((row, i) => {
    const key = keys[i];
    if (!key || row.match_analytical_uuid === row.analytical_uuid || !isIdentity(row.match_analytical_uuid) ||
        typeof row.distance !== 'number' || !Number.isFinite(row.distance) || !row.match_group?.trim()) return;
    const current = best.get(row.analytical_uuid);
    if (!current || row.distance < current.distance) best.set(row.analytical_uuid, { key, distance: row.distance });
  });
  return [...best.values()].map(entry => entry.key);
}
