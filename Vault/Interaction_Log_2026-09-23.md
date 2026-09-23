# Interaction Log — 2026-09-23

- Continued Phase 4 client work. Slice D: Ordination route (PCA variance bars/scatter/score table + Export PCA scores CSV button, UMAP with fixed seed 20260914 echo, LDA gated on descriptive group column), pure `PcaResult`/`UmapResult` components for SSR tests, theme persistence via `preferences.put('theme')`. All gates green (11 web tests, 151 Rust). Committed `739d268`. Note: [[Phase_4_Client_Ordination_Prefs_Exports_2026-09-23]].
- Slice E: `lastOpenedDataset` preference hydrate-on-mount/persist-on-select wired through `main.tsx` → `ExplorePage` (`initialDataset`/`onDatasetOpened`); `apps/web/src/exports.ts` `downloadExportResult` (Blob download of ExportResult); Explore "Export measured data (CSV)" button. All gates green. Committed `4f01162`.
- Phase 4 client surface now complete (Explore + Ordination + preferences + exports + shell + dual transport). Next: Phase 5 Visualize & Assign.
