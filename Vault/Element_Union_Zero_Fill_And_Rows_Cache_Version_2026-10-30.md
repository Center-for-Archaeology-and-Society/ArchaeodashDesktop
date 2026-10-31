Up: [[Index]]

# Element union with zero fill; rows cache version (2026-10-30)

## Combining groups with different measured columns
- Combined analysis input (`apps/web/src/visualize/combine.ts`, `crates/application/src/combined.rs`, kept identical): elemental columns are the union in first-seen order. A row whose group file lacks a column gets 0; missing values in a column the file has stay missing.
- `CombinedInput::require_columns` no longer refuses elemental columns missing from some files; descriptive columns still must exist in every file.
- The all-groups imputation pool (`prepared.rs`) now takes every group file of the folder (zero-filled), so a zero-filled row is always in the pool.
- Analysis precompute (`analysis-setup/precompute.ts`) uses the same union.
- Notice: `zeroFillNotice` names groups and the columns set to 0; shown in the Data Manager folder details and above the Explore dataset table.

## Group row operations
- `assert_schema_compatible` replaced by `merged_roles` + `align_row` (`crates/storage`): the destination keeps its columns and ID column and gains the columns incoming rows add. Existing rows get 0 for new measured columns and blank new descriptive cells; incoming rows are aligned by name (0 for measured columns they lack). A descriptive column named like the incoming row's old ID column gets that ID when the cell is absent.
- Refused (`SchemaMismatch`): a name measured in one group and descriptive or an ID column in the other.
- `assert_measured_values_preserved_in` checks by column name (original values bit-identical, added columns 0). The old positional entry point remains for same-column successors.
- Used by move, copy, batch transfer, merge, and apply clusters.

## Rows shown from an earlier file (Gwynn Canyon report)
- The backend returns every row of a copied group. Pages read rows through the session `DataCache`, keyed by path and revision. Every new group file starts at `rev-1`, so a file deleted or moved away and another created under the same name (copy, new group from an assignment) shares path and revision, and pages got the old file's rows while the Data Manager showed the new row count from the scan.
- Fix: `rowsVersion(summary)` (revision, group id, row count, columns) is the cache version, and `observeScan` drops cached rows of files no longer listed and clears computed results when any file went away.
- Not changed: revision ids themselves still restart at `rev-1` for a new file under a reused path (backend follow-up if needed).
