import type { BatchTransferUnitsRequest, GroupCandidate } from '@archaeodash/client';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { selectableRows } from './assignment-model.ts';
import { layoutDendrogram } from './dendrogram-model.ts';

export interface AssignmentRecommendation {
  analyticalUuid: string;
  groupLabel: string;
}

export interface AutomaticDestinationMapping {
  groupLabel: string;
  destinationPath: string;
  newGroupName?: string;
}

const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
const isIdentity = (value: unknown): value is string =>
  typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);

function selectedIds(result: AnalysisResult, selectedUuids: readonly string[], cutK: number): string[] {
  const rows = selectableRows(result, cutK);
  const valid = new Set(rows.map(row => row.analyticalUuid));
  const selected = [...new Set(selectedUuids)];
  if (!selected.length || selected.some(uuid => !valid.has(uuid))) {
    throw new Error('Selection must contain valid analytical units from this result.');
  }
  return selected;
}

/** Recommend a destination label for each selected analytical identity. */
export function recommendedAssignments(
  result: AnalysisResult,
  selectedUuids: readonly string[],
  cutK = 2,
): AssignmentRecommendation[] {
  const selected = selectedIds(result, selectedUuids, cutK);
  if (result.kind === 'fit') {
    const { data } = result;
    const clusterByUuid = new Map<string, number>();
    if (Array.isArray(data.cluster)) {
      data.analytical_uuids.forEach((uuid, i) => clusterByUuid.set(uuid, data.cluster![i]!));
    } else {
      const layout = layoutDendrogram(data, cutK);
      if (!layout) throw new Error('The clustering result cannot be cut into the requested number of clusters.');
      const groupByRow = new Map(layout.leaves.map(leaf => [leaf.row, leaf.group + 1]));
      data.analytical_uuids.forEach((uuid, i) => {
        const group = groupByRow.get(i + 1);
        if (group !== undefined) clusterByUuid.set(uuid, group);
      });
    }
    return selected.map(analyticalUuid => {
      const cluster = clusterByUuid.get(analyticalUuid);
      if (!cluster) throw new Error('A selected observation has no available cluster.');
      return { analyticalUuid, groupLabel: `Cluster ${cluster}` };
    });
  }
  if (result.kind === 'membership') {
    const { data } = result;
    const indexByUuid = new Map(data.analytical_uuids.map((uuid, index) => [uuid, index]));
    return selected.map(analyticalUuid => {
      const i = indexByUuid.get(analyticalUuid)!;
      const group = data.best_group[i];
      const value = data.best_value[i];
      if (typeof group !== 'string' || !group.trim() || !data.eligible_groups.includes(group) || !finite(value)) {
        throw new Error('A selected observation has no eligible group with a finite best score.');
      }
      return { analyticalUuid, groupLabel: group };
    });
  }
  if (result.kind === 'euclidean') {
    const selectedSet = new Set(selected);
    const closest = new Map<string, { distance: number; groups: Set<string> }>();
    for (const row of result.data.rows) {
      if (!selectedSet.has(row.analytical_uuid) || row.match_analytical_uuid === row.analytical_uuid || !finite(row.distance)) continue;
      if (!isIdentity(row.match_analytical_uuid) || typeof row.match_group !== 'string' || !row.match_group.trim()) {
        throw new Error('A finite Euclidean match has incomplete identity or group data.');
      }
      const current = closest.get(row.analytical_uuid);
      if (!current || row.distance < current.distance) {
        closest.set(row.analytical_uuid, { distance: row.distance, groups: new Set([row.match_group]) });
      } else if (row.distance === current.distance) {
        current.groups.add(row.match_group);
      }
    }
    return selected.map(analyticalUuid => {
      const best = closest.get(analyticalUuid);
      if (!best) throw new Error('A selected observation has no finite non-self Euclidean match.');
      if (best.groups.size !== 1) {
        throw new Error('The closest Euclidean matches disagree; choose a destination manually.');
      }
      return { analyticalUuid, groupLabel: [...best.groups][0]! };
    });
  }
  throw new Error('This analysis result cannot recommend assignments.');
}

function safePath(path: string): boolean {
  return !!path.trim() && !path.includes('\\') && !path.includes('\0') && !path.startsWith('/') &&
    !/^[a-zA-Z]:/.test(path) && path.split('/').every(part => part !== '' && part !== '.' && part !== '..');
}

/** Turn explicit label-to-destination choices into an atomic batch transfer. */
export function buildAutomaticAssignmentRequest(
  result: AnalysisResult,
  selectedUuids: readonly string[],
  mappings: readonly AutomaticDestinationMapping[],
  candidates: readonly GroupCandidate[],
  cutK = 2,
): BatchTransferUnitsRequest {
  const recommendations = recommendedAssignments(result, selectedUuids, cutK);
  const sourcePath = result.data.path;
  const sourceRevision = result.data.revision_id;
  if (!safePath(sourcePath) || !sourceRevision.trim()) throw new Error('The analysis result has no safe source path or revision.');
  if (!mappings.length) throw new Error('Choose a destination for each recommended group.');

  const requiredLabels = new Set(recommendations.map(item => item.groupLabel));
  const byLabel = new Map<string, AutomaticDestinationMapping>();
  const paths = new Set<string>();
  for (const mapping of mappings) {
    if (!requiredLabels.has(mapping.groupLabel) || byLabel.has(mapping.groupLabel)) {
      throw new Error('Destination mappings must include each recommended group exactly once.');
    }
    const keepInSource = mapping.destinationPath === sourcePath && mapping.newGroupName === undefined;
    if (!safePath(mapping.destinationPath) || (paths.has(mapping.destinationPath) && !keepInSource) ||
        (mapping.destinationPath === sourcePath && !keepInSource)) {
      throw new Error('Choose distinct, safe destination paths; only Keep in current group may use the source path.');
    }
    byLabel.set(mapping.groupLabel, mapping);
    paths.add(mapping.destinationPath);
  }
  if (byLabel.size !== requiredLabels.size) throw new Error('Choose a destination for every recommended group.');

  const uuidsByLabel = new Map<string, string[]>();
  for (const recommendation of recommendations) {
    const groupUuids = uuidsByLabel.get(recommendation.groupLabel) ?? [];
    groupUuids.push(recommendation.analyticalUuid);
    uuidsByLabel.set(recommendation.groupLabel, groupUuids);
  }
  const targets = mappings.filter(mapping => mapping.destinationPath !== sourcePath).map(mapping => {
    const selected = uuidsByLabel.get(mapping.groupLabel)!;
    const candidate = candidates.find(item => item.path === mapping.destinationPath);
    if (mapping.newGroupName !== undefined) {
      if (!mapping.newGroupName.trim() || candidate) throw new Error('A new group needs a name and a path unused by every group candidate.');
      return {
        destination_path: mapping.destinationPath,
        destination_group_name: mapping.newGroupName.trim(),
        expected_destination_revision: null,
        selected_uuids: selected,
      };
    }
    if (!candidate?.ready || !candidate.group || candidate.group.path !== candidate.path || !candidate.group.revision_id.trim()) {
      throw new Error(`Destination ${mapping.destinationPath} is not a ready group with a revision.`);
    }
    return {
      destination_path: mapping.destinationPath,
      expected_destination_revision: candidate.group.revision_id,
      selected_uuids: selected,
    };
  });
  if (!targets.length) throw new Error('Every selected observation is already in its recommended group.');
  return { source_path: sourcePath, expected_source_revision: sourceRevision, targets };
}
