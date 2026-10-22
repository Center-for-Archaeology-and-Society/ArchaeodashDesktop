# All groups selected by default in every Dataset picker (2026-10-22)

Up: [[Index]]

## Decision
Every analysis page (Explore, Visualize & Assign, Ordination, Cluster, Probabilities and Distances, Euclidean Distance) uses one shared Dataset picker: a dropdown of group names with checkboxes, **all groups selected by default**. Each project group is its own group file, so selecting several groups means combining several files.

## How it works
- **Frontend**: `apps/web/src/datasets/useGroupFiles.ts` (scan, default-all selection, load + combine rows) and `GroupMultiSelect.tsx`. `visualize/combine.ts` builds the combined table: shared measured columns (intersection), descriptive columns (union) plus a synthetic group column.
- **Synthetic group column**: named `Group`, or `Group (file)` when a real `Group` descriptive column exists (same rule in Rust `synthetic_group_column` and TS `syntheticGroupColumn`). It holds each row's file group name and is read-only in the table.
- **Default grouping** (`defaultGroupColumn`): the synthetic column when several groups are selected; with one group, the first descriptive column (a single-level grouping would make Hotelling/LDA fail).
- **Backend**: every analysis/export request takes `extra_paths`; `crates/application/src/combined.rs` (`CombinedInput`) reads the files into one virtual group file (rows in request order). Ordination transformations now apply to the pooled rows. Single-file exports are byte-identical (no synthetic column); multi-file exports include it and are named `<first>-and-N-more.csv`.
- **Moves from results** (Cluster/Probabilities/Euclidean and Visualize): units can come from several files, so `clustering/multi-source-move.ts` splits a move into one transfer per source file, refreshing revisions between steps and naming a new group only on the step that creates it. Not atomic across files: a mid-way failure reports how many units already moved.
- **Explore edits** are saved per source file.

## Verification
- Web typecheck + 122 unit tests; Rust application/contracts/api/desktop tests, fmt, clippy gate.
- Browser: all pages default to 5/5 groups on the sample project; crosstab by Group, PCA colored by group, Hotelling and k-means over 307 rows; a cross-group move of 257 units into D3a took four per-source steps.
- e2e `import.mjs`, `phase6.mjs`, `visualize-assignment.mjs` pass on a fresh project (they now pick groups via the picker; checkboxes carry `data-path`).

Related: [[Desktop_UI_Fixes_Explore_Visualize_Ordination_2026-10-22]]
