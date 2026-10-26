import type { BatchTransferUnitsRequest, GroupCandidate } from '@archaeodash/client';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { euclideanMatchKeys, selectableRows, selectedUnits } from './assignment-model.ts';
import { layoutDendrogram } from './dendrogram-model.ts';
import { newGroupProblem, safePath } from './new-group.ts';

export interface AssignmentRecommendation {
  analyticalUuid: string;
  groupLabel: string;
}

/** Destination value meaning "leave these units in their own group". */
export const KEEP_IN_SOURCE = '__keep__';

export interface AutomaticDestinationMapping {
  groupLabel: string;
  destinationPath: string;
  newGroupName?: string;
}

const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
const isIdentity = (value: unknown): value is string =>
  typeof value === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);

/** The selected units (distinct, in selection order) after validating every key. */
function selectedIds(result: AnalysisResult, selectedKeys: readonly string[], cutK: number): string[] {
  const rows = selectableRows(result, cutK);
  const valid = new Set(rows.map(row => row.key));
  const keys = [...new Set(selectedKeys)];
  if (!keys.length || keys.some(key => !valid.has(key))) {
    throw new Error('Selection must contain valid analytical units from this result.');
  }
  return selectedUnits(rows, keys);
}

/** Recommend a destination label for each selected analytical identity. */
export function recommendedAssignments(
  result: AnalysisResult,
  selectedKeys: readonly string[],
  cutK = 2,
): AssignmentRecommendation[] {
  const selected = selectedIds(result, selectedKeys, cutK);
  if (result.kind === 'fit') {
    const { data } = result;
    const clusterByUuid = new Map<string, number>();
    if (Array.isArray(data.cluster)) {
      data.analytical_uuids.forEach((uuid, i) => clusterByUuid.set(uuid, data.cluster![i]!));
    } else {
      const layout = layoutDendrogram(data, cutK);
      if (!layout) throw new Error('The clustering result cannot be cut into the requested number of clusters.');
      data.analytical_uuids.forEach((uuid, i) => {
        const group = layout.clusterOfRow[i];
        if (group !== undefined) clusterByUuid.set(uuid, group + 1);
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
    // Each checked match row assigns its sample to that match's group, so
    // a later match (not only the nearest) can be chosen.
    const rows = result.data.rows;
    const keys = euclideanMatchKeys(rows);
    const chosen = new Set(selectedKeys);
    const groupOf = new Map<string, string>();
    rows.forEach((row, i) => {
      const key = keys[i];
      if (!key || !chosen.has(key)) return;
      if (row.match_analytical_uuid === row.analytical_uuid || !finite(row.distance) ||
          !isIdentity(row.match_analytical_uuid) || typeof row.match_group !== 'string' || !row.match_group.trim()) {
        throw new Error('A checked Euclidean match has incomplete identity, distance, or group data.');
      }
      const current = groupOf.get(row.analytical_uuid);
      if (current !== undefined && current !== row.match_group) {
        throw new Error('Checked matches for one sample name different groups; check one match per sample.');
      }
      groupOf.set(row.analytical_uuid, row.match_group);
    });
    return selected.map(analyticalUuid => ({ analyticalUuid, groupLabel: groupOf.get(analyticalUuid)! }));
  }
  throw new Error('This analysis result cannot recommend assignments.');
}

/** Turn explicit label-to-destination choices into an atomic batch transfer. */
export function buildAutomaticAssignmentRequest(
  result: AnalysisResult,
  selectedKeys: readonly string[],
  mappings: readonly AutomaticDestinationMapping[],
  candidates: readonly GroupCandidate[],
  cutK = 2,
  multiSource = false,
): BatchTransferUnitsRequest {
  const recommendations = recommendedAssignments(result, selectedKeys, cutK);
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
    // Several source files: "keep" is a sentinel (each unit stays in its own
    // file) and every real group, the first file included, is a destination.
    const keepInSource = multiSource
      ? mapping.destinationPath === KEEP_IN_SOURCE
      : mapping.destinationPath === sourcePath && mapping.newGroupName === undefined;
    if ((!keepInSource && !safePath(mapping.destinationPath)) || (paths.has(mapping.destinationPath) && !keepInSource) ||
        (!multiSource && mapping.destinationPath === sourcePath && !keepInSource)) {
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
  const keep = multiSource ? KEEP_IN_SOURCE : sourcePath;
  const targets = mappings.filter(mapping => mapping.destinationPath !== keep).map(mapping => {
    const selected = uuidsByLabel.get(mapping.groupLabel)!;
    const candidate = candidates.find(item => item.path === mapping.destinationPath);
    if (mapping.newGroupName !== undefined) {
      if (!mapping.newGroupName.trim() || candidate) throw new Error('A new group needs a name and a path unused by every group candidate.');
      const problem = newGroupProblem(mapping.newGroupName, mapping.destinationPath, candidates);
      if (problem) throw new Error(problem);
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
