/**
 * Pure helpers for the import wizard: which columns it offers and the
 * commit request it sends. Kept free of React so they can be unit tested.
 */

/**
 * Visible-ID column sent for "Automatic IDs": the 1-based source row number
 * every preview frame starts with (the legacy loader's "Use rowid" choice).
 * Sent explicitly so the backend never substitutes the `anid` default.
 */
export const AUTOMATIC_ID_COLUMN = 'rowid';

/** The commit's `visible_id_column` for the wizard's ID choice ('' = automatic). */
export function visibleIdForCommit(idColumn: string): string {
  return idColumn || AUTOMATIC_ID_COLUMN;
}

/** Columns the user can pick as the visible ID (rowid is the automatic choice). */
export function idColumnChoices(columns: readonly string[]): string[] {
  return columns.filter((column) => column !== AUTOMATIC_ID_COLUMN);
}

/** Columns offered as measured values: not rowid, the ID, or the group column. */
export function measuredColumnChoices(columns: readonly string[], idColumn: string, groupColumn: string): string[] {
  return columns.filter((column) => column !== AUTOMATIC_ID_COLUMN && column !== idColumn && column !== groupColumn);
}
