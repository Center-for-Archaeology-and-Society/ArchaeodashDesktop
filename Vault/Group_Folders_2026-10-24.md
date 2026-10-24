# Group folders: create, rename, copy/move, one sample per folder (2026-10-24)

Up: [[Index]]

## Request
- Create and rename folders of groups; copy groups in and out (e.g. imported data already has groups and you want a new grouping).
- Copying groups into a folder must fail when it would duplicate row data in that folder.
- Name the first folder after the import's group column instead of "groups".

## Model
- A folder = one grouping of the samples. Group copies keep their samples' hidden analytical UUIDs, so "duplicate row data" means a UUID appearing twice in one folder.
- Analyses use one folder at a time (the "active folder"; radio circle in the Data Manager, "In analyses" badge). Otherwise a sample copied into two folders would be counted twice. The choice is stored per project (`archaeodash.activeFolder:<project>`), pinned as soon as it is known so new folders never take over; exclusions stay per group and are edited only inside the active folder.

## Backend (`crates/application/src/groups.rs`)
- Every row-changing transaction (move/copy units, batch moves, merge, duplicate, copy/move groups) goes through `GroupService::execute`, which refuses when a written folder would hold a sample twice ("folder “X” would hold the same samples twice: N rows of “A” are already in “B”…"). Only rows the transaction writes are checked, so older overlaps between untouched files don't block other work. Descriptive edits and deletes skip the check.
- `list_folders` (folders with group files + empty folders marked by `.archaeodash-folder`), `create_folder`, `rename_folder` (same parent, under the project lock), `delete_folder` (empty only), `copy_groups` (copy or move whole groups into a folder, created when missing; keeps file names, names, identities, lineage; new `TransactionAction::CopyGroups/MoveGroups`). Folder names are checked for Windows/macOS/Linux portability; `sources` and dot-names are reserved.
- `duplicate_group` now defaults to the source's folder (was hard-coded `groups/`); with preserved UUIDs that is refused, so the UI replaced Duplicate with Copy to….
- Tauri: `list_group_folders`, `create_group_folder`, `rename_group_folder`, `delete_group_folder`, `copy_groups`. HTTP: `GET/POST /api/v1/groups/folders`, `/folders/rename`, `/folders/delete`, `POST /api/v1/groups/copy`.

## Frontend
- Data Manager tree: one row per folder (empty ones included), groups under each; click a folder to select it, ▸ to collapse, circle to use it in analyses; groups in other folders show a dashed include box (clicking it switches folders). Toolbar: New folder…, Rename… (selected folder; F2), Copy to…, Move to… (existing folder or a new one), Merge (same folder only), Delete (groups, or an empty selected folder). Folder details panel.
- Analysis pages, Dataset pickers, and precompute see only the active folder's groups.
- Import wizard sends `destination_dir` = the group column made safe (`folderNameForColumn`); single-group imports still go to `groups`. The wizard names the folder before importing.
- Fixed: new groups created from Visualize and automatic assignment were placed in the project root on Windows (paths use backslashes).

## Verification
- Rust: application/storage/api/desktop/contracts/file-store tests incl. `folders_create_rename_copy_move_and_refuse_repeated_samples`; same-folder duplicate and unit copy now refused (tests updated); clippy clean. Windows build compiles the new commands.
- Web: typecheck + 170 tests (folder helpers, tree with folders, folder actions).
- Browser (`?demo`, which now simulates folders): copied two groups into a new folder; tree, badge, and switch rendered. The refusal message itself was not exercised in the browser (pane click targeting was unreliable); it is covered by the Rust test.
- Not checked by hand in the desktop app.

Related: [[UI_Redesign_Exports_KeepAlive_2026-10-23]], [[Interaction_Log_2026-10-24]]
