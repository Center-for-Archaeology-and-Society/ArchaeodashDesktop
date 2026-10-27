import { AutomaticAssignment } from './AutomaticAssignment.tsx';
import { useMemo, useState, type ReactElement } from 'react';
import type { ApplyClustersRequest, TransferUnitsRequest, BatchTransferUnitsRequest, GroupCandidate } from '@archaeodash/client';
import { ClusterGroupsAssignment, type ClusterGroupSources } from './ClusterGroups.tsx';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { buildAssignmentRequest, nearestMatchKeys, selectableRows, selectedUnits } from './assignment-model.ts';
import { newGroupPath, newGroupProblem } from './new-group.ts';
import { BusyLabel } from '../ui/Busy.tsx';

/** Destination value for "Create a new group…". */
export const NEW_GROUP = '__new__';

export function MoveConfirmation({ request, busy, onConfirm, onCancel, multiSource = false, groupName = (path: string) => path }: {
  request: TransferUnitsRequest;
  busy: boolean;
  onConfirm: (request: TransferUnitsRequest) => void;
  onCancel: () => void;
  multiSource?: boolean;
  groupName?: (path: string) => string;
}): ReactElement {
  const destination = request.destination_group_name ? `the new group ${request.destination_group_name} (${request.destination_path})` : groupName(request.destination_path);
  return <section aria-label="Confirm analytical unit move">
    <p>Move {request.selected_uuids.length} selected analytical units from <strong>{multiSource ? 'their current groups' : groupName(request.source_path)}</strong> to <strong>{destination}</strong>?{multiSource ? ' Units already in that group stay put.' : ''}</p>
    <p>The units will leave the source group. Original measured values remain unchanged. An empty source group file is removed.</p>
    <button type="button" disabled={busy} onClick={() => onConfirm(request)}><BusyLabel busy={busy} busyText="Moving…">Confirm move</BusyLabel></button>{' '}
    <button type="button" disabled={busy} onClick={onCancel}>Cancel</button>
  </section>;
}

/**
 * Assignment controls for the rows checked in the results table. Selection
 * keys (UUIDs, or Euclidean match-row keys) stay in state and requests,
 * never in checkbox values or labels.
 */
/** How the checked samples are assigned. */
type AssignMode = 'manual' | 'automatic' | 'replace' | 'new_folder';

export function ResultAssignment({ result, destinations, cutK = 2, busy, onConfirm, candidates = [], onBatchConfirm, onReviewChange = () => {}, multiSource = false, rowLabel, selected: selectedProp, onSelectedChange, clusterSources, onApplyClusters }: {
  result: AnalysisResult;
  /** Sample label (ANID) by hidden identity and input row index. */
  rowLabel?: (uuid: string | undefined, index: number) => string;
  /** The result spans several group files; moves are split per source. */
  multiSource?: boolean;
  destinations: readonly string[];
  candidates?: readonly GroupCandidate[];
  onBatchConfirm?: (request: BatchTransferUnitsRequest) => void;
  onReviewChange?: (reviewing: boolean) => void;
  cutK?: number;
  busy: boolean;
  onConfirm: (request: TransferUnitsRequest) => void;
  /** Row keys checked in the results table (owned by the caller). */
  selected?: ReadonlySet<string>;
  onSelectedChange?: (next: Set<string>) => void;
  /** Cluster results: the analyzed groups, for turning clusters into groups. */
  clusterSources?: ClusterGroupSources;
  onApplyClusters?: (request: ApplyClustersRequest) => void;
}): ReactElement | null {
  const rows = useMemo(() => selectableRows(result, cutK, rowLabel), [result, cutK, rowLabel]);
  const [ownSelected, setOwnSelected] = useState<Set<string>>(() => new Set());
  const selected = selectedProp ?? ownSelected;
  const setSelected = onSelectedChange ?? setOwnSelected;
  const [target, setTarget] = useState('');
  const [newName, setNewName] = useState('');
  const [newPath, setNewPath] = useState<string | null>(null);
  const [prepared, setPrepared] = useState<TransferUnitsRequest | null>(null);
  const [error, setError] = useState('');
  const [mode, setMode] = useState<AssignMode>('manual');
  const [autoReviewing, setAutoReviewing] = useState(false);
  const keys = useMemo(() => {
    const valid = new Set(rows.map(row => row.key));
    return [...selected].filter(key => valid.has(key));
  }, [rows, selected]);
  if (result.kind === 'diagnostics') return null;
  // Cluster results become groups (replacing the analyzed ones, or in a new
  // folder); other results map their labels onto existing groups.
  const clusterGroups = result.kind === 'fit' && !!clusterSources && !!onApplyClusters;
  const automatic = mode === 'automatic' && !clusterGroups;
  const asClusterGroups = clusterGroups && (mode === 'replace' || mode === 'new_folder');
  const manual = !automatic && !asClusterGroups;
  const units = selectedUnits(rows, keys);
  const euclidean = result.kind === 'euclidean';
  const targets = destinations.filter(path => multiSource || path !== result.data.path);
  const groupName = (path: string) => candidates.find(c => c.path === path)?.group?.group_name ?? path;
  const creating = target === NEW_GROUP;
  const path = newPath ?? newGroupPath(result.data.path, newName);
  const newProblem = creating ? newGroupProblem(newName, path, candidates) : null;
  const sampleCount = new Set(rows.map(row => row.analyticalUuid)).size;
  function review() {
    try {
      if (creating) {
        if (newProblem) throw new Error(newProblem);
        setPrepared(buildAssignmentRequest(result, keys, path, newName.trim(), multiSource, cutK));
      } else {
        if (!targets.includes(target)) throw new Error('Choose an available destination group.');
        setPrepared(buildAssignmentRequest(result, keys, target, undefined, multiSource, cutK));
      }
      onReviewChange(true);
      setError('');
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
  }
  return <section className="card" aria-label="Assign analysis results">
    <header className="card-header"><h2 className="card-title">Assign samples to groups</h2></header>
    <p className="card-note">{euclidean
      ? `${keys.length} match rows checked (${units.length} of ${sampleCount} samples). Check rows in the results table; each checked match can supply its group.`
      : `${units.length} of ${rows.length} samples selected. Check rows in the results table above.`}</p>
    {rows.length === 0 ? <p>No assignable analytical units are available. Recompute the analysis if its identities are unavailable.</p> : <>
      <fieldset disabled={busy || prepared !== null || autoReviewing}>
        <legend>Result selection</legend>
        {clusterGroups
          ? <label>Assignment mode <select value={manual ? 'manual' : mode} onChange={event => setMode(event.target.value as AssignMode)}>
            <option value="manual">Move checked samples to one group</option>
            <option value="replace">Replace the analyzed groups with the clusters</option>
            <option value="new_folder">Create a new folder of cluster groups</option>
          </select></label>
          : onBatchConfirm && <label>Assignment mode <select value={automatic ? 'automatic' : 'manual'} onChange={event => setMode(event.target.value === 'automatic' ? 'automatic' : 'manual')}>
            <option value="manual">Choose one destination</option><option value="automatic">{euclidean ? 'Use checked match groups' : 'Use analysis groups'}</option>
          </select></label>}
        {asClusterGroups ? null : euclidean
          ? <button type="button" onClick={() => setSelected(new Set(nearestMatchKeys(result.data.rows)))}>Check the nearest match of every sample</button>
          : <button type="button" onClick={() => setSelected(new Set(rows.map(row => row.key)))}>Select all {rows.length} analytical units</button>}{' '}
        {!asClusterGroups && <button type="button" onClick={() => setSelected(new Set())}>Clear selection</button>}
        {manual && <><label>Destination group <select value={target} onChange={event => { setTarget(event.target.value); setError(''); }}>
          <option value="">Choose a group…</option>{targets.map(path => <option key={path} value={path}>{groupName(path)}</option>)}
          <option value={NEW_GROUP}>Create a new group…</option>
        </select></label>
        {creating && <>
          <label>New group name <input value={newName} onChange={event => setNewName(event.target.value)} /></label>
          <label>New group file <input value={path} onChange={event => setNewPath(event.target.value)} /></label>
          {newName.trim() !== '' && newProblem && <p role="alert">{newProblem}</p>}
        </>}
        {!targets.length && !creating && <p>No other ready group file is available. Create a new group to assign these results.</p>}
        <button type="button" disabled={!units.length || (creating ? !!newProblem : !targets.includes(target))} onClick={review}>Review move</button></>}
      </fieldset>
      {asClusterGroups && result.kind === 'fit' && clusterSources && onApplyClusters && <ClusterGroupsAssignment
        key={mode} data={result.data} cutK={cutK} mode={mode === 'replace' ? 'replace' : 'new_folder'} sources={clusterSources} busy={busy}
        onApply={onApplyClusters} onReviewChange={value => { setAutoReviewing(value); onReviewChange(value); }} />}
      {automatic && onBatchConfirm && <AutomaticAssignment multiSource={multiSource} result={result} selected={keys} candidates={candidates} cutK={cutK} busy={busy} onConfirm={onBatchConfirm} onReviewChange={value => { setAutoReviewing(value); onReviewChange(value); }} />}
      {prepared && <MoveConfirmation request={prepared} busy={busy} multiSource={multiSource} groupName={groupName} onConfirm={onConfirm} onCancel={() => { setPrepared(null); onReviewChange(false); }} />}
    </>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
