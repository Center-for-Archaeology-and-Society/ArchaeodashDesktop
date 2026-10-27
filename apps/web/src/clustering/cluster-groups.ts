/**
 * Turning a clustering into groups: every analyzed sample goes to
 * `Cluster 1`, `Cluster 2`, ... (named by the backend; rename them later in
 * the Data Manager), either replacing the analyzed groups in their folder or
 * in a new folder beside it.
 */
import type { ApplyClustersMode, ApplyClustersRequest, ClusterFitResponse, GroupRevisionRef } from '@archaeodash/client';
import { layoutDendrogram } from './dendrogram-model.ts';

/** Each cluster's analytical UUIDs, cluster 1 first (hierarchical trees cut
 *  into `cutK` clusters). */
export function clusterMembers(data: ClusterFitResponse, cutK: number): string[][] {
  const ids = data.analytical_uuids;
  if (!Array.isArray(ids) || ids.length !== data.n_rows || new Set(ids).size !== ids.length) {
    throw new Error('This result has no sample identities; run the analysis again.');
  }
  let labels: number[];
  if (Array.isArray(data.cluster)) {
    labels = data.cluster;
  } else {
    const layout = layoutDendrogram(data, cutK);
    if (!layout) throw new Error('The clustering cannot be cut into the requested number of clusters.');
    labels = layout.clusterOfRow.map((group) => group + 1);
  }
  if (labels.length !== ids.length || labels.some((label) => !Number.isInteger(label) || label < 1)) {
    throw new Error('The clustering has incomplete cluster labels; run the analysis again.');
  }
  const clusters: string[][] = Array.from({ length: Math.max(...labels) }, () => []);
  labels.forEach((label, i) => clusters[label - 1]!.push(ids[i]!));
  return clusters;
}

/** The request that turns every analyzed sample into cluster groups. */
export function buildApplyClustersRequest(
  data: ClusterFitResponse,
  cutK: number,
  sources: readonly GroupRevisionRef[],
  mode: ApplyClustersMode,
): ApplyClustersRequest {
  if (sources.length === 0 || sources.some((s) => !s.path.trim() || !s.expected_revision.trim())) {
    throw new Error('The analyzed groups are not loaded; reload the dataset.');
  }
  return { sources: sources.map((s) => ({ ...s })), clusters: clusterMembers(data, cutK), mode };
}

/** Folder part of a project-relative group path (either separator). */
export function folderOfPath(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut < 0 ? '' : path.slice(0, cut);
}
