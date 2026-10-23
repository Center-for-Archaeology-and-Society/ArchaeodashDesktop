# Precomputed PCA/UMAP/LDA with a disk cache (2026-10-23)

Up: [[Index]]

## What
- Analysis setup dialog: "Precompute in the background" checkboxes for PCA, UMAP, and LDA (stored as `precompute` on the `analysis_setup` definition). Stage 3 of the Data Manager proposals; see [[Analysis_Setup_Elements_Log_2026-10-23]] and [[MICE_Imputation_Port_2026-10-23]].
- After the setup is saved, and whenever group files change, the Data Manager runs the chosen ordinations for all ready groups and shows "✓ PCA ready · UMAP computing… · LDA skipped (needs three or more groups)". Ordination and Visualize then get those results instantly instead of computing on the fly.

## Design
- Backend cache (`crates/application/src/ordination.rs`): `pca`/`lda`/`umap` look up `.archaeodash/cache/ordination/<kind>/<sha256>.json` before computing and store results after. Key = `CACHE_VERSION` + crate version + kind + the serialized request + each input file's path, size, and modification time. Any rewrite of an input file changes the key, so stale results are never served; no file is read on a hit. At most 200 entries per kind (oldest pruned); writes are atomic and best effort. Disposable per Section 6 (`CacheStore`); never group files.
- The cache helps every caller (desktop, local API), not only precompute.
- Frontend: `apps/web/src/analysis-setup/precompute.ts` plans exactly the requests the pages send for the default "all groups" selection (ready groups in scan order, columns every group shares, setup transformation, `UMAP_SEED`, LDA both by file (Visualize) and by the synthetic Group column (Ordination)), so pages hit the cache. Runs PCA, LDA, then UMAP (slowest last).
- Contract: `TransformationDefinition.precompute: Vec<OrdinationKind>` (`pca`/`umap`/`lda`), omitted from JSON when empty so existing definitions are unchanged.

## Limits
- Cluster/Probabilities/Euclidean compute PCA/UMAP/LDA sources inside their own analysis jobs and do not use this cache yet.
- A different Dataset selection on a page is a different request (computed on the fly, then cached).

## Verification
- Rust: `results_are_cached_per_request_and_input_file_state` (hit returns the stored entry, new request → new entry, rewritten file → recompute, group file untouched); application/contracts/desktop/api/parity tests pass; fmt and clippy clean.
- Web: 144/144 tests (planner requests, skip reasons, job calls, dialog checkboxes).
- Browser (stubbed API): saving PCA+UMAP+LDA ran pca, lda×2, umap in order and the panel showed all three ready.
