# Phase 4 Client Ordination, Preferences & Exports — 2026-09-23

Phase 4 client surface (IMPLEMENTATION.md Phase 4 line: "PCA, UMAP, LDA,
descriptive-edit table with elemental columns locked, crosstabs,
missing/histogram/profile views, preferences, and exports") completed in
apps/web. Commits `739d268` (Slice D) and `4f01162` (Slice E).

## Slice D — Ordination route (`739d268`)

- `apps/web/src/ordination/OrdinationPage.tsx`: legacy pill tabs (PCA / UMAP /
  LDA), dataset select reusing `GroupsService.scan/rows`.
- PCA: explained-variance bars + scatter (`ScatterPlot`, shared) + score
  table; **Export PCA scores (CSV)** button → `ExportsService.pcaScores` →
  `downloadExportResult`.
- UMAP: fixed reproducible seed `20260914` (Section 8.7 / golden 07), seed +
  n_neighbors + n_epochs + warnings echoed in the UI; pure `UmapResult`
  component exported for SSR tests.
- LDA: gated on first descriptive column (Section 9.4 "availability status"),
  levels/priors/scores shown.
- Fetch wrappers use a shared `useOrdination<TResp>` hook; pure result
  components (`PcaResult`, `UmapResult`, `LdaView` renders) are testable with
  `renderToString` (effects don't run under node:test).
- Theme changes persist via `transport.preferences.put('theme', next)` after
  the local paint (Section 10.1).

## Slice E — preferences + exports (`4f01162`)

- `apps/web/src/exports.ts`: `downloadExportResult(ExportResult)` — Blob +
  object-URL anchor download of `file_name`/`content` (CSV string, never
  contains analytical_uuid).
- `lastOpenedDataset` preference (legacy `DataLoader.R` selector-default
  semantics, `PreferenceKey::LastOpenedDataset`, string ≤ max len):
  - `main.tsx` hydrates on mount from `preferences.get()` after theme
    hydration; offline falls back to first ready candidate.
  - `ExplorePage` takes `initialDataset` (preferred path among ready
    candidates) and `onDatasetOpened` (persist on selection via
    `preferences.put('lastOpenedDataset', path)`).
- Explore table actions gain **Export measured data (CSV)** →
  `ExportsService.measuredData({ path, raw_text: false })`.
- `ExploreDeps` / `OrdinationDeps` now carry `exports: ExportsService`;
  tests stub it (throws "unused").

## Testing notes (hard-won)

- `renderToString` does not run effects — keep result rendering in pure
  components and test those directly; test the page for static chrome only.
- The `fe` line-edit tool corrupted JSX repeatedly (off-by-one line ranges,
  dropped `.then((list) => {` openers, duplicated fragments/declarations).
  Full-file `write` or `edit` with unique anchors is safer for TSX; always
  `tsc --noEmit` after edit batches and watch for duplicate `const` lines
  (esbuild catches what tsc's parse errors mask).
- Web test count: 11 (explore 7, ordination, shell, theme, design-system
  remain). Rust unchanged: 151.

## Status

- Phase 4 client code complete: Explore + Ordination + preferences + exports
  + theme persistence + app shell + dual transport.
- Phase 4 *exit* ("numerical and visual parity accepted on all
  sources/themes/viewports") still requires the Section 15.4 re-execution of
  procedures 6–8 and 12 against golden fixtures (backend side already
  ratified per [[Phase_4_UMAP_Naive_Port_Golden_07_2026-09-20]] and
  [[Phase_4_Explore_Views_Golden_12_2026-09-19]]); visual-parity acceptance is
  a later gate, not more client code.
- Next: Phase 5 — Visualize & Assign (interactive plot, lasso selection keyed
  by analytical_uuid, ellipses/symbols/labels, assignment transaction,
  multiplots with the 100k-point deterministic sampling ceiling).

Back to [[Interaction_Log_2026-09-23]].
