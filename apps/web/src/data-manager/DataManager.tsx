import { useCallback, useEffect, useState, type ReactElement } from 'react';
import type { GroupCandidate, GroupSummary, ImportPreviewResponse } from '@archaeodash/contracts';
import type { Transport } from '@archaeodash/client';
import type { StagedFile } from '@archaeodash/contracts';

export type DataManagerTransport = Pick<Transport, 'kind' | 'files' | 'imports' | 'groups'>;

interface CheckedCandidate { candidate: GroupCandidate; validation: GroupSummary | null; error: string }

/** Upload and import project data, then inspect the groups that the project can use. */
export function DataManager({ transport, onChanged }: { transport: DataManagerTransport; onChanged?: () => void }): ReactElement {
  const [source, setSource] = useState('');
  const [preview, setPreview] = useState<ImportPreviewResponse | null>(null);
  const [groupColumn, setGroupColumn] = useState('');
  const [groupMode, setGroupMode] = useState<'column' | 'single'>('column');
  const [groupName, setGroupName] = useState('');
  const [idColumn, setIdColumn] = useState('');
  const [measured, setMeasured] = useState<string[]>([]);
  const [groups, setGroups] = useState<CheckedCandidate[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');

  const refreshGroups = useCallback(async () => {
    const found = await transport.groups.scan();
    const checked = await Promise.all(found.map(async candidate => {
      try { return { candidate, validation: await transport.groups.validate(candidate.path), error: '' }; }
      catch (cause) { return { candidate, validation: null, error: cause instanceof Error ? cause.message : String(cause) }; }
    }));
    setGroups(checked);
  }, [transport]);

  useEffect(() => { void refreshGroups().catch(cause => setError(cause instanceof Error ? cause.message : String(cause))); }, [refreshGroups]);

  const showError = (cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause));
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
    if (!column || !source) { setPreview(current => current ? { ...current, partitions: [] } : current); return; }
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
      await refreshGroups();
      onChanged?.();
      setMessage(`Imported ${result.groups.length} group${result.groups.length === 1 ? '' : 's'} (${result.groups.reduce((sum, item) => sum + item.row_count, 0)} rows).`);
    } catch (cause) { showError(cause); }
    finally { setBusy(false); }
  };

  return <div className="page-content data-manager-page">
    <h1>Data Manager</h1>
    <section aria-labelledby="import-heading">
      <h2 id="import-heading">Import data</h2>
      {transport.kind === 'tauri' && transport.files.pickImportSource
        ? <button type="button" disabled={busy} onClick={() => void chooseNativeSource()}>Choose a source file</button>
        : <label>Choose a CSV, TSV, or XLSX file <input type="file" accept=".csv,.tsv,.xlsx,text/csv" disabled={busy} onChange={event => void upload(event.currentTarget.files?.[0])} /></label>}
      {source && <p>Source: {source}</p>}
      {preview && <>
        <p>{preview.row_count} rows; {preview.columns.length} columns</p>
        <fieldset><legend>How should rows be grouped?</legend>
          <label><input type="radio" disabled={busy} name="import-group-mode" checked={groupMode === 'column'} onChange={() => { setGroupMode('column'); setGroupColumn(''); setPreview(current => current ? { ...current, partitions: [] } : current); }} /> Split using a source column</label>
          <label><input type="radio" disabled={busy} name="import-group-mode" checked={groupMode === 'single'} onChange={() => { setGroupMode('single'); setPreview(current => current ? { ...current, partitions: [] } : current); }} /> Put every row in one named group</label>
        </fieldset>
        {groupMode === 'column' && <>
        <label>Group column <select value={groupColumn} onChange={event => void previewGroups(event.target.value)} disabled={busy}>
          <option value="">Choose a column</option>{preview.columns.map(column => <option key={column} value={column}>{column}</option>)}
        </select></label>
        </>}
        {groupMode === 'single' && <>
          <label>Group name <input value={groupName} onChange={event => { setGroupName(event.target.value); setPreview(current => current ? { ...current, partitions: [] } : current); }} disabled={busy} /></label>
          <button type="button" disabled={busy || !groupName.trim()} onClick={() => void previewSingleGroup()}>Preview single group</button>
        </>}
        <label>Visible ID column <select value={idColumn} onChange={event => { setIdColumn(event.target.value); setMeasured(current => current.filter(item => item !== event.target.value)); }} disabled={busy}>
          <option value="">Automatic IDs</option>{preview.columns.map(column => <option key={column} value={column}>{column}</option>)}
        </select></label>
        <fieldset><legend>Measured numeric columns</legend>
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
    </section>
    <section aria-labelledby="groups-heading">
      <h2 id="groups-heading">Project groups</h2>
      <button type="button" disabled={busy} onClick={() => { setBusy(true); void refreshGroups().catch(showError).finally(() => setBusy(false)); }}>Refresh groups</button>
      {!groups.length ? <p>No group files found.</p> : <ul>{groups.map(({ candidate, validation, error: validationError }) => <li key={candidate.path}>
        <strong>{validation?.group_name ?? candidate.group?.group_name ?? candidate.path.split('/').at(-1)}</strong> — {validation ? `Valid, ${validation.row_count} rows` : `Needs attention: ${validationError || 'validation failed'}`}
      </li>)}</ul>}
    </section>
    {busy && <p role="status">Working…</p>}{message && <p role="status">{message}</p>}{error && <p role="alert">{error}</p>}
  </div>;
}
