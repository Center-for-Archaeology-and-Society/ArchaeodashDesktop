import { useMemo, useState, type ReactElement } from 'react';
import type { BatchTransferUnitsRequest, GroupCandidate } from '@archaeodash/client';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { KEEP_IN_SOURCE, buildAutomaticAssignmentRequest, recommendedAssignments } from './automatic-assignment.ts';
import { BusyLabel } from '../ui/Busy.tsx';
import { TableExport } from '../ui/TableExport.tsx';

type Mapping = { groupLabel: string; destinationPath: string; newGroupName?: string };
export function BatchConfirmation({ request, selectedCount, busy, onConfirm, onCancel, multiSource = false }: {
  request: BatchTransferUnitsRequest; selectedCount: number; busy: boolean;
  onConfirm: (request: BatchTransferUnitsRequest) => void; onCancel: () => void; multiSource?: boolean;
}): ReactElement {
  const moving = request.targets.reduce((sum, target) => sum + target.selected_uuids.length, 0);
  return <section aria-label="Confirm automatic assignment">
    <h3>Review group assignments</h3>
    {multiSource
      ? <p>Move up to {moving} analytical units from the selected groups, one transaction per source group; units already in their destination stay put. {selectedCount - moving} selected units stay in their own group.</p>
      : <p>Move {moving} analytical units from {request.source_path} in one transaction. {selectedCount - moving} selected units stay in the source group.</p>}
    <TableExport fileName="Planned assignments" sheets={() => [{ name: 'Planned assignments', columns: ['Destination', 'Group', 'Units'], rows: request.targets.map(t => [t.destination_path, t.destination_group_name ?? 'Existing group', t.selected_uuids.length]) }]} />
    <table className="data-table"><thead><tr><th>Destination</th><th>Group</th><th>Units</th></tr></thead>
      <tbody>{request.targets.map(target => <tr key={target.destination_path}><td>{target.destination_path}</td><td>{target.destination_group_name ?? 'Existing group'}</td><td>{target.selected_uuids.length}</td></tr>)}</tbody>
    </table>
    <p>Original measured values remain unchanged. An empty source group file is removed.</p>
    <button type="button" className="btn btn-primary" disabled={busy} onClick={() => onConfirm(request)}><BusyLabel busy={busy} busyText="Assigning…">Confirm assignments</BusyLabel></button>{' '}
    <button type="button" disabled={busy} onClick={onCancel}>Cancel</button>
  </section>;
}

export function AutomaticAssignment({ result, selected, candidates, cutK, busy, onConfirm, onReviewChange, multiSource = false }: {
  result: AnalysisResult; selected: readonly string[]; candidates: readonly GroupCandidate[]; cutK: number; busy: boolean;
  onConfirm: (request: BatchTransferUnitsRequest) => void; onReviewChange: (reviewing: boolean) => void;
  /** The result spans several group files (see buildAutomaticAssignmentRequest). */
  multiSource?: boolean;
}): ReactElement {
  const [choices, setChoices] = useState<Record<string, Mapping>>({});
  const [prepared, setPrepared] = useState<BatchTransferUnitsRequest | null>(null);
  const [error, setError] = useState('');
  const plan = useMemo(() => {
    if (!selected.length) return { labels: [] as string[], error: '' };
    try { return { labels: [...new Set(recommendedAssignments(result, selected, cutK).map(row => row.groupLabel))], error: '' }; }
    catch (e) { return { labels: [] as string[], error: e instanceof Error ? e.message : String(e) }; }
  }, [result, selected, cutK]);
  const defaults = (label: string): Mapping => {
    const matching = candidates.filter(candidate => candidate.ready && candidate.group?.group_name === label);
    return { groupLabel: label, destinationPath: matching.length === 1 ? matching[0]!.path : '' };
  };
  const choice = (label: string): Mapping => Object.hasOwn(choices, label) ? choices[label]! : defaults(label);
  const update = (label: string, mapping: Mapping) => { setChoices(current => ({ ...current, [label]: mapping })); setError(''); };
  function review() {
    try {
      const request = buildAutomaticAssignmentRequest(result, selected, plan.labels.map(choice), candidates, cutK, multiSource);
      setPrepared(request); onReviewChange(true); setError('');
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
  }
  return <section aria-label="Automatic group assignments">
    <h3>{result.kind === 'fit' ? 'Record selected cluster assignments' : result.kind === 'membership' ? 'Assign to best groups' : 'Assign to nearest matched groups'}</h3>
    <p>Review where each result group will be stored. No files change until you confirm.</p>
    {result.kind === 'euclidean' && <p>The nearest finite-distance match determines the group. Equal-distance matches in different groups require manual assignment.</p>}
    {!selected.length && <p>Select analytical units above to prepare assignments.</p>}
    {plan.error && <p role="alert">{plan.error}</p>}
    <fieldset disabled={busy || prepared !== null}><legend>Destination mapping</legend>
      {plan.labels.map((label, index) => {
        const mapping = choice(label);
        const isNew = mapping.newGroupName !== undefined;
        return <div key={label}>
          <label>{label} destination <select value={isNew ? '__new__' : mapping.destinationPath} onChange={event => {
            if (event.target.value === '__new__') {
              // Beside the analyzed groups (Windows paths use backslashes).
              const cut = Math.max(result.data.path.lastIndexOf('/'), result.data.path.lastIndexOf('\\'));
              const folder = result.data.path.slice(0, cut + 1);
              update(label, { groupLabel: label, destinationPath: `${folder}${label.replace(/[^A-Za-z0-9._-]/g, '_') || `group_${index + 1}`}.parquet`, newGroupName: label });
            } else update(label, { groupLabel: label, destinationPath: event.target.value });
          }}>
            <option value="">Choose a destination…</option>
            <option value={multiSource ? KEEP_IN_SOURCE : result.data.path}>Keep in current group</option>
            {candidates.filter(candidate => candidate.ready && (multiSource || candidate.path !== result.data.path)).map(candidate => <option key={candidate.path} value={candidate.path}>{candidate.group?.group_name ?? candidate.path}</option>)}
            <option value="__new__">Create a new group</option>
          </select></label>
          {isNew && <>
            <label>New group name <input value={mapping.newGroupName} onChange={event => update(label, { ...mapping, newGroupName: event.target.value })} /></label>
            <label>New group file <input value={mapping.destinationPath} onChange={event => update(label, { ...mapping, destinationPath: event.target.value })} /></label>
          </>}
        </div>;
      })}
      <button type="button" disabled={!plan.labels.length || !!plan.error} onClick={review}>Review assignments</button>
    </fieldset>
    {error && <p role="alert">{error}</p>}
    {prepared && <BatchConfirmation request={prepared} selectedCount={selected.length} busy={busy} multiSource={multiSource} onConfirm={onConfirm} onCancel={() => { setPrepared(null); onReviewChange(false); }} />}
  </section>;
}
