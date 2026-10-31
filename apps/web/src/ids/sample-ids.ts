/**
 * Which sample ID the analysis pages show and export: each group's visible
 * ID column (the ID column chosen at import, e.g. anid) or the import rowid
 * (the row number in the import source, always present).
 *
 * Combined groups from different imports can use different visible ID
 * columns, or have none, so their IDs mix or come out blank. "Automatic"
 * then uses rowid. Rowids restart at 1 in every import, so when any rowid
 * repeats among the combined rows every label becomes `group:rowid`; a label
 * that still repeats (rows of two imports in one group) gets " (2)", " (3)"…
 * The backend applies the same rule (`crates/application/src/sample_ids.rs`)
 * to Membership and Euclidean IDs and to exports.
 */
import type { GroupRowDto, IdColumnChoice } from '@archaeodash/client';

/** The shared setting: automatic, or a fixed choice. */
export type IdPreference = 'auto' | IdColumnChoice;

/** Column name shown for rowid labels. */
export const ROWID_COLUMN = 'rowid';

/** Column name shown when the groups' visible ID columns differ. */
export const SAMPLE_ID_COLUMN = 'Sample ID';

/** What the sample IDs need from each loaded group file. */
export interface IdFile {
  readonly data: {
    readonly visible_id_column: string;
    readonly rows: readonly Pick<GroupRowDto, 'visible_id'>[];
  };
}

export interface IdSummary {
  /** Distinct visible ID column names, in file order. */
  readonly columns: readonly string[];
  /** Rows without a visible ID. */
  readonly missing: number;
  /** Groups none of whose rows has a visible ID. */
  readonly emptyGroups: number;
  /** Rows whose visible ID another row shares. */
  readonly duplicated: number;
}

const blank = (value: string | null | undefined) => value === null || value === undefined || value.trim() === '';

export function summarizeIds(files: readonly IdFile[]): IdSummary {
  const columns: string[] = [];
  let missing = 0;
  let emptyGroups = 0;
  const counts = new Map<string, number>();
  for (const file of files) {
    if (!columns.includes(file.data.visible_id_column)) columns.push(file.data.visible_id_column);
    let present = 0;
    for (const row of file.data.rows) {
      if (blank(row.visible_id)) { missing++; continue; }
      present++;
      counts.set(row.visible_id!, (counts.get(row.visible_id!) ?? 0) + 1);
    }
    if (file.data.rows.length > 0 && present === 0) emptyGroups++;
  }
  let duplicated = 0;
  for (const n of counts.values()) if (n > 1) duplicated += n;
  return { columns, missing, emptyGroups, duplicated };
}

/**
 * The automatic choice: rowid when the groups use different visible ID
 * columns or a whole group has no IDs; otherwise the visible ID (a few
 * missing IDs show their rowid instead).
 */
export function autoIdChoice(summary: IdSummary): IdColumnChoice {
  return summary.columns.length > 1 || summary.emptyGroups > 0 ? 'rowid' : 'visible';
}

export function resolveIdChoice(preference: IdPreference, summary: IdSummary): IdColumnChoice {
  return preference === 'auto' ? autoIdChoice(summary) : preference;
}

/** Name of the visible ID column: the groups' shared name, else "Sample ID". */
export function visibleIdColumnName(summary: IdSummary): string {
  return summary.columns.length === 1 ? summary.columns[0]! : SAMPLE_ID_COLUMN;
}

/** Menu text for a choice: "Sample ID (anid)" or "rowid". */
export function idChoiceLabel(summary: IdSummary, choice: IdColumnChoice): string {
  if (choice === 'rowid') return ROWID_COLUMN;
  const column = visibleIdColumnName(summary);
  return column === SAMPLE_ID_COLUMN ? SAMPLE_ID_COLUMN : `${SAMPLE_ID_COLUMN} (${column})`;
}

/** Each row's rowid; a missing rowid is the 1-based row position. */
function plainRowids(rows: readonly Pick<GroupRowDto, 'legacy_rowid'>[]): string[] {
  return rows.map((row, i) => (blank(row.legacy_rowid) ? String(i + 1) : row.legacy_rowid!));
}

/** Unambiguous rowid label per row (see the module note). */
export function rowidLabels(rows: readonly Pick<GroupRowDto, 'legacy_rowid'>[], groupOf: readonly string[]): string[] {
  const base = plainRowids(rows);
  if (new Set(base).size === base.length) return base;
  const counts = new Map<string, number>();
  return base.map((rowid, i) => {
    const label = `${groupOf[i] ?? ''}:${rowid}`;
    const n = (counts.get(label) ?? 0) + 1;
    counts.set(label, n);
    return n === 1 ? label : `${label} (${n})`;
  });
}

export interface SampleIds {
  readonly choice: IdColumnChoice;
  /** Header of the ID column. */
  readonly column: string;
  /** ID per combined row. */
  readonly ids: readonly string[];
  /** Whether rowids carry their group name (they repeat across groups). */
  readonly qualified: boolean;
}

/**
 * The ID of every combined row. The visible ID falls back to the rowid
 * label for rows without one.
 */
export function sampleIds(
  combined: { readonly rows: readonly Pick<GroupRowDto, 'visible_id' | 'legacy_rowid'>[]; readonly groupOf: readonly string[] },
  summary: IdSummary,
  choice: IdColumnChoice,
): SampleIds {
  const rowids = rowidLabels(combined.rows, combined.groupOf);
  const qualified = new Set(plainRowids(combined.rows)).size !== combined.rows.length;
  if (choice === 'rowid') return { choice, column: ROWID_COLUMN, ids: rowids, qualified };
  return {
    choice,
    column: visibleIdColumnName(summary),
    ids: combined.rows.map((row, i) => (blank(row.visible_id) ? rowids[i]! : row.visible_id!)),
    qualified,
  };
}

/** A short note on why this choice is used or worth changing, or ''. */
export function idNotice(preference: IdPreference, summary: IdSummary, ids: SampleIds): string {
  const differ = summary.columns.length > 1;
  const columns = summary.columns.join(', ');
  if (ids.choice === 'rowid') {
    const why = preference !== 'auto' ? '' : differ ? `The groups use different ID columns (${columns}), so rowid is used. `
      : summary.emptyGroups > 0 ? 'Some groups have no sample IDs, so rowid is used. ' : '';
    return `${why}${ids.qualified ? 'Rowids repeat across these groups, so they show as group:rowid.' : ''}`.trim();
  }
  if (differ) return `The groups use different ID columns (${columns}); rowid may be clearer.`;
  if (summary.missing > 0) return `${summary.missing} ${summary.missing === 1 ? 'sample has' : 'samples have'} no sample ID and ${summary.missing === 1 ? 'shows its' : 'show their'} rowid.`;
  if (summary.duplicated > 0) return `${summary.duplicated} samples share a sample ID; rowid tells them apart.`;
  return '';
}
