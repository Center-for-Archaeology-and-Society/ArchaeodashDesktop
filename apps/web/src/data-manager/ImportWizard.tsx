/**
 * Import wizard (choose source file → preview → grouping → ID column →
 * measured columns → import). Launched from the Data Manager sidebar inside
 * a modal dialog; the caller refreshes the file tree via `onImported`.
 */
import { useState, type ReactElement } from 'react';
import type { ImportPreviewResponse, StagedFile } from '@archaeodash/contracts';
import type { DataManagerTransport } from './types.ts';

export type ImportCommitResult = Awaited<ReturnType<DataManagerTransport['imports']['commit']>>;

export interface ImportWizardProps {
  readonly transport: DataManagerTransport;
  readonly onImported?: (result: ImportCommitResult) => void | Promise<void>;
}

export function ImportWizard({ transport, onImported }: ImportWizardProps): ReactElement {
  const [source, setSource] = useState('');
  const [preview, setPreview] = useState<ImportPreviewResponse | null>(null);
  const [groupColumn, setGroupColumn] = useState('');
  const [groupMode, setGroupMode] = useState<'column' | 'single'>('column');
  const [groupName, setGroupName] = useState('');
  const [idColumn, setIdColumn] = useState('');
  const [measured, setMeasured] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const showError = (cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause));
  const clearPartitions = () => setPreview(current => current ? { ...current, partitions: [] } : current);

  const previewStagedSource = async (staged: StagedFile, displayName: string) => {
    if (staged.parse_state === 'parse_failed') throw new Error(staged.parse_error || 'The uploaded file could not be parsed.');
    setSource(staged.path);
    const next = await transport.imports.preview({ source: staged.path });
    setPreview(next);
    setGroupColumn('');
    setGroupMode('column');
    setGroupName('');
    setIdColumn(next.id_column ?? '');
    setMeasured(next.elemental_columns.filter(column => column !== (next.id_column ?? '')));
    setMessage(`Uploaded ${displayName}. Choose a group column to preview and import.`);
  };
  const upload = async (file?: File) => {
    if (!file) return;
    setBusy(true); setError(''); setMessage(''); setPreview(null);
    try {
      const safeName = file.name.replaceAll(/[^A-Za-z0-9._-]/g, '_') || 'data.csv';
      const path = `sources/${Date.now()}-${safeName}`;
      const staged = await transport.files.upload(path, new Uint8Array(await file.arrayBuffer()));
      await previewStagedSource(staged, file.name);
    } catch (cause) { showError(cause); }
    finally { setBusy(false); }
  };
  const chooseNativeSource = async () => {
    if (!transport.files.pickImportSource) return;
    setBusy(true); setError(''); setMessage('');
    try {
      const staged = await transport.files.pickImportSource();
      if (!staged) return;
      setPreview(null);
      const displayName = staged.path.split(/[\\/]/).at(-1) || staged.path;
      await previewStagedSource(staged, displayName);
    } catch (cause) { showError(cause); }
    finally { setBusy(false); }
  };

  const previewGroups = async (column: string) => {
    setGroupColumn(column); setGroupMode('column'); setMeasured(current => current.filter(item => item !== column)); setError(''); setMessage('');
    if (!column || !source) { clearPartitions(); return; }
    setBusy(true);
    try { setPreview(await transport.imports.preview({ source, group_column: column })); }
    catch (cause) { showError(cause); }
    finally { setBusy(false); }
  };

  const previewSingleGroup = async () => {
    setGroupMode('single'); setError(''); setMessage('');
    if (!source || !groupName.trim()) { setError('Enter a name for this group.'); return; }
    setBusy(true);
    try { setPreview(await transport.imports.preview({ source, group_name: groupName.trim() })); }
    catch (cause) { showError(cause); }
    finally { setBusy(false); }
  };

  const commit = async () => {
    if (!preview || !measured.length || (groupMode === 'column' && !groupColumn) || (groupMode === 'single' && !groupName.trim())) return;
    setBusy(true); setError(''); setMessage('');
    try {
      const result = await transport.imports.commit({ source, group_column: groupMode === 'column' ? groupColumn : '', group_name: groupMode === 'single' ? groupName.trim() : null, visible_id_column: idColumn || null, elemental_columns: measured });
      await onImported?.(result);
      setMessage(`Imported ${result.groups.length} group${result.groups.length === 1 ? '' : 's'} (${result.groups.reduce((sum, item) => sum + item.row_count, 0)} rows).`);
    } catch (cause) { showError(cause); }
    finally { setBusy(false); }
  };

  return <div className="import-wizard">
    {transport.kind === 'tauri' && transport.files.pickImportSource
      ? <button type="button" disabled={busy} onClick={() => void chooseNativeSource()}>Choose a source file</button>
      : <label>Choose a CSV, TSV, or XLSX file <input type="file" accept=".csv,.tsv,.xlsx,text/csv" disabled={busy} onChange={event => void upload(event.currentTarget.files?.[0])} /></label>}
    {source && <p className="import-wizard-source">Source: {source}</p>}
    {preview && <>
      <p>{preview.row_count} rows; {preview.columns.length} columns</p>
      <fieldset><legend>How should rows be grouped?</legend>
        <label><input type="radio" disabled={busy} name="import-group-mode" checked={groupMode === 'column'} onChange={() => { setGroupMode('column'); setGroupColumn(''); clearPartitions(); }} /> Split using a source column</label>
        <label><input type="radio" disabled={busy} name="import-group-mode" checked={groupMode === 'single'} onChange={() => { setGroupMode('single'); clearPartitions(); }} /> Put every row in one named group</label>
      </fieldset>
      {groupMode === 'column' && <label>Group column <select value={groupColumn} onChange={event => void previewGroups(event.target.value)} disabled={busy}>
        <option value="">Choose a column</option>{preview.columns.map(column => <option key={column} value={column}>{column}</option>)}
      </select></label>}
      {groupMode === 'single' && <div className="import-wizard-row">
        <label>Group name <input value={groupName} onChange={event => { setGroupName(event.target.value); clearPartitions(); }} disabled={busy} /></label>
        <button type="button" disabled={busy || !groupName.trim()} onClick={() => void previewSingleGroup()}>Preview single group</button>
      </div>}
      <label>Visible ID column <select value={idColumn} onChange={event => { setIdColumn(event.target.value); setMeasured(current => current.filter(item => item !== event.target.value)); }} disabled={busy}>
        <option value="">Automatic IDs</option>{preview.columns.map(column => <option key={column} value={column}>{column}</option>)}
      </select></label>
      <fieldset className="import-wizard-columns"><legend>Measured numeric columns</legend>
        {preview.columns.filter(column => column !== idColumn && (groupMode === 'single' || column !== groupColumn)).map(column => <label key={column}>
          <input type="checkbox" checked={measured.includes(column)} disabled={busy} onChange={event => setMeasured(current => event.target.checked ? [...current, column] : current.filter(item => item !== column))} /> {column}
        </label>)}
        <p>Unselected columns are stored as descriptive data. Choose at least one measured column.</p>
      </fieldset>
      {((groupMode === 'column' && groupColumn) || (groupMode === 'single' && preview.partitions.length > 0)) && <>
        <h3>Import preview</h3>
        {preview.partitions.length ? <ul>{preview.partitions.map((partition, index) => <li key={`${partition.group_name}-${index}`}>{partition.group_name}: {partition.row_count} rows</li>)}</ul> : <p>No groups found for this column.</p>}
        <button type="button" disabled={busy || !preview.partitions.length || !measured.length} onClick={() => void commit()}>{busy ? 'Working…' : groupMode === 'single' ? 'Import one group' : 'Import groups'}</button>
      </>}
    </>}
    {busy && <p role="status">Working…</p>}{message && <p role="status">{message}</p>}{error && <p role="alert">{error}</p>}
  </div>;
}
