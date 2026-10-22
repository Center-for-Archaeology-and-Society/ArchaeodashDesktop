/**
 * Combine several group files into one plotting dataset for Visualize &
 * Assign. Each project group is its own file, so the file's group name is
 * the row's group (color and ellipse key). Measured columns are the ones
 * every selected file shares (ordination needs a common matrix); descriptive
 * columns are the union, with absent cells left missing.
 */
import type { GroupRowDto, GroupRowsResponse } from '@archaeodash/client';

export interface LoadedGroupFile {
  readonly path: string;
  readonly groupName: string;
  readonly data: GroupRowsResponse;
}

export interface CombinedRows {
  readonly visibleIdColumn: string;
  readonly legacyRowidColumn: string;
  readonly descriptiveColumns: string[];
  readonly elementalColumns: string[];
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
  const rows: GroupRowDto[] = [];
  const groupOf: string[] = [];
  const pathOf: string[] = [];
  for (const f of files) {
    const dIdx = descriptiveColumns.map((c) => f.data.descriptive_columns.indexOf(c));
    const eIdx = elementalColumns.map((c) => f.data.elemental_columns.indexOf(c));
    for (const row of f.data.rows) {
      rows.push({
        ...row,
        descriptive: dIdx.map((i) => (i >= 0 ? (row.descriptive[i] ?? null) : null)),
        elemental: eIdx.map((i) => row.elemental[i] ?? null),
      });
      groupOf.push(f.groupName);
      pathOf.push(f.path);
    }
  }
  return {
    visibleIdColumn: first?.visible_id_column ?? 'ID',
    legacyRowidColumn: first?.legacy_rowid_column ?? 'rowid',
    descriptiveColumns,
    elementalColumns,
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
