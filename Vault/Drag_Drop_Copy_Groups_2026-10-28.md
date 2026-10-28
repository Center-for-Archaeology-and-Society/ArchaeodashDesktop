# Data Manager: drag groups onto a folder to copy them (2026-10-28)

Up: [[Index]]

## Request
Allow drag and drop to copy groups into different folders.

## Change (web only; the backend `copy_groups` already existed)
- `DataManager.tsx`: valid group rows (summary, revision, no error, not being renamed) are `draggable` while the panel is idle and folders are supported. Dragging a highlighted group carries every highlighted group (`draggedGroups`); dragging another group highlights and carries only it.
- Folder rows (`<li>`, so their group rows too) are drop targets. A drop copies the groups (`copy_groups`, `remove_sources: false`); holding **Shift** while dropping moves them, as in the system file manager. The drop goes through the same `copyInto` as **Copy to…** / **Move to…** (one change, rescan, the new copies highlighted, the status line says what happened).
- `dropProblem` refuses a folder that already holds any dragged group (a copy would put the same samples in the folder twice and the backend would refuse it; a move would change nothing) and the project root. The cursor shows no-drop there; a refused drop explains why.
- While dragging, the status line says "Drop … on a folder to copy. Hold Shift to move instead." and the target folder is outlined (`.fm-item-folder.is-drop-target` in `file-manager.css`).
- Empty-folder hint and the New folder message now mention dragging; `content/help.md` describes it.
- Desktop: `tauri.conf.json` sets `"dragDropEnabled": false` on the window. Tauri's file-drop handler otherwise takes over drag events in WebView2 on Windows and HTML5 drag and drop inside the page never fires. Nothing in the app used Tauri file-drop events (imports use the native file picker).

## Verification
- Web (home2 `check.sh web`): typecheck and 296 tests pass, including new tests for `draggedGroups`, `dropProblem` (Windows separators, own folder, partial overlap, root) and the tree's `draggable` / `is-drop-target` markup.
- Browser (`?demo` preview): created a folder, dropped two highlighted groups on it (copied, originals kept, copies highlighted), confirmed the drop is refused on their own folder, Shift-dropped an unselected group onto a group row inside the folder (moved), and saw the highlight and hint clear on drag end. Drag events were fired from script, because the pane's simulated mouse drag does not start a native drag.
- Not checked by hand in the Windows desktop app (exe not rebuilt).

Related: [[Group_Folders_2026-10-24]], [[Rename_Single_Group_2026-10-27]], [[Interaction_Log_2026-10-28]]
