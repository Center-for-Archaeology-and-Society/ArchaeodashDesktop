# Membership sorting/number filters, portable group names, assign hold, collapsible tables (2026-10-28)

Up: [[Index]]

## Requests
1. Membership probabilities need sorting options and number filtering.
2. Group names must not contain characters invalid in file names.
3. UMAP and plot updates should wait until samples are reassigned and the Data Manager has refreshed.
4. Tables should be collapsible.

## Changes
**Result grid** (`clustering/ResultGrid.tsx`, `grid-model.ts`; used by membership, Euclidean and cluster tables):
- Toolbar "Sort [column] [direction] then [column] [direction]"; header clicks still sort by one column. `sortRows` takes several keys (missing values last).
- Number columns (fixed decimals, or only numbers present) get an operator select (Any, ≥, ≤, >, <, =, Between) and value box(es) instead of a text box. Stored as filter text (`>= 50`, `10..50`); typed ranges `a..b` and `≥`/`≤` also parse.
- "Show N rows" select (100/250/500/1000/All) beside "Show 100 more rows".

**Group names** (`datasets/groupNames.ts` `portableGroupNameProblem`; backend `validate_group_name`):
- Same rule everywhere a name is typed: not empty, ≤ 100 chars, no leading dot, none of `< > : " / \ | ? *` or control characters, no Windows-reserved file. Used by Data Manager rename and merge, Visualize new group, Clustering/auto-assign new group (`newGroupProblem`). The message names the offending characters.
- Backend now validates new names in `transfer_units`, `batch_transfer_units`, `merge_groups` and `duplicate_group` (rename already did). Previously these silently sanitized only the file name and stored the raw name. Import still sanitizes group names that come from data values.

**Assign hold** (`data-manager/events.ts`, `datasets/groupUpdates.ts`):
- Log showed ~8 parallel `scan_group_candidates` (3–5 s each on the share) and a UMAP run after each `transfer_units`.
- `holdGroupUpdates()` is taken by Visualize assign and Clustering assign/apply-clusters before the move and released after their own rescan and `await notifyGroupsChanged()`. `notifyGroupsChanged` now resolves when every listener's refresh (pages, Data Manager tree) has settled.
- While held: the Visualize scatter and multiplot keep their last picture (`useHeldValue`), Visualize scores and Ordination page runs don't start, and precompute waits. After release they run once, and only if their inputs changed (unchanged rows keep the result, so a move does not rerun PCA/UMAP).
- `sharedScan`: refreshes answering one change notice share one scan (per change epoch). Other scans stay private, so an unannounced write can't be hidden by a reused scan.

**Collapsible tables** (`ui/TableExport.tsx` `TableCard`): the title is a toggle (▾/▸, `aria-expanded`). The state is remembered per table in localStorage (best effort). Collapsed bodies stay mounted (`hidden`), so filters, sort and checked rows survive.

## Verification
- Tests: `grid-number-filters.test.ts`, `groupUpdates.test.ts`, name cases in `new-group.test.ts` / `VisualizePage.test.tsx`, backend `new_group_names_must_be_portable_file_names`; existing grid tests updated for the number-filter labels.
- home2: web typecheck + 303 tests pass; Rust application/api/desktop fmt, tests, clippy pass.
- `?demo` preview: membership table sorted by Group with Baca ≥ 50 (107 of 240 rows); group sizes card collapses.
- Not checked by hand in the desktop app: the timing of the assign hold against a real project.

Related: [[UMAP_Selectable_Dimensions_2026-10-28]], [[Analysis_Reads_Wait_For_Saves_2026-10-28]], [[No_Recompute_On_Group_Moves_2026-10-26]], [[Rename_Single_Group_2026-10-27]]
