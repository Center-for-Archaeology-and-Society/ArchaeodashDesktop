# Data Manager: rename a single group (2026-10-27)

Up: [[Index]]

## Request
Offer a rename for one group (a group file) in the Data Manager, separate from renaming a group folder.

## What already existed
- Backend `GroupService::rename_group` (`crates/application/src/groups.rs`): one journaled `RenameGroup` transaction that sets the profile name and id and moves the file within its folder (`sanitize_group_name`, so `Baca Pueblo` → `Baca_Pueblo.parquet`); rows, UUIDs and lineage are unchanged. It refuses a stale revision, a name used by another group in the folder (case-insensitive), and a file name that is already taken.
- Desktop crate `rename_group`, the Tauri command `rename_group`, `POST /api/v1/groups/rename`, and `renameGroup` in the Tauri and HTTP client transports.
- In the web UI, a single "Rename…" button that renamed either the selected folder or the one highlighted group.

## Change
- Toolbar: separate **Rename folder…** (selected folder) and **Rename group…** (one highlighted, valid group) buttons. Rename group… shows whenever the transport has `renameGroup`, folders or not. F2 still renames whatever row is focused.
- Validation (`groupNameProblem` in `DataManager.tsx`, same rules as the backend): not empty, at most 100 characters, no leading dot, no control characters or `< > : " / \ | ? *`, not a Windows-reserved file, and no other group in the same folder with that name or that group file. The `RenameGroupForm` shows the reason (`role="alert"`) and disables Rename; backend errors (for example a clash made in another window) appear in the panel's error line.
- Backend: `rename_group` now calls the new `validate_group_name`, so it refuses slashes and other invalid file-name characters in the name instead of turning them into `_`. Folder names, cluster prefixes and group names share `portable_name_problem`.
- After a rename, `renameGroupFile` moves the group's exclusion to its new path, so it stays checked or unchecked for analyses. The group list is refreshed, and the renamed group stays selected and focused.
- Help (`content/help.md`) lists both buttons.

## Verification
- Rust (home2): `rename_group_refuses_clashes_blank_names_and_stale_revisions` now also checks `/`, `\`, `:`, `?`, a leading dot and `CON`. fmt, 119 application tests and clippy pass.
- Web: new `DataManager.test.tsx` tests cover a successful rename (request, Windows separators, enabled form), invalid names, name and file clashes (form and backend rejection), and inclusion kept for checked and unchecked groups. Typecheck and 292 web tests pass.

## Follow-up: renaming felt slow
The user found renaming slow and asked that nothing rerun until a name is applied, and that they can keep renaming without interruption.

### What was slow
The desktop log (`%LOCALAPPDATA%\org.archaeodash.desktop\logs`) showed:
- `rename_group` took 2.7–7.7 s. Part of that was its clash check scanning every group file in the project. The rest was waiting for the exclusive project lock behind scans that the previous rename had set off.
- After each rename the panel stayed locked through `runChange`: a full rescan, `list_group_folders`, and a validation of every group (1–3 s each).
- `onDataChanged` then bumped `AppShell`'s `dataVersion`, which remounted every open analysis page. Each page rescanned, reloaded its rows and reran its analyses (a UMAP run started and was cancelled).

### Change
- Backend: the clash check reads only the renamed group's folder. New `scan_folder` in data-io (non-recursive) and `FsGroupFileStore::scan_folder_snapshot`; `GroupService::folder_candidates`.
- Web (`DataManager.tsx`):
  - Submitting closes the form, and the tree shows the new name at once with "Renaming…".
  - Renames go to the backend one at a time through a promise queue, so you can rename the next group straight away. Other file actions (Import, Copy, Merge, Delete, Refresh, …) wait while renames are being saved.
  - The tree entry, selection and focus are updated from the backend's answer (`renamedEntry`); there is no rescan.
  - A failed rename restores the old name and shows the error.
  - Inclusion: `exclusionsAfterRename` keeps a left-out group left out under its new path. Its old path stays excluded too until renaming settles, because the pages still list it; `withoutStaleExclusions` then drops it.
  - Settling: with no rename being saved and no rename form open for `RENAME_SETTLE_MS` (1.5 s), the panel announces the change once with `notifyGroupsChanged`. Pages rescan in place, and the panel ignores its own announcement. It no longer calls `onDataChanged`, so pages are not remounted.
  - The precompute (`AnalysisSetupPanel` `hold`) and the row preload wait until then.
- Reruns: PCA/UMAP views and their backend cache are keyed on row content (UUIDs and values), so a rename reuses them. LDA (labelled by group, and cached by file) recomputes, which is correct for new labels.

### Verification
- Rust: `rename_group_only_checks_its_own_folder` and `scan_folder_skips_subfolders_that_scan_project_reads`; fmt, tests and clippy pass for data-io, file-store-fs and application.
- Web: tests for the saving state in the tree and clash checks against names still being saved, the entry updated from the answer, inclusion kept (left out, included, renamed twice, a path used again, case-only), and the settle delay. Typecheck and 294 web tests pass.

Related: [[Ordination_Cancel_Superseded_Runs_2026-10-27]], [[No_Recompute_On_Group_Moves_2026-10-26]], [[Group_Folders_2026-10-24]], [[Recent_Projects_And_Group_Inclusion_2026-10-23]], [[Interaction_Log_2026-10-27]]
