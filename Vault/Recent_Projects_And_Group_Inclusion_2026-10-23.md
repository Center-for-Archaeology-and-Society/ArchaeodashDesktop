# Recent projects and group inclusion (2026-10-23)

Up: [[Index]]

## Recent projects (desktop)
- Pattern: VS Code / Office. On launch with no project open, the last project reopens automatically if its folder still exists ("Reopening Site A…"). With no project open, the main panel is a start screen: "Open a folder…" plus the recent list (name, folder path without the `\?\` prefix, "Opened 2 days ago"); moved or deleted folders show "Folder not found" and can be removed. The top-bar project button gains a ▾ menu: recent projects (current one marked), "Browse for a folder…", "Clear recent list"; keyboard: arrows, Escape.
- Storage: `crates/desktop/src/recent.rs` (`RecentProjects`), JSON in the app config directory (`recent_projects.json`), 10 entries, most recent first, de-duplicated, atomic writes; a damaged file reads as empty.
- Security: the Section 11 rule "JavaScript never supplies a filesystem path" still holds — `open_recent_project` only accepts a path already on the list, which only the native folder picker adds to.
- Tauri commands (`apps/desktop/src-tauri/src/projects.rs`): `recent_projects`, `open_recent_project`, `remove_recent_project`, `clear_recent_projects`; every successful open (picker or recent) records the project.

## Group inclusion (Data Manager)
- Analysis: two concepts were overloaded on one checkbox (highlight selection for Duplicate/Merge/Delete vs. what pages analyze), and each page's Dataset picker reset on navigation. Chosen model: the GIS layers panel — the checkbox means "include in analyses", the row highlight means "selected for file actions". This keeps Delete from ever applying to "everything included" and matches what archaeologists know from QGIS/ArcGIS.
- Controls: row checkbox toggles inclusion (Space from the keyboard; `aria-checked` on the tree item); double-click a checkbox = only that group (like double-clicking a Plotly legend entry); bar above the tree: "In analyses: 3 of 5", **All**, **None**, **Only selected** (Ctrl/Shift-click rows, then include just those). Excluded rows are dimmed; files that need attention cannot be included.
- One shared set (`apps/web/src/datasets/GroupInclusion.tsx`, provider in `main.tsx`): every page's Dataset picker reads and edits it, so the choice carries across pages; stored per project in local storage as the *excluded* set, so new imports are included automatically. Background precompute uses the included groups so its requests match the pages'.

## Verification
- Rust: recent store tests (order/dedupe/cap, list gate + missing folders, damaged file); desktop crate clippy; Tauri app clippy for x86_64-pc-windows-msvc (from the home2 mirror).
- Web: 151/151 tests (path/time formatting, start screen, menu, inclusion helpers, tree inclusion rendering).
- Browser (stubbed API): Ctrl-click Baca + Zuni → Only selected → Explore loads only those two; double-click Hooper → page switches to Hooper; page picker All → sidebar 5 of 5; unticking Chaco updates the page and storage.
- Not run in the desktop app yet (commit only, no rebuild).
