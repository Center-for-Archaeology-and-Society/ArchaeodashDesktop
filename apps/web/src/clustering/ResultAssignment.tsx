import { AutomaticAssignment } from './AutomaticAssignment.tsx';
import { useMemo, useState, type ReactElement } from 'react';
import type { TransferUnitsRequest, BatchTransferUnitsRequest, GroupCandidate } from '@archaeodash/client';
import type { AnalysisResult } from './AnalysisPage.tsx';
import { buildAssignmentRequest, selectableRows } from './assignment-model.ts';

export function MoveConfirmation({ request, busy, onConfirm, onCancel }: {
  request: TransferUnitsRequest;
  busy: boolean;
  onConfirm: (request: TransferUnitsRequest) => void;
  onCancel: () => void;
}): ReactElement {
  return <section aria-label="Confirm analytical unit move">
    <p>Move {request.selected_uuids.length} selected analytical units from <strong>{request.source_path}</strong> to <strong>{request.destination_path}</strong>?</p>
    <p>The units will leave the source group. Original measured values remain unchanged. An empty source group file is removed.</p>
    <button type="button" disabled={busy} onClick={() => onConfirm(request)}>Confirm move</button>{' '}
    <button type="button" disabled={busy} onClick={onCancel}>Cancel</button>
  </section>;
}

/** UUIDs stay in component state and requests, never checkbox values or labels. */
export function ResultAssignment({ result, destinations, cutK = 2, busy, onConfirm, candidates = [], onBatchConfirm, onReviewChange = () => {} }: {
  result: AnalysisResult;
  destinations: readonly string[];
  candidates?: readonly GroupCandidate[];
  onBatchConfirm?: (request: BatchTransferUnitsRequest) => void;
  onReviewChange?: (reviewing: boolean) => void;
  cutK?: number;
  busy: boolean;
  onConfirm: (request: TransferUnitsRequest) => void;
}): ReactElement | null {
  const rows = useMemo(() => selectableRows(result, cutK), [result, cutK]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [target, setTarget] = useState('');
  const [shown, setShown] = useState(100);
  const [prepared, setPrepared] = useState<TransferUnitsRequest | null>(null);
  const [error, setError] = useState('');
  const [automatic, setAutomatic] = useState(false);
  const [autoReviewing, setAutoReviewing] = useState(false);
  if (result.kind === 'diagnostics') return null;
  const targets = destinations.filter(path => path !== result.data.path);
  function review() {
    try {
      if (!targets.includes(target)) throw new Error('Choose an available destination group.');
      setPrepared(buildAssignmentRequest(result, [...selected], target));
      onReviewChange(true);
      setError('');
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
  }
  return <section aria-label="Assign analysis results">
    <h2>Select analytical units to move</h2>
    <p>{selected.size} of {rows.length} analytical units selected. Repeated nearest matches select the observation once.</p>
    {rows.length === 0 ? <p>No assignable analytical units are available. Recompute the analysis if its identities are unavailable.</p> : <>
      <fieldset disabled={busy || prepared !== null || autoReviewing}>
        <legend>Result selection</legend>
        {onBatchConfirm && <label>Assignment mode <select value={automatic ? 'automatic' : 'manual'} onChange={event => setAutomatic(event.target.value === 'automatic')}><option value="manual">Choose one destination</option><option value="automatic">Use analysis groups</option></select></label>}
        <button type="button" onClick={() => setSelected(new Set(rows.map(row => row.analyticalUuid)))}>Select all {rows.length} analytical units</button>{' '}
        <button type="button" onClick={() => setSelected(new Set())}>Clear selection</button>
        <table className="data-table"><thead><tr><th>Select</th><th>Analytical unit</th><th>Result</th></tr></thead>
          <tbody>{rows.slice(0, shown).map((row, index) => <tr key={row.analyticalUuid}>
            <td><input type="checkbox" aria-label={`Select analytical unit ${index + 1}`} checked={selected.has(row.analyticalUuid)} onChange={event => {
              const next = new Set(selected);
              if (event.target.checked) next.add(row.analyticalUuid); else next.delete(row.analyticalUuid);
              setSelected(next);
            }} /></td><td>{row.label}</td><td>{row.detail}</td>
          </tr>)}</tbody>
        </table>
        {shown < rows.length && <button type="button" onClick={() => setShown(value => value + 100)}>Show 100 more analytical units</button>}
        {!automatic && <><label>Destination group <select value={target} onChange={event => setTarget(event.target.value)}>
          <option value="">Choose an existing group…</option>{targets.map(path => <option key={path}>{path}</option>)}
        </select></label>
        {!targets.length && <p>No other ready group file is available. Import or create a destination group before assigning results.</p>}
        <button type="button" disabled={!selected.size || !targets.includes(target)} onClick={review}>Review move</button></>}
      </fieldset>
      {automatic && onBatchConfirm && <AutomaticAssignment result={result} selected={[...selected]} candidates={candidates} cutK={cutK} busy={busy} onConfirm={onBatchConfirm} onReviewChange={value => { setAutoReviewing(value); onReviewChange(value); }} />}
      {prepared && <MoveConfirmation request={prepared} busy={busy} onConfirm={onConfirm} onCancel={() => { setPrepared(null); onReviewChange(false); }} />}
    </>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
