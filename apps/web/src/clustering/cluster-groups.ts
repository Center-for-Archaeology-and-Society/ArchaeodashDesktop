/**
 * Turning a clustering into groups: every analyzed sample goes to
 * `Cluster 1`, `Cluster 2`, ... (or `<prefix>1`, ...; named by the backend,
 * renamed later in the Data Manager), either replacing the analyzed groups in
 * their folder or in a new folder beside it.
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

/** Text before each cluster number in new group names: `Cluster 1`, ... */
export const DEFAULT_CLUSTER_NAME_PREFIX = 'Cluster ';

const WINDOWS_RESERVED = /^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])$/i;

/**
 * Why `prefix` cannot start the cluster group names, or null when it can.
 * Mirrors the backend (`cluster_name_prefix`): leading spaces are dropped; the
 * names must not be empty, hidden, longer than 100 characters, contain path
 * separators or characters invalid in file names, or give a Windows-reserved
 * file (`COM1`).
 */
export function clusterNamePrefixProblem(prefix: string): string | null {
  const text = prefix.trimStart();
  if (!text) return 'Enter the text that goes before each cluster number.';
  if ([...text].length > 100) return 'The name prefix is longer than 100 characters.';
  if (text.startsWith('.')) return 'The name prefix cannot start with a dot.';
  if (/[<>:"/\\|?*\u0000-\u001f\u007f]/.test(text)) return 'The name prefix cannot contain < > : " / \\ | ? * or control characters.';
  const file = `${text}1`.replace(/[^\p{L}\p{N}._-]/gu, '_');
  if (WINDOWS_RESERVED.test(file.split('.')[0] ?? '')) return 'The name prefix gives group files that Windows reserves.';
  return null;
}

/** Shown names of `count` cluster groups: `Cluster 1` or `Cluster 1 to Cluster 3`. */
export function clusterGroupNames(prefix: string, count: number): string {
  const text = prefix.trimStart();
  return count === 1 ? `${text}1` : `${text}1 to ${text}${count}`;
}

/** The request that turns every analyzed sample into cluster groups named
 *  `<namePrefix>1`, `<namePrefix>2`, ... */
export function buildApplyClustersRequest(
  data: ClusterFitResponse,
  cutK: number,
  sources: readonly GroupRevisionRef[],
  mode: ApplyClustersMode,
  namePrefix: string = DEFAULT_CLUSTER_NAME_PREFIX,
): ApplyClustersRequest {
  if (sources.length === 0 || sources.some((s) => !s.path.trim() || !s.expected_revision.trim())) {
    throw new Error('The analyzed groups are not loaded; reload the dataset.');
  }
  const problem = clusterNamePrefixProblem(namePrefix);
  if (problem) throw new Error(problem);
  return {
    sources: sources.map((s) => ({ ...s })), clusters: clusterMembers(data, cutK), mode,
    name_prefix: namePrefix.trimStart(),
  };
}

/** Folder part of a project-relative group path (either separator). */
export function folderOfPath(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut < 0 ? '' : path.slice(0, cut);
}
