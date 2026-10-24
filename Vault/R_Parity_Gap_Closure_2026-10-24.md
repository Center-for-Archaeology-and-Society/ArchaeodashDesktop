# R-app parity gap closure and bug audit (2026-10-24)

Up: [[Index]]

## Request
- Find features of the R Shiny app that the desktop app lacks (excluding what IMPLEMENTATION.md / docs/implementation/13 retire: CDA, subsetData, ternary, test3, runtime package checks, MySQL tables, Shiny workarounds), implement them, then audit for bugs and fix them. Commit often.

## Method
- Four read-only gap reviews (data input/transforms, Explore + Visualize, Ordination/Cluster/Membership/Euclidean/Export, shell/help/preferences) compared each R tab with its new counterpart.
- Six implementation agents worked in parallel on disjoint files (Visualize by the parent; Explore; shell; Ordination; Cluster/Membership/Euclidean; Analysis setup; import/Data Manager), then four audit agents reviewed `43883de..HEAD` by area and fixed confirmed bugs with tests.
- Builds, tests, and commits ran on home2 through `scripts/home2/` ([[Home2_Build_Wrappers_2026-10-23]]). `commit.sh` stages whole files, so a few commits carry other agents' hunks in shared files (contracts, Tauri `lib.rs`, `styles.css`); the message of `0e7d772` understates what it includes.

## Implemented (by area)
- **Visualize**: viridis / Dark 3 palette; data ellipse on at 0.90 with a 0.50–0.99 slider, ported from ggplot2 `stat_ellipse` (MASS `cov.trob` t fit, `sqrt(2·qf(level,2,n-1))`, n ≥ 4); multi-value filter + Clear; symbols default to the group column; label column choice; richer hover; multiplot X/Y pickers building the Y-by-X facet grid, axis ticks, legend, point size 0.1–10, figure height, progressive drawing with Stop/Continue.
- **Explore**: missing-values bar chart by band; histogram with exactly n equal-width bins (ggplot semantics; `hist()` pretty breaks kept for golden 12); table search and sort; crosstab on any column, 2-decimal summaries; "Export with Analysis setup" (legacy "chemical" export).
- **Ordination**: PCA individuals colored by cos²; variables plot colored by contribution; contribution table over the first min(4, k) PCs; LDA vector plot and a one-discriminant view; stale-response guard; visible export errors; PCA scores to Excel.
- **Cluster / Probabilities / Euclidean**: diagnostic max k (default 10); "First n PCs (var%/cum%)" select defaulting to all; group sizes table; eligibility picker with R's all-groups fallback warning; sortable/filterable result grid with column visibility, compact mode, in-table checkboxes; manual "Create a new group…"; Euclidean selection per match row; unique-valued ID columns; R defaults (average linkage, 10 matches, same group excluded).
- **Analysis setup**: ratio builder (cartesian pairs, names, preview, append/only); saved setups (save as with overwrite confirm, load, delete; reserved name refused).
- **Import / Data Manager**: numeric-only measured defaults; real automatic IDs; "keep as descriptive" choice; destination folder with collision prompt; replace and append imports as journaled transactions; Invert and Show filter; rename one group; add descriptive column with overwrite confirm.
- **Shell**: Home/About parity (version, license, issues link, CAS attribution); Help TOC and help window; toasts; per-page error boundaries; command logging (name, elapsed, error) and Show log folder; desktop theme kept local; hosted-only routes hidden on desktop; external links through `tauri-plugin-opener` (https only).
- **Plots**: Size… control (width/height px for Plotly, PNG 96–600 dpi, default 300).
- **Help** rewritten for the current UI with a Methods section.

## Bugs fixed (beyond those found by the gap reviews)
- Transforms: ratios now built from imputed raw values before the transform; unselected ratio sources no longer enter analyses (`5a09473`).
- Membership: `projection_included` matches R; blank group label never eligible (`0e7d772`, `98623fc`).
- Import: replace refused when a group holds rows from other files (was silent row loss); worksheet names containing `#`; append with a different ID column; folder names in another case on Windows/macOS (`941c40e`).
- Visualize: selection pruned on filter change; "New group" no longer merges into an existing file; large arrays no longer spread into `Math.min/max` (also PartitionPlot).
- Plotly text escaping for user-controlled names (`e1623a6`); LDA redraw loop; multiplot save layout.
- Data Manager/setup: padded reserved names, Windows separators after rename/merge/copy, ratio preview race, setup state leaking across project switches, demo rename overwrite.
- Shell: Info menu closing on click; stale project theme; activity spinner during jobs; page crash taking down the app.

## Not done / follow-ups
- TSV export format (CSV and XLSX cover exports; plan 07 lists TSV).
- Regroup a folder by another descriptive column (needs a new transaction).
- Append-twice guard only sees group-level lineage; first imports into an empty folder are not journaled; Windows reserved names (CON, AUX…) fail safely but are not avoided.
- Nothing above was exercised by hand in the desktop app; coverage is unit/render tests plus Rust tests and parity goldens.

## Verification
- `check.sh all` (fmt, Rust tests, clippy `-D warnings`, typecheck, web tests), `check.sh parity`, `run.sh clippy-desktop` all green after the audit fixes.

Related: [[Group_Folders_2026-10-24]], [[Analysis_Setup_Elements_Log_2026-10-23]], [[Interaction_Log_2026-10-24]]
