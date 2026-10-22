# Desktop UI fixes: Explore, Visualize, Ordination, Data Manager (2026-10-22)

Up: [[Index]]

## What changed
- **Crosstabs blank**: the Rust `ExploreCrosstabResponse` flattens the tagged `CrosstabRows` enum (`kind`/`rows` at top level), but the TS contract nested it under `rows`, so neither render branch matched. TS contract now mirrors the wire shape; counts render as a group × value contingency table with totals; the default second field skips all-missing columns.
- **Compositional profile blank**: values (ppm, up to ~10^4) were mapped with `100 - value*10` and clamped to the top edge. Now a Plotly chart (SVG renderer) with one translucent line per sample, colored by a descriptive group field, log concentration axis (legacy `comp.profile` parity).
- **Visualize & Assign**: Dataset is a multiselect dropdown of group *names* (all groups selected by default). Each group file is a group, so color + ellipses key on the file's group name (spread viridis via `groupColors`). "Symbols by" now sets per-point marker symbols with a Symbols legend. The PCA checkbox became an Axes dropdown: Elements / PCA / UMAP / LDA. Labels default to None. Assignment moves selected units per source file.
- **Backend**: `PcaRequest`/`UmapRequest`/`LdaRequest` gained `extra_paths` (rows concatenated in order, first file's revision); `LdaRequest.group_by_file` labels rows by file group name. Duplicate paths are rejected. Transformations apply per file.
- **Ordination PCA**: the stretched unlabeled SVG scatter and cumulative-percent string were replaced with a Plotly scree chart (bars + cumulative line), a variance table (first 10 PCs), and a labelled PC1/PC2 scatter; score table collapsed.
- **Data Manager**: now a file manager in the sidebar (tree of group files, multi-select, Duplicate/Merge/Delete with inline confirmations, details panel, Import… opens the wizard as a modal). `/data` redirects home; the `archaeodash:groups-changed` window event refreshes the tree.

## Verification
- Web: typecheck clean, unit tests pass; Rust: application/contracts/api/desktop tests, fmt and clippy gate clean.
- Browser e2e (`visualize-assignment.mjs`, `import.mjs`) pass against the local API + Vite dev server.
- Note: the repo's sshfs mount is `noexec`; builds/tests run from a mirror at `~/build/archaeodashdesktop`. The in-app test browser lacks WebGL, so Plotly charts use SVG below 3000 points.
