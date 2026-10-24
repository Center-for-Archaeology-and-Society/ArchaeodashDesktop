/**
 * Import destination: the folder the new group files go to and, when that
 * folder already has groups, the required choice between new groups,
 * replacing the groups imported from this same file, or adding the rows to
 * the groups of the same names (the legacy "Add rows" load mode).
 */
import { useId, type ReactElement } from 'react';
import { newGroupsBlocked, type DestinationChoice, type ImportConflicts } from './import-request.ts';

export interface ImportDestinationProps {
  readonly folder: string;
  readonly conflicts: ImportConflicts;
  readonly choice: DestinationChoice;
  /** Free folder name offered by "Use a new folder". */
  readonly freeFolder: string;
  /** Existing group folders, suggested while typing. */
  readonly folders?: readonly string[];
  readonly disabled?: boolean;
  readonly onFolderChange: (folder: string) => void;
  readonly onChoice: (choice: DestinationChoice) => void;
  readonly onCancel?: () => void;
}

const plural = (count: number, noun: string) => `${count} ${noun}${count === 1 ? '' : 's'}`;
const quoted = (text: string) => `“${text}”`;

export function ImportDestination({ folder, conflicts, choice, freeFolder, folders = [], disabled = false, onFolderChange, onChoice, onCancel }: ImportDestinationProps): ReactElement {
  const listId = useId();
  const existing = conflicts.inFolder.length;
  const blocked = newGroupsBlocked(conflicts);
  const radio = (value: DestinationChoice, label: string, enabled = true) => (
    <label>
      <input type="radio" name="import-destination-choice" checked={choice === value} disabled={disabled || !enabled} onChange={() => onChoice(value)} /> {label}
    </label>
  );
  return (
    <div className="import-destination">
      <label>Destination folder <input value={folder} list={listId} disabled={disabled} onChange={(event) => onFolderChange(event.target.value)} /></label>
      <datalist id={listId}>{[...new Set(folders)].filter(Boolean).sort().map((name) => <option key={name} value={name} />)}</datalist>
      {conflicts.sameSourceElsewhere.length > 0 && (
        <p className="import-destination-note">This file was already imported into {conflicts.sameSourceElsewhere.map((f) => quoted(f || 'Project root')).join(', ')}. Importing it again elsewhere makes separate copies of its samples.</p>
      )}
      {existing > 0 && (
        <fieldset className="import-destination-choice">
          <legend>The folder {quoted(folder)} already has {plural(existing, 'group')}. What should this import do?</legend>
          {radio('new', 'Import as new groups')}
          {radio('replace', conflicts.sameSource.length
            ? `Replace the ${plural(conflicts.sameSource.length, 'group')} imported from this file`
            : 'Replace the groups imported from this file (none in this folder)', conflicts.sameSource.length > 0)}
          {radio('append', conflicts.sameSource.length
            ? 'Add the rows to the groups with the same names (not available: this file is already in the folder)'
            : `Add the rows to the groups with the same names (${plural(conflicts.matchingNames.length, 'matching group')}; others become new groups)`, conflicts.sameSource.length === 0)}
          {choice === '' && <p className="import-destination-hint">Choose one to continue.</p>}
          {choice === 'new' && blocked && (
            <div className="import-destination-blocked" role="alert">
              <p>
                {conflicts.sameSource.length > 0
                  ? `This file's groups are already in ${quoted(folder)} (${conflicts.sameSource.map((g) => g.group_name).join(', ')}); importing them again would hold the same samples twice.`
                  : `${quoted(folder)} already has group files named ${conflicts.clashes.join(', ')}.`}
              </p>
              <div className="import-destination-actions">
                <button type="button" disabled={disabled} onClick={() => { onFolderChange(freeFolder); onChoice(''); }}>Use a new folder {quoted(freeFolder)}</button>
                {onCancel && <button type="button" disabled={disabled} onClick={onCancel}>Cancel import</button>}
              </div>
            </div>
          )}
          {choice === 'replace' && (
            <p className="import-destination-hint">The replaced groups keep their samples&apos; identities, so copies of them in other folders stay matched. Groups no longer produced by the new grouping are removed.</p>
          )}
          {choice === 'append' && (
            <p className="import-destination-hint">Descriptive columns are merged across the folder; measured columns must match each group (a numeric column the group measures is used even if unchecked).</p>
          )}
        </fieldset>
      )}
    </div>
  );
}

export interface DescriptiveColumnsProps {
  readonly choices: readonly string[];
  readonly dropped: readonly string[];
  readonly disabled?: boolean;
  readonly onChange: (dropped: string[]) => void;
}

/** "Keep as descriptive" checklist (all kept by default). */
export function DescriptiveColumns({ choices, dropped, disabled = false, onChange }: DescriptiveColumnsProps): ReactElement {
  return (
    <fieldset className="import-wizard-columns import-wizard-descriptive">
      <legend>Keep as descriptive</legend>
      {choices.map((column) => (
        <label key={column}>
          <input type="checkbox" checked={!dropped.includes(column)} disabled={disabled} onChange={(event) => onChange(event.target.checked ? dropped.filter((c) => c !== column) : [...dropped, column])} /> {column}
        </label>
      ))}
      {!choices.length && <p>No other columns.</p>}
      <p>Unchecked columns are left out of the imported group files.</p>
    </fieldset>
  );
}
