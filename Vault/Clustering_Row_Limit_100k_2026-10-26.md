# Clustering row limit raised to 100,000 (2026-10-26)

Up: [[Index]]

## Why
- A 29,706-row group file failed clustering with "rows exceed the pairwise distance limit". The old cap was 1,000,000 distance cells (about 1,000 rows) for every clustering method, because silhouettes and diagnostics built a full n x n `Vec<Vec<f64>>` matrix (80 GB at 100,000 rows).

## What changed
- `crates/analysis/src/pairwise.rs` (new): computes distances on the fly in cache-sized tiles and spreads row blocks over all cores (`std::thread::scope`, no new dependency). Distances are summed over columns in the same order as the dense helpers, so results are bit-identical.
- `silhouette_widths_streaming_cancellable` (analysis `cluster.rs`): row silhouettes for several clusterings in one distance pass, memory linear in rows. A test checks it matches the dense `silhouette_widths` bit for bit for all four metrics.
- k-means: the 25 random starts now run in parallel (centres still drawn in R's order, best still picked in start order, so results are unchanged). `unique_rows` is now hash-based instead of quadratic.
- Euclidean matching (`calc_e_distance_cancellable`): parallel, keeps each row's top `limit` then merges per rowid key; same output order as before.
- `ClusterService` limits (`crates/application/src/clustering.rs`):
  - k-means fit, k-means diagnostics, Euclidean matching: up to `MAX_ANALYSIS_ROWS` = 100,000 rows.
  - PAM, hierarchical, DIANA, PAM diagnostics, UMAP source: unchanged dense cap (`MAX_DISTANCE_CELLS` = 1,000,000 cells, about 1,000 rows); the error now says so and points to k-means.
  - Analysis matrix cap `MAX_INPUT_CELLS` 4M -> 10M cells; group-file read cap is now `MAX_READ_CELLS` = 20M cells (it counts descriptive columns too).
  - Membership result cap: rows x eligible groups <= 10M (was 1M).

## Verification
- On home2: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` (desktop app excluded: no GTK there), `cargo test --workspace` all pass, golden parity suites included.
- Timing (home2, 4 cores, 30 columns, release build):
  - 30,000 rows: k-means k=2..10 x 25 starts 41 s total; one silhouette pass 6 s; 9 silhouettes in one pass 8 s.
  - 100,000 rows: k-means k=2..10 x 25 starts 82 s; one silhouette pass 68 s; 9 silhouettes in one pass 90 s.
  - So a 100k-row k-means fit is about 1.5 min and full diagnostics a few minutes, inside the 10-minute default job timeout (`analysis_jobs.rs`).

## Not done
- PAM / hierarchical / DIANA still need the full matrix. Scaling them would need different algorithms (for example CLARA-style sampling for PAM).

Related: [[Interaction_Log_2026-10-26]], [[Golden_09_Clustering_Kmeans_Pam_Parity_Fix_2026-09-24]]
