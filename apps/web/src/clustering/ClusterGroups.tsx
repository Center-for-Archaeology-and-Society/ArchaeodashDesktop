import { useMemo, useState, type ReactElement } from 'react';
import type { ApplyClustersMode, ApplyClustersRequest, ClusterFitResponse, GroupRevisionRef } from '@archaeodash/client';
import { buildApplyClustersRequest, clusterMembers, folderOfPath } from './cluster-groups.ts';
import { BusyLabel } from '../ui/Busy.tsx';

/** The analyzed group files a clustering can be turned into groups from. */
export interface ClusterGroupSources {
  readonly refs: readonly GroupRevisionRef[];
  /** Group names of `refs`, in order. */
  readonly names: readonly string[];
}

const folderLabel = (folder: string) => {
  const name = folder.split(/[\\/]/).pop() ?? '';
  return name ? `“${name}”` : 'the project folder';
};

/**
 * Turns every sample of a cluster result into groups named Cluster 1,
 * Cluster 2, ...: replacing the analyzed groups in their folder, or in a new
 * folder beside it. Names can be changed later in the Data Manager.
 */
export function ClusterGroupsAssignment({ data, cutK, mode, sources, busy, onApply, onReviewChange }: {
  data: ClusterFitResponse;
  cutK: number;
  mode: ApplyClustersMode;
  sources: ClusterGroupSources;
  busy: boolean;
  onApply: (request: ApplyClustersRequest) => void;
  onReviewChange: (reviewing: boolean) => void;
}): ReactElement {
  const [prepared, setPrepared] = useState<ApplyClustersRequest | null>(null);
  const [error, setError] = useState('');
  const plan = useMemo(() => {
    try {
      const clusters = clusterMembers(data, cutK).filter((members) => members.length > 0);
      return { clusters: clusters.length, samples: clusters.reduce((n, members) => n + members.length, 0), error: '' };
    } catch (e) {
      return { clusters: 0, samples: 0, error: e instanceof Error ? e.message : String(e) };
    }
  }, [data, cutK]);
  const folder = folderOfPath(sources.refs[0]?.path ?? '');
  const groups = sources.names.map((name) => `“${name}”`).join(', ');
  const names = plan.clusters === 1 ? 'Cluster 1' : `Cluster 1 to Cluster ${plan.clusters}`;
  const description = mode === 'replace'
    ? `All ${plan.samples.toLocaleString()} samples become ${plan.clusters} groups named ${names} in ${folderLabel(folder)}, replacing the analyzed ${sources.names.length === 1 ? 'group' : 'groups'} ${groups}. Other groups in the folder stay as they are.`
    : `All ${plan.samples.toLocaleString()} samples are copied into ${plan.clusters} groups named ${names} in a new folder beside ${folderLabel(folder)}. The analyzed groups stay as they are.`;
  function review() {
    try {
      setPrepared(buildApplyClustersRequest(data, cutK, sources.refs, mode));
      onReviewChange(true);
      setError('');
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
  }
  return <section aria-label={mode === 'replace' ? 'Replace groups with clusters' : 'New folder of cluster groups'}>
    <p>{plan.error || description} Rename the new groups later in the Data Manager.</p>
    {!prepared && <button type="button" disabled={busy || !!plan.error || plan.clusters === 0} onClick={review}>Review</button>}
    {prepared && <div role="group" aria-label="Confirm cluster groups">
      <p><strong>{mode === 'replace'
        ? `Replace ${groups} with ${plan.clusters} cluster groups?`
        : `Create a new folder with ${plan.clusters} cluster groups?`}</strong> Original measured values remain unchanged.</p>
      <button type="button" className="btn btn-primary" disabled={busy} onClick={() => onApply(prepared)}>
        <BusyLabel busy={busy} busyText="Creating groups…">{mode === 'replace' ? 'Replace groups' : 'Create folder'}</BusyLabel>
      </button>{' '}
      <button type="button" disabled={busy} onClick={() => { setPrepared(null); onReviewChange(false); }}>Cancel</button>
    </div>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
