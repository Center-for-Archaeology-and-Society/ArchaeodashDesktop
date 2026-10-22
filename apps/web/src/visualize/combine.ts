/**
 * Combine several group files into one dataset. Each project group is its
 * own file, so the file's group name is the row's group. Measured columns are
 * the ones every selected file shares (analyses need a common matrix);
 * descriptive columns are the union, with absent cells left missing, plus a
 * synthetic group column holding each row's group name. The backend builds
 * the identical combined table for `extra_paths` requests, so column names
 * and row order line up with analysis results.
 */
import type { GroupRowDto, GroupRowsResponse } from '@archaeodash/client';

export interface LoadedGroupFile {
  readonly path: string;
  readonly groupName: string;
  readonly data: GroupRowsResponse;
}

/**
 * Synthetic group column name, matching the backend rule: "Group", unless a
 * real descriptive column is already called "Group".
 */
export function syntheticGroupColumn(descriptiveNames: Iterable<string>): string {
  for (const name of descriptiveNames) if (name === 'Group') return 'Group (file)';
  return 'Group';
}

export interface CombinedRows {
  readonly visibleIdColumn: string;
  readonly legacyRowidColumn: string;
  /** Union of descriptive columns, ending with the synthetic group column. */
  readonly descriptiveColumns: string[];
  readonly elementalColumns: string[];
  /** Name of the synthetic group column (last descriptive column). */
  readonly groupColumn: string;
  /** Rows re-aligned to the combined column lists, in file order. */
  readonly rows: GroupRowDto[];
  /** Group name per combined row. */
  readonly groupOf: string[];
  /** Source file path per combined row. */
  readonly pathOf: string[];
  /** Group names in file order. */
  readonly groupNames: string[];
}

export function combineGroupFiles(files: readonly LoadedGroupFile[]): CombinedRows {
  const first = files[0]?.data;
  const elementalColumns = first
    ? first.elemental_columns.filter((c) => files.every((f) => f.data.elemental_columns.includes(c)))
    : [];
  const descriptiveColumns: string[] = [];
  for (const f of files) {
    for (const c of f.data.descriptive_columns) {
      if (!descriptiveColumns.includes(c)) descriptiveColumns.push(c);
    }
  }
  const groupColumn = syntheticGroupColumn(descriptiveColumns);
  const rows: GroupRowDto[] = [];
  const groupOf: string[] = [];
  const pathOf: string[] = [];
  for (const f of files) {
    const dIdx = descriptiveColumns.map((c) => f.data.descriptive_columns.indexOf(c));
    const eIdx = elementalColumns.map((c) => f.data.elemental_columns.indexOf(c));
    for (const row of f.data.rows) {
      rows.push({
        ...row,
        descriptive: [
          ...dIdx.map((i) => (i >= 0 ? (row.descriptive[i] ?? null) : null)),
          f.groupName,
        ],
        elemental: eIdx.map((i) => row.elemental[i] ?? null),
      });
      groupOf.push(f.groupName);
      pathOf.push(f.path);
    }
  }
  return {
    visibleIdColumn: first?.visible_id_column ?? 'ID',
    legacyRowidColumn: first?.legacy_rowid_column ?? 'rowid',
    descriptiveColumns: [...descriptiveColumns, groupColumn],
    elementalColumns,
    groupColumn,
    rows,
    groupOf,
    pathOf,
    groupNames: files.map((f) => f.groupName),
  };
}

/** Group selected uuids by their source file, preserving file order. */
export function selectionBySource(
  combined: CombinedRows,
  uuids: ReadonlySet<string>,
): Map<string, string[]> {
  const out = new Map<string, string[]>();
  combined.rows.forEach((row, i) => {
    if (!uuids.has(row.analytical_uuid)) return;
    const path = combined.pathOf[i]!;
    const list = out.get(path);
    if (list) list.push(row.analytical_uuid);
    else out.set(path, [row.analytical_uuid]);
  });
  return out;
}

/**
 * The combined rows in the single-file `GroupRowsResponse` shape, for views
 * written against one file. `path`/`revision_id` are the first file's.
 */
export function asRowsResponse(files: readonly LoadedGroupFile[], combined: CombinedRows): GroupRowsResponse {
  return {
    path: files[0]?.path ?? '',
    revision_id: files[0]?.data.revision_id ?? '',
    visible_id_column: combined.visibleIdColumn,
    legacy_rowid_column: combined.legacyRowidColumn,
    descriptive_columns: combined.descriptiveColumns,
    elemental_columns: combined.elementalColumns,
    rows: combined.rows,
  };
}

/** `{ path, extra_paths }` for a request over the loaded files. */
export function inputPaths(files: readonly LoadedGroupFile[]): { path: string; extra_paths: string[] } {
  const [first, ...rest] = files.map((f) => f.path);
  return { path: first ?? '', extra_paths: rest };
}

/**
 * Default grouping column: the synthetic group column when several groups
 * are combined (each file is a group); with a single file it has only one
 * level, so fall back to the file's first descriptive column.
 */
export function defaultGroupColumn(combined: CombinedRows): string {
  if (combined.groupNames.length > 1) return combined.groupColumn;
  return combined.descriptiveColumns.find((c) => c !== combined.groupColumn) ?? combined.groupColumn;
}
