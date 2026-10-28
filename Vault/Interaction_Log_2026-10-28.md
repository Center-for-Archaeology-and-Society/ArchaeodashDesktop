# Interaction Log 2026-10-28

Up: [[Interaction_Log]]

- Data Manager: drag groups onto a folder to copy them there (Shift-drop moves); refused on a folder that already holds them. Tauri window now sets `dragDropEnabled: false` so HTML5 drag and drop works in WebView2. Web checks pass (296 tests); checked in the `?demo` preview; not committed, exe not rebuilt. See [[Drag_Drop_Copy_Groups_2026-10-28]].
- Committed the drag-and-drop copy change and its Vault notes with `commit.sh` (no push); Windows exe not rebuilt yet.
