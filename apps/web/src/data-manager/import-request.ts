/**
 * Pure helpers for the import wizard: which columns it offers, what the
 * destination folder already holds, and the commit request it sends. Kept
 * free of React so they can be unit tested.
 */
import type { ExistingImportGroup, ImportCommitRequest, ImportMode, ImportPreviewResponse, ImportRecipeDto } from '@archaeodash/contracts';

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

/**
 * Columns that become descriptive data unless unchecked (the legacy
 * loader's column choice): every column except rowid, the ID, and the
 * measured columns. The group column stays offered: its values are also the
 * group names.
 */
export function descriptiveColumnChoices(columns: readonly string[], idColumn: string, measured: readonly string[]): string[] {
  return columns.filter((column) => column !== AUTOMATIC_ID_COLUMN && column !== idColumn && !measured.includes(column));
}

/** The descriptive columns kept: the offered ones minus the unchecked ones. */
export function keptDescriptive(choices: readonly string[], dropped: readonly string[]): string[] {
  return choices.filter((column) => !dropped.includes(column));
}

/** File name a commit would give a partition (`Baca.parquet`). */
const fileNameOf = (path: string) => path.slice(path.replace(/\\/g, '/').lastIndexOf('/') + 1);

export interface ImportConflicts {
  /** Groups already in the destination folder. */
  readonly inFolder: readonly ExistingImportGroup[];
  /** Planned file names that already exist there. */
  readonly clashes: readonly string[];
  /** Groups in the folder imported from this same file. */
  readonly sameSource: readonly ExistingImportGroup[];
  /** Other folders holding groups imported from this file. */
  readonly sameSourceElsewhere: readonly string[];
  /** Partitions whose group name matches a group in the folder (append targets). */
  readonly matchingNames: readonly string[];
}

/** What the destination folder already holds, from the preview's project listing. */
export function importConflicts(preview: Pick<ImportPreviewResponse, 'partitions' | 'existing_groups'>, folder: string): ImportConflicts {
  const existing = preview.existing_groups ?? [];
  // Folder names match case-insensitively: on Windows and macOS "site" and
  // "Site" are one folder, and the backend writes into the existing one.
  const sameFolder = (other: string) => other.toLowerCase() === folder.toLowerCase();
  const inFolder = existing.filter((group) => sameFolder(group.folder));
  const names = new Set(inFolder.map((group) => fileNameOf(group.path)));
  const groupNames = new Set(inFolder.map((group) => group.group_name));
  return {
    inFolder,
    clashes: preview.partitions.map((p) => fileNameOf(p.suggested_path)).filter((name) => names.has(name)),
    sameSource: inFolder.filter((group) => group.same_source),
    sameSourceElsewhere: [...new Set(existing.filter((g) => g.same_source && !sameFolder(g.folder)).map((g) => g.folder))].sort(),
    matchingNames: preview.partitions.map((p) => p.group_name).filter((name) => groupNames.has(name)),
  };
}

/** How the wizard resolves a folder that already has groups. */
export type DestinationChoice = '' | 'new' | 'replace' | 'append';

/** True when importing as new groups into the folder would be refused. */
export function newGroupsBlocked(conflicts: ImportConflicts): boolean {
  return conflicts.clashes.length > 0 || conflicts.sameSource.length > 0;
}

/**
 * The commit mode for the wizard's choice, or null while the user still has
 * to choose (the folder already has groups) or the choice would be refused.
 */
export function commitMode(conflicts: ImportConflicts, choice: DestinationChoice): ImportMode | null {
  if (!conflicts.inFolder.length) return 'new_groups';
  switch (choice) {
    case 'new': return newGroupsBlocked(conflicts) ? null : 'new_groups';
    case 'replace': return conflicts.sameSource.length ? 'replace_source' : null;
    case 'append': return conflicts.sameSource.length ? null : 'append';
    default: return null;
  }
}

/** `base`, or `base 2`, `base 3`, … — the first name no existing folder uses. */
export function freeFolderName(base: string, taken: readonly string[]): string {
  const used = new Set(taken.map((name) => name.toLowerCase()));
  if (!used.has(base.toLowerCase())) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base} ${n}`;
    if (!used.has(candidate.toLowerCase())) return candidate;
  }
}

export interface CommitChoices {
  readonly source: string;
  readonly sheet: string;
  readonly groupMode: 'column' | 'single';
  readonly groupColumn: string;
  readonly groupName: string;
  readonly idColumn: string;
  readonly measured: readonly string[];
  readonly descriptive: readonly string[];
  readonly recipe: ImportRecipeDto;
  readonly folder: string;
  readonly mode: ImportMode;
}

/** The `imports/commit` request for the wizard's choices. */
export function commitRequestFor(choices: CommitChoices): ImportCommitRequest {
  return {
    source: choices.source,
    group_column: choices.groupMode === 'column' ? choices.groupColumn : '',
    group_name: choices.groupMode === 'single' ? choices.groupName.trim() : null,
    visible_id_column: visibleIdForCommit(choices.idColumn),
    elemental_columns: [...choices.measured],
    descriptive_columns: [...choices.descriptive],
    recipe: choices.recipe,
    destination_dir: choices.folder,
    mode: choices.mode,
    ...(choices.sheet ? { sheet: choices.sheet } : {}),
  };
}
