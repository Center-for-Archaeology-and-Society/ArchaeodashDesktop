/**
 * Import wizard (choose source file → worksheet (XLSX) → preview → grouping →
 * ID column → measured columns → import). Launched from the Data Manager sidebar inside
 * a modal dialog; the caller refreshes the file tree via `onImported`.
 */
import { useEffect, useRef, useState, type ReactElement } from 'react';
import type { ImportPreviewResponse, StagedFile } from '@archaeodash/contracts';
import { ImportProgress, type ImportStep } from './ImportProgress.tsx';
import type { DataManagerTransport } from './types.ts';

export type ImportCommitResult = Awaited<ReturnType<DataManagerTransport['imports']['commit']>>;

export interface ImportWizardProps {
  readonly transport: DataManagerTransport;
  readonly onImported?: (result: ImportCommitResult) => void | Promise<void>;
  /** Shown as a Close button once an import has finished. */
  readonly onClose?: () => void;
}

export function ImportWizard({ transport, onImported, onClose }: ImportWizardProps): ReactElement {
  const [source, setSource] = useState('');
  const [sheet, setSheet] = useState('');
  const [preview, setPreview] = useState<ImportPreviewResponse | null>(null);
  const [groupColumn, setGroupColumn] = useState('');
  const [groupMode, setGroupMode] = useState<'column' | 'single'>('column');
  const [groupName, setGroupName] = useState('');
  const [idColumn, setIdColumn] = useState('');
  const [measured, setMeasured] = useState<string[]>([]);
  const [step, setStep] = useState<ImportStep | null>(null);
  const [startedAt, setStartedAt] = useState(0);
  const [done, setDone] = useState<ImportCommitResult | null>(null);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const closeRef = useRef<HTMLButtonElement>(null);
  const busy = step !== null;
  // After a finished import the form is read-only; choosing a new file resets it.
  const locked = busy || done !== null;

  // Move focus to Close when the import finishes so keyboard users land on it.
  useEffect(() => { if (done) closeRef.current?.focus(); }, [done]);

  const showError = (cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause));
  /** Runs one backend step with the progress bar showing `next`. */
  const run = async (next: ImportStep, work: () => Promise<void>) => {
    setStep(next); setStartedAt(Date.now()); setError('');
    try { await work(); }
    catch (cause) { showError(cause); }
    finally { setStep(null); }
  };
  const clearPartitions = () => setPreview(current => current ? { ...current, partitions: [] } : current);
  // Only workbook previews name a sheet; CSV/TSV requests leave it out.
  const sheetArg = (name: string) => name ? { sheet: name } : {};

  // Fresh preview of the source (or one worksheet of it) with column defaults reset.
  const loadSource = async (path: string, sheetName: string) => {
    const next = await transport.imports.preview({ source: path, ...sheetArg(sheetName) });
    setPreview(next);
    setSheet(next.sheet ?? '');
    setGroupColumn('');
    setGroupMode('column');
    setGroupName('');
    setIdColumn(next.id_column ?? '');
    setMeasured(next.elemental_columns.filter(column => column !== (next.id_column ?? '')));
  };
  const previewStagedSource = async (staged: StagedFile, displayName: string) => {
    if (staged.parse_state === 'parse_failed') throw new Error(staged.parse_error || 'The uploaded file could not be parsed.');
    setSource(staged.path);
    setStep('reading');
    await loadSource(staged.path, '');
    setMessage(`Uploaded ${displayName}. Choose a group column to preview and import.`);
  };
  const resetForNewSource = () => { setMessage(''); setPreview(null); setSheet(''); setDone(null); };
  const chooseSheet = async (name: string) => {
    if (!source || name === sheet) return;
    setMessage('');
    await run('reading', () => loadSource(source, name));
  };
  const upload = async (file?: File) => {
    if (!file) return;
    resetForNewSource();
    await run('uploading', async () => {
      const safeName = file.name.replaceAll(/[^A-Za-z0-9._-]/g, '_') || 'data.csv';
      const path = `sources/${Date.now()}-${safeName}`;
      const staged = await transport.files.upload(path, new Uint8Array(await file.arrayBuffer()));
      await previewStagedSource(staged, file.name);
    });
  };
  const chooseNativeSource = async () => {
    const pick = transport.files.pickImportSource;
    if (!pick) return;
    setMessage('');
    await run('picking', async () => {
      const staged = await pick();
      if (!staged) return;
      resetForNewSource();
      const displayName = staged.path.split(/[\\/]/).at(-1) || staged.path;
      await previewStagedSource(staged, displayName);
    });
  };

  const previewGroups = async (column: string) => {
    setGroupColumn(column); setGroupMode('column'); setMeasured(current => current.filter(item => item !== column)); setError(''); setMessage('');
    if (!column || !source) { clearPartitions(); return; }
    await run('grouping', async () => { setPreview(await transport.imports.preview({ source, group_column: column, ...sheetArg(sheet) })); });
  };

  const previewSingleGroup = async () => {
    setGroupMode('single'); setError(''); setMessage('');
    if (!source || !groupName.trim()) { setError('Enter a name for this group.'); return; }
    await run('grouping', async () => { setPreview(await transport.imports.preview({ source, group_name: groupName.trim(), ...sheetArg(sheet) })); });
  };

  const commit = async () => {
    if (done || !preview || !measured.length || (groupMode === 'column' && !groupColumn) || (groupMode === 'single' && !groupName.trim())) return;
    setMessage('');
    await run('importing', async () => {
      const result = await transport.imports.commit({ source, group_column: groupMode === 'column' ? groupColumn : '', group_name: groupMode === 'single' ? groupName.trim() : null, visible_id_column: idColumn || null, elemental_columns: measured, ...sheetArg(sheet) });
      setDone(result);
      setMessage(`Imported ${result.groups.length} group${result.groups.length === 1 ? '' : 's'} (${result.groups.reduce((sum, item) => sum + item.row_count, 0)} rows).`);
      await onImported?.(result);
    });
  };

  return <div className="import-wizard">
    {transport.kind === 'tauri' && transport.files.pickImportSource
      ? <button type="button" disabled={busy} onClick={() => void chooseNativeSource()}>Choose a source file</button>
      : <label>Choose a CSV, TSV, or XLSX file <input type="file" accept=".csv,.tsv,.xlsx,text/csv" disabled={busy} onChange={event => void upload(event.currentTarget.files?.[0])} /></label>}
    {source && <p className="import-wizard-source">Source: {source}</p>}
    {preview && <>
      {(preview.sheets?.length ?? 0) > 1 && <label>Worksheet <select value={sheet} onChange={event => void chooseSheet(event.target.value)} disabled={locked}>
        {(preview.sheets ?? []).map(name => <option key={name} value={name}>{name}</option>)}
      </select></label>}
      <p>{preview.row_count} rows; {preview.columns.length} columns{sheet ? ` (worksheet ${sheet})` : ''}</p>
      <fieldset><legend>How should rows be grouped?</legend>
        <label><input type="radio" disabled={locked} name="import-group-mode" checked={groupMode === 'column'} onChange={() => { setGroupMode('column'); setGroupColumn(''); clearPartitions(); }} /> Split using a source column</label>
        <label><input type="radio" disabled={locked} name="import-group-mode" checked={groupMode === 'single'} onChange={() => { setGroupMode('single'); clearPartitions(); }} /> Put every row in one named group</label>
      </fieldset>
      {groupMode === 'column' && <label>Group column <select value={groupColumn} onChange={event => void previewGroups(event.target.value)} disabled={locked}>
        <option value="">Choose a column</option>{preview.columns.map(column => <option key={column} value={column}>{column}</option>)}
      </select></label>}
      {groupMode === 'single' && <div className="import-wizard-row">
        <label>Group name <input value={groupName} onChange={event => { setGroupName(event.target.value); clearPartitions(); }} disabled={locked} /></label>
        <button type="button" disabled={locked || !groupName.trim()} onClick={() => void previewSingleGroup()}>Preview single group</button>
      </div>}
      <label>Visible ID column <select value={idColumn} onChange={event => { setIdColumn(event.target.value); setMeasured(current => current.filter(item => item !== event.target.value)); }} disabled={locked}>
        <option value="">Automatic IDs</option>{preview.columns.map(column => <option key={column} value={column}>{column}</option>)}
      </select></label>
      <fieldset className="import-wizard-columns"><legend>Measured numeric columns</legend>
        {preview.columns.filter(column => column !== idColumn && (groupMode === 'single' || column !== groupColumn)).map(column => <label key={column}>
          <input type="checkbox" checked={measured.includes(column)} disabled={locked} onChange={event => setMeasured(current => event.target.checked ? [...current, column] : current.filter(item => item !== column))} /> {column}
        </label>)}
        <p>Unselected columns are stored as descriptive data. Choose at least one measured column.</p>
      </fieldset>
      {((groupMode === 'column' && groupColumn) || (groupMode === 'single' && preview.partitions.length > 0)) && <>
        <h3>Import preview</h3>
        {preview.partitions.length ? <ul>{preview.partitions.map((partition, index) => <li key={`${partition.group_name}-${index}`}>{partition.group_name}: {partition.row_count} rows</li>)}</ul> : <p>No groups found for this column.</p>}
        {!done && <button type="button" disabled={locked || !preview.partitions.length || !measured.length} onClick={() => void commit()}>{groupMode === 'single' ? 'Import one group' : 'Import groups'}</button>}
      </>}
    </>}
    {step && <ImportProgress step={step} startedAt={startedAt} groups={preview?.partitions.length ?? 0} />}
    {!step && message && <p role="status" className={done ? 'import-wizard-done' : undefined}>{message}</p>}
    {error && <p role="alert">{error}</p>}
    {done && onClose && <div className="import-wizard-footer">
      <button ref={closeRef} type="button" className="fm-tool fm-tool-primary" onClick={onClose}>Close</button>
    </div>}
  </div>;
}
