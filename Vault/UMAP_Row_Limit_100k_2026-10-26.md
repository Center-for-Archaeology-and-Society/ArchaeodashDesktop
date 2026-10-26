# UMAP row limit raised to 100,000 (2026-10-26)

Up: [[Index]]

## Why
- UMAP rejected inputs of 2,048+ rows (`umap_input_too_large`). Two steps held full V x V tables: the brute-force neighbour search (distance matrix) and the spectral starting layout (dense eigendecomposition, O(V^3)). R's own `umap` switches to approximate neighbours at 2,048 rows, which was never ported.

## What changed (`crates/analysis/src/umap.rs`)
- Neighbour search: exact, parallel, tiled (reuses `pairwise.rs` from [[Clustering_Row_Limit_100k_2026-10-26]]). Keeps only each row's best 15, so memory is O(V * k). Used for every input size; a test checks it matches the dense `knn_brute_force` exactly (indexes and distance bits, with ties).
- Starting layout: connected components under `SPARSE_SPECTRAL_ROWS` (2,048) keep R's dense eigendecomposition, so R-parity results under 2,048 rows are unchanged. Larger components use faer's matrix-free Krylov-Schur solver (`faer::matrix_free::eigen::partial_self_adjoint_eigen`) on `I + D^-1/2 W D^-1/2`; its largest eigenvalues are the smallest of the normalized Laplacian. Fixed start vector, serial, so a seed always gives the same layout.
- If the sparse solve does not converge: PCA of the component's rows (warning "spectral initial embedding did not converge; using PCA embedding instead"), then the legacy random layout if PCA fails.
- Components are grouped in one pass (was a filter over all rows and edges per component). Same output.
- SGD reuses one buffer instead of allocating per edge and per negative sample. Same arithmetic.
- Limit: `UMAP_MAX_ROWS` = 100,000. UMAP as a clustering source (`clustering.rs`) now uses the same limit (was the ~1,000-row dense cap).

## Verification
- Tests: tiled kNN equals dense kNN; sparse eigenvalues match dense within 1e-6 with small residuals; 2,100-row single-component run converges with no fallback warning; 3-blob 2,100-row layout keeps >= 95% of nearest neighbours in their blob; 100,001 rows rejected. Workspace tests (UMAP golden parity included), clippy gate and fmt pass on home2.
- Timing (home2, 4 cores, 30 columns, release build, no fallback warnings):
  - 30,000 rows, 6 clusters: 38 s. 30,000 rows, 1 cluster: 43 s.
  - 100,000 rows, 6 clusters: 195 s (about 3.3 min).

## Notes
- Ordination UMAP (`ordination.rs`) calls `umap`, not `umap_cancellable`, so a large run cannot be cancelled from that path. Results are cached per input.

Related: [[Interaction_Log_2026-10-26]], [[Phase_4_UMAP_Naive_Port_Golden_07_2026-09-20]]
