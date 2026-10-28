/**
 * Export buttons for any table the app shows: Excel (one worksheet per sheet)
 * or CSV (first sheet). The backend formats the file (`exports.table`); the
 * client saves it through the native save dialog (desktop) or a download.
 */
import { createContext, useContext, useId, useState, type ReactElement, type ReactNode } from 'react';
import type { ExportFormat, ExportsService, TableCell } from '@archaeodash/client';
import { downloadExportResult } from '../exports.ts';
import { BusyLabel } from './Busy.tsx';

export interface TableSheet {
  readonly name: string;
  readonly columns: readonly string[];
  readonly rows: readonly (readonly TableCell[])[];
}

const ExportsContext = createContext<Pick<ExportsService, 'table'> | null>(null);

export function ExportsProvider({ service, children }: { service: Pick<ExportsService, 'table'>; children: ReactNode }): ReactElement {
  return <ExportsContext.Provider value={service}>{children}</ExportsContext.Provider>;
}

export function useTableExporter(): ExportsService['table'] {
  return useContext(ExportsContext)?.table;
}

/** A number cell, or null when it is missing or not finite. */
export function num(value: number | null | undefined): TableCell {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

export function TableExport({
  fileName,
  sheets,
  className,
}: {
  /** Suggested file name without extension, e.g. "UMAP scores". */
  fileName: string;
  /** Built on click, so large tables cost nothing until exported. */
  sheets: () => readonly TableSheet[];
  className?: string;
}): ReactElement | null {
  const exportTable = useTableExporter();
  const [busy, setBusy] = useState<ExportFormat | null>(null);
  const [error, setError] = useState('');
  if (!exportTable) return null;
  const run = (format: ExportFormat) => {
    setBusy(format);
    setError('');
    void Promise.resolve()
      .then(() => exportTable({
        file_name: fileName,
        format,
        sheets: sheets().map((s) => ({ name: s.name, columns: [...s.columns], rows: s.rows.map((r) => [...r]) })),
      }))
      .then(downloadExportResult)
      .catch((cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause)))
      .finally(() => setBusy(null));
  };
  return (
    <div className={className ? `export-actions ${className}` : 'export-actions'} role="group" aria-label={`Export ${fileName}`}>
      <span className="export-label">Export</span>
      <button type="button" className="btn btn-sm" disabled={busy !== null} onClick={() => run('xlsx')}>
        <BusyLabel busy={busy === 'xlsx'}>Excel</BusyLabel>
      </button>
      <button type="button" className="btn btn-sm" disabled={busy !== null} onClick={() => run('csv')}>
        <BusyLabel busy={busy === 'csv'}>CSV</BusyLabel>
      </button>
      {error && <span role="alert" className="export-error">Export failed: {error}</span>}
    </div>
  );
}

const COLLAPSED_KEY = 'archaeodash.tableCollapsed';

/** Collapsed table cards, by export file name (per viewer, best effort). */
function readCollapsed(): Record<string, boolean> {
  try {
    const raw = typeof localStorage === 'undefined' ? null : localStorage.getItem(COLLAPSED_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, boolean>) : {};
  } catch {
    return {};
  }
}

function writeCollapsed(name: string, collapsed: boolean): void {
  try {
    if (typeof localStorage === 'undefined') return;
    const all = readCollapsed();
    if (collapsed) all[name] = true;
    else delete all[name];
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(all));
  } catch {
    // Remembering is a convenience; the card still toggles.
  }
}

/**
 * A titled table card: heading, export buttons, and the scrolling table.
 * The heading collapses and expands the table; the choice is remembered per
 * table (by its export name). A collapsed table stays mounted, so its
 * filters, sort and checked rows survive.
 */
export function TableCard({
  title,
  fileName,
  sheets,
  children,
  note,
  bare = false,
  defaultCollapsed = false,
}: {
  title: ReactNode;
  fileName: string;
  sheets: () => readonly TableSheet[];
  children: ReactNode;
  note?: ReactNode;
  /** Children are not a table (e.g. a chart): no scrolling table frame. */
  bare?: boolean;
  /** Start collapsed unless the viewer chose otherwise. */
  defaultCollapsed?: boolean;
}): ReactElement {
  const bodyId = useId();
  const [collapsed, setCollapsed] = useState(() => readCollapsed()[fileName] ?? defaultCollapsed);
  const toggle = () => {
    setCollapsed((current) => {
      writeCollapsed(fileName, !current);
      return !current;
    });
  };
  return (
    <section className={collapsed ? 'card table-card is-collapsed' : 'card table-card'}>
      <header className="card-header">
        <h3 className="card-title">
          <button type="button" className="card-collapse" aria-expanded={!collapsed} aria-controls={bodyId} onClick={toggle}>
            <span aria-hidden="true" className="card-collapse-icon">{collapsed ? '▸' : '▾'}</span>
            {title}
          </button>
        </h3>
        <TableExport fileName={fileName} sheets={sheets} />
      </header>
      <div id={bodyId} hidden={collapsed}>
        {note && <p className="card-note">{note}</p>}
        {bare ? children : <div className="table-scroll">{children}</div>}
      </div>
    </section>
  );
}
