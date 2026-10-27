# Assigning to an existing group keeps its inclusion (2026-10-27)

Up: [[Index]]

## Why
- Moving observations to another group (Visualize & Assign, or the analysis result assignment) re-included the destination group afterwards. That was right for a newly created group but wrong for an existing one: assigning to an unchecked "group 2" checked it.

## What (`apps/web`)
- `clustering/multi-source-move.ts`: `moveAcrossSources` now returns `createdGroups`, the destinations the move created (the steps that named a new group). A new `includedAfterMove(all, included, createdGroups)` keeps the groups that were included and adds only created groups (paths compared with `/` and `\` treated alike).
- `visualize/VisualizePage.tsx`: after a move, keeps the destination only when it is a new group (before, it always kept it).
- `clustering/AnalysisPage.tsx`: used every transaction output path, which includes an existing destination; now uses `outcome.createdGroups`.
- Unchanged: new groups are still included; an existing destination that was checked stays checked; cluster groups from "replace"/"new folder" ([[Cluster_Groups_Replace_Or_New_Folder_2026-10-26]]) still auto-include.

## Verification
- `multi-source-move.test.ts`: new group is reported created and included; existing unchecked destination stays unchecked (checked stays checked); batch move includes only the new destination; separator-insensitive matching.
- `bash scripts/home2/check.sh web`: typecheck ok, 277/277 tests pass. No Rust changes. Committed (no push); exe not rebuilt.

Related: [[Interaction_Log_2026-10-27]]
