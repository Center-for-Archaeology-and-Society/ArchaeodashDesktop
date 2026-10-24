/**
 * Membership group controls (GroupMembership.R): the group-size table and
 * the eligible-group picker. Only eligible groups can be chosen; with none
 * eligible the analysis falls back to every group, as the legacy app did.
 */
import type { ReactElement } from 'react';
import type { GroupEligibility } from './input-options.ts';
import { TableCard } from '../ui/TableExport.tsx';

export function GroupSizesTable({ groups, column }: { groups: readonly GroupEligibility[]; column: string }): ReactElement {
  const sheet = { name: 'Group sizes', columns: ['Group', 'Count', 'Eligible'], rows: groups.map(g => [g.group, g.count, g.eligible ? 'Yes' : 'No']) };
  return <TableCard title="Group sizes" fileName={`Group sizes - ${column}`} sheets={() => [sheet]} note={`Samples per ${column} group.`}>
    {groups.length === 0 ? <p className="empty-note">No groups in this column.</p> : <table className="data-table is-compact">
      <thead><tr>{sheet.columns.map(c => <th key={c}>{c}</th>)}</tr></thead>
      <tbody>{groups.map(g => <tr key={g.group}><td>{g.group}</td><td className="num">{g.count}</td><td>{g.eligible ? 'Yes' : 'No'}</td></tr>)}</tbody>
    </table>}
  </TableCard>;
}

export function EligibleGroupPicker({ groups, selected, onChange, featureCount, disabled = false }: {
  groups: readonly GroupEligibility[];
  /** Chosen groups; null means every eligible group (the default). */
  selected: readonly string[] | null;
  onChange: (next: string[]) => void;
  featureCount: number;
  disabled?: boolean;
}): ReactElement {
  const eligible = groups.filter(g => g.eligible).map(g => g.group);
  const chosen = new Set(selected ?? eligible);
  const threshold = Math.max(featureCount, groups.length) + 1;
  return <fieldset className="field field-wide eligible-groups" disabled={disabled}>
    <legend>Eligible groups</legend>
    <p className="field-hint">A group is eligible with more than {threshold} samples (max of {featureCount} analysis features and {groups.length} groups, plus one).</p>
    {eligible.length === 0
      ? <p role="status" className="callout callout-info">No group has enough samples to be eligible; membership will use all groups.</p>
      : <div className="eligible-groups-actions">
        <button type="button" className="btn btn-sm" onClick={() => onChange(eligible)}>All eligible</button>
        <button type="button" className="btn btn-sm" onClick={() => onChange([])}>None</button>
      </div>}
    <div className="eligible-groups-list">
      {groups.map(g => <label key={g.group} className={`field-check${g.eligible ? '' : ' is-ineligible'}`}>
        <input type="checkbox" disabled={!g.eligible} checked={g.eligible && chosen.has(g.group)}
          onChange={event => onChange(eligible.filter(name => (name === g.group ? event.target.checked : chosen.has(name))))} />
        {g.group} <span className="muted">(n = {g.count}{g.eligible ? '' : ', too few'})</span>
      </label>)}
    </div>
    {eligible.length > 0 && eligible.every(name => !chosen.has(name)) && <p className="field-hint">No group chosen: every eligible group is used.</p>}
  </fieldset>;
}
