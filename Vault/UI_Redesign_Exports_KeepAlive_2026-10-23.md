# UI redesign, table/plot exports, ANID labels, keep-alive pages (2026-10-23)

Up: [[Index]]

## Request
- Analysis forms looked unstyled (labels and controls run together). Make the UI look designed and carry the theme through the app.
- Elements are chosen only in the Data Manager's Analysis setup (remove per-page "Input columns").
- Remove the Projects and Account tabs (desktop).
- Export options for every table (UMAP/LDA scores, cluster results, …); save-plot options.
- Cluster tables showed "Analytical unit N" instead of the ANID; check for other places.
- Pages must keep their state when you switch away; open the project folder in the file manager.

## Design system
- `apps/web/src/styles.css` rewritten around semantic tokens per `html[data-theme]` (Simple, Light, Dark): surfaces, borders, text, primary green, soft fills, focus ring, shadows, radii; system UI font (Segoe UI Variable on Windows). Legacy names (`--text`, `--accent`, `--panel`…) are aliases.
- `applyTheme` now only sets `data-theme` (and clears inline variables older builds set); the stylesheet owns colors. Plotly charts read the tokens (`plotTheme()`), have transparent backgrounds, and redraw on theme change.
- Components: page header (`ui/PageHeader.tsx`), cards, labelled form grid (`.form-grid`, `.field`), segmented tabs (`.pills`), table cards with sticky headers, callouts, band badges, styled selects/inputs/buttons (`.btn`, `.btn-primary`, `.btn-ghost`, `.btn-sm`), restyled Data Manager (grouped toolbar), dialogs, start screen, and a new Home page (hero + getting-started steps).

## Behaviour changes
- Cluster / Probabilities / Euclidean: no element picker; columns come from the Analysis setup ("Values": setup, measured values, or a saved definition). Projection groups use the checkbox dropdown.
- ANID everywhere a sample is named: cluster assignments (ANID, Group, Cluster, Silhouette), hclust cut membership and merge steps (samples by ANID), assignment checklist, dendrogram leaves, partition-plot hover, ordination score tables (ANID, Group, scores) and scatter hover. Membership/Euclidean already used the chosen ID column.
- Exports: generic `export_table` (Tauri) / `POST /api/v1/exports/table` (`archaeodash_application::export_table`): CSV (first sheet, formula guard) or XLSX (one worksheet per sheet, autofit). `ui/TableExport.tsx` adds Excel/CSV buttons to: cluster/diagnostics/merge/membership/Euclidean tables, PCA variance + loadings + scores, UMAP and LDA scores, missing values, histogram bins, crosstabs, Visualize selected rows, planned assignments.
- Plot saving (`ui/PlotSave.tsx`): Save PNG / Save SVG on every Plotly chart, the Visualize scatter, cluster plots (all figures in a panel combined), and the multiplot (all panels in one grid image). Images go through the native save dialog (dialog now offers PNG/SVG); the multiplot previously used browser download links, which do nothing in the desktop app.
- Histogram is now a Plotly bar chart.
- Keep-alive: `shell/KeepAliveOutlet.tsx` keeps visited pages mounted (hidden) so results and settings survive navigation; Explore and Ordination sub-tabs use `ui/TabPanels.tsx`. Pages still reset when the project or its data change.
- `reveal_project` command opens the project folder (Explorer / Finder / xdg-open; strips `\\?\` prefixes). Shown as an icon beside the project name and in the project ▾ menu.
- Desktop nav hides Account and Projects (hosted-only); hosted web keeps them.
- `?demo` URL flag (web build only, not in the desktop app): an in-memory synthetic project (`demo/demo-fetch.ts`) for UI review in a plain browser without a backend.

## Verification
- Rust: application tests incl. `table_export_writes_csv_and_xlsx`; contracts/api pass; Windows build compiles the Tauri shell (reveal, export_table, dialog filters).
- Web: typecheck + 164 tests.
- Visual: home2 preview with `?demo` in the in-app browser: Cluster (results with ANID + export), Explore (dataset, histogram), Ordination (PCA scree/variance/scores), Probabilities, Visualize, Home; Simple and Dark themes; results survive leaving and returning to a page.
- Not checked by hand in the desktop app: native save dialogs for PNG/SVG/XLSX, the reveal button.

## Note on editing files after a home2 commit
- `scripts/home2/commit.sh` resets file modes on home2; afterwards the Edit tool's atomic replace can fail on those files over the share (EPERM on rename). Rewriting the file in place (same bytes) once fixes it.

Related: [[Desktop_Concurrency_Export_Loading_Indicators_2026-10-23]], [[Home2_Build_Wrappers_2026-10-23]], [[Interaction_Log_2026-10-23]]
