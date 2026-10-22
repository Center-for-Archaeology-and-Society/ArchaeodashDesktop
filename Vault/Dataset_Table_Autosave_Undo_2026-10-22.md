# Dataset table autosave with undo (2026-10-22)

Up: [[Index]]

## Behaviour
- Explore → Dataset: descriptive cells save when the cell loses focus or on Enter; Escape cancels the edit in progress. Unchanged values are not saved; empty text saves as missing (null).
- Undo / Redo buttons and Ctrl+Z / Ctrl+Shift+Z (Ctrl+Y) reverse saved edits. While a cell has unsaved typing, Ctrl+Z is left to the text field. Undo history lasts while Explore stays open on the same loaded groups (it survives switching Explore tabs; Reload or changing the Dataset selection starts fresh).
- Status line: "Saving…" / "All changes saved" / "N edits not saved" with Retry; cells that failed are red. A revision conflict (file changed elsewhere) blocks that file and offers "Reload changed groups".
- Pending typing is committed when the table unmounts (tab/page switch) and on `pagehide`. A desktop window close may still drop an edit that is mid-save.

## Design
- `apps/web/src/explore/autosave.ts` (`CellAutosave`): per-file queue, one save in flight per file; edits made meanwhile are bundled into the next patch at the revision the previous save returned. No reload after saves, so focus/scroll are kept. Other Explore views wait for pending saves.
- Backend history policy (`crates/file-store-fs`): descriptive-edit transactions (`PatchDescriptive`) share one prior-revision snapshot per edit session (10 minutes, `EDIT_SNAPSHOT_INTERVAL`), tracked by a `<key>.edit-session` marker next to the group's history dir. Any other transaction on the group snapshots as before, and the next edit starts a new session. Crash-safety backups are unchanged. This keeps the 3-revision history (`MAX_HISTORY_PER_GROUP`) meaningful instead of being flooded by single-cell revisions.

## Verification
- Unit tests: `explore/autosave.test.ts` (no-op edits, bundling + revisions, undo/redo, per-file independence, conflict, retry); file store `edit_session_shares_one_history_snapshot`.
- Browser: edit + Tab saved (focus moved on), Enter saved, Ctrl+Z reverted on disk; three edits → revision rev-4 with a single history snapshot.

Related: [[All_Groups_Default_Dataset_2026-10-22]]
