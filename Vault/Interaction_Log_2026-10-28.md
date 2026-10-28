# Interaction Log 2026-10-28

Up: [[Interaction_Log]]

- Data Manager: drag groups onto a folder to copy them there (Shift-drop moves); refused on a folder that already holds them. Tauri window now sets `dragDropEnabled: false` so HTML5 drag and drop works in WebView2. Web checks pass (296 tests); checked in the `?demo` preview; not committed, exe not rebuilt. See [[Drag_Drop_Copy_Groups_2026-10-28]].
- Committed the drag-and-drop copy change and its Vault notes with `commit.sh` (no push); Windows exe not rebuilt yet.
- "Assignment failed: … publish failed: Access is denied" after unchecking a group while an assignment saved: Explore/Ordination/exports/transforms/clustering read group files without the project lock, so a rerun held a file the save had to replace. These reads now take the shared project lock and wait for the save; publish retries briefly on Windows and names the file. Rust checks pass; committed with `commit.sh` (no push), exe not rebuilt. See [[Analysis_Reads_Wait_For_Saves_2026-10-28]].
- Installed the faster group rename build (274b0b9) with `install-windows.sh --launch` once ArchaeoDash was closed; the new exe (34,176,000 bytes) launched, previous build kept as `ArchaeoDash.previous.exe`. See [[Rename_Single_Group_2026-10-27]].
- UMAP dimensions are now selectable: 2 up to `min(analysis columns, 10)` on the Ordination page (with X/Y axis choice) and as the Clustering UMAP source. Cache key keeps old 2-D entries; the disconnected-graph offset grid no longer builds `(m+1)^d` rows. Rust, web and parity checks pass on home2; committed with `commit.sh` (no push), exe not rebuilt. See [[UMAP_Selectable_Dimensions_2026-10-28]].
