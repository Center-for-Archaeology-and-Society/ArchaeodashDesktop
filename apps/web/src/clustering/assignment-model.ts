import type { TransferUnitsRequest } from '@archaeodash/client';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { layoutDendrogram } from './dendrogram-model.ts';

export interface SelectableRow {
  analyticalUuid: string;
  label: string;
  detail: string;
}

const isIdentity = (value: unknown): value is string =>
  typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value);

/** Return rows that can be safely addressed by identity in an assignment. */
export function selectableRows(result: AnalysisResult, cutK = 2): SelectableRow[] {
  if (result.kind === 'diagnostics') return [];
  if (result.kind === 'fit') {
    const data = result.data;
    const ids = data.analytical_uuids;
    if (!Array.isArray(ids) || ids.length !== data.n_rows || ids.some(id => !isIdentity(id)) || new Set(ids).size !== ids.length) return [];
    if (data.cluster) {
      if (data.cluster.length !== data.n_rows || data.cluster.some(label => !Number.isInteger(label) || label < 1) || (data.silhouette && data.silhouette.length !== data.n_rows)) return [];
      return ids.map((analyticalUuid, i) => ({ analyticalUuid, label: `Analytical unit ${i + 1}`, detail: `Cluster ${data.cluster![i]}` }));
    }
    const layout = layoutDendrogram(data, cutK);
    if (!layout || layout.leaves.length !== data.n_rows) return [];
    const groups = new Array<number>(data.n_rows);
    for (const leaf of layout.leaves) groups[leaf.row - 1] = leaf.group + 1;
    if (groups.some(group => !Number.isInteger(group))) return [];
    return ids.map((analyticalUuid, i) => ({ analyticalUuid, label: `Analytical unit ${i + 1}`, detail: `Cluster ${groups[i]}` }));
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
      analyticalUuid,
      label: data.ids[i] || `Analytical unit ${i + 1}`,
      detail: `${data.best_group[i] ?? 'No best group'} · ${data.best_value[i] == null ? 'Unavailable' : data.best_value[i]}`,
    }));
  }
  const data = result.data;
  const rows = data.rows;
  const seen = new Set<string>();
  const selectable: SelectableRow[] = [];
  for (const row of rows) {
    if (!isIdentity(row.analytical_uuid) || seen.has(row.analytical_uuid)) continue;
    seen.add(row.analytical_uuid);
    selectable.push({ analyticalUuid: row.analytical_uuid, label: row.id || 'Unlabeled observation', detail: `Match: ${row.match_id} · ${row.distance ?? 'Unavailable'}` });
  }
  return selectable;
}

export function buildAssignmentRequest(
  result: AnalysisResult,
  selectedUuids: readonly string[],
  destinationPath: string,
  destinationGroupName?: string,
): TransferUnitsRequest {
  const validRows = selectableRows(result);
  const valid = new Set(validRows.map(row => row.analyticalUuid));
  const selected = [...new Set(selectedUuids)];
  if (!validRows.length || !selected.length || selected.some(uuid => !isIdentity(uuid) || !valid.has(uuid))) {
    throw new Error('Selection must contain valid analytical units from this result.');
  }
  const path = result.data.path;
  if (!path.trim() || !destinationPath.trim() || path === destinationPath) throw new Error('Choose a distinct, non-empty destination path.');
  if (!result.data.revision_id.trim()) throw new Error('The analysis result has no source revision.');
  if (destinationGroupName !== undefined && !destinationGroupName.trim()) throw new Error('Destination group name cannot be empty.');
  return {
    action: 'move', source_path: path, destination_path: destinationPath,
    ...(destinationGroupName === undefined ? {} : { destination_group_name: destinationGroupName }),
    selected_uuids: selected, expected_source_revision: result.data.revision_id,
  };
}
