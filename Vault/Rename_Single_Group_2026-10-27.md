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

Related: [[Group_Folders_2026-10-24]], [[Recent_Projects_And_Group_Inclusion_2026-10-23]], [[Interaction_Log_2026-10-27]]
