# PAM, hierarchical and DIANA beyond 1,000 rows (2026-10-26)

Up: [[Index]]

## Problem
A 26,880-row group was rejected: "exceed the 1,000 row limit for PAM, hierarchical, and DIANA clustering". The cap (`MAX_DISTANCE_CELLS` = 1,000,000) protected n x n matrices:
- PAM used a full n x n `Vec<Vec<f64>>`: 5.8 GB at 26,880 rows.
- Hierarchical and DIANA used R's half table: 2.9 GB.
- DIANA's `twins.c` split loop is O(n^3): 18 s at 2,000 rows, 192 s at 4,000, about 16 h at 26,880.

## Changes (`crates/analysis/src/cluster.rs`, `pairwise.rs`)
- **PAM**: matrix-free and parallel.
  - Distance rows are computed on demand; only the k medoid rows are kept, so memory is O(k n).
  - Candidate h runs in parallel blocks; blocks are combined in order, so ties pick the same swap as the sequential scan.
  - Bit-identical to the old dense PAM: `pam_dense_reference` is kept for tests, and the comparison covers tied distances.
- **Hierarchical** (`hclust.f` NN-chain, Ward.D2 path) and **DIANA**:
  - The half table is built in parallel in place (`par_fill_rows`), with the same per-entry arithmetic.
  - Allocation goes through `try_zeroed` (`try_reserve_exact`): failure is a `cluster_memory` error, not a process abort.
- **DIANA** above `DIANA_EXACT_ROWS` (2,000):
  - Each split keeps every object's sums to both parts and updates them per move: O(m) per move instead of O(m^2).
  - The diameter for the banner comes from the same pass.
  - At or below 2,000 rows, R's loop runs unchanged (golden parity).
  - Incremental sums can differ from R's in the last bits; a test shows identical trees on separated data.
- DIANA merge reconstruction is now O(n log n) (sorted banners plus union-find) instead of an O(n^2) scan; equivalent, checked by the golden test.
- `within_pair_ss_cancellable`: PAM diagnostics' factoextra within-SS is streamed in the same summation order (bit-identical), with no n x n matrix.

## Application (`crates/application/src/clustering.rs`, `analysis_jobs.rs`)
- PAM, like k-means, computes distances on the fly: up to 100,000 rows.
- Hierarchical and DIANA: up to 100,000 rows, but the half table (8 bytes per pair, `table_bytes`) must fit in 80% of free memory (`sysinfo`, `available_memory_bytes`).
  - Otherwise `cluster_memory` reports the table size and free memory, and suggests k-means or PAM.
  - 100,000 rows needs 40 GB, so in practice the limit is memory.
- Job result cap: 8 MiB to 256 MiB.
  - A 100,000-row k-means fit is about 15 MB.
  - Euclidean matches return up to `limit` rows per sample (about 40 MB at 26,880 x 10).
  - Finished results kept together are capped at 512 MiB; the oldest finished jobs are dropped first.
- New dependency: `sysinfo` 0.39 (system feature only), because `unsafe_code` is forbidden workspace-wide.

## Web
- `dendrogram-model.ts`:
  - Layout is now linear in n (per-node spans and union-find; the old leaf-list copies were O(n^2)).
  - Above 1,000 samples it draws the top 200 branches (more if the cut asks, up to 1,000). Collapsed branches are dashed and labelled with their sample count.
  - `clusterOfRow` gives every sample's cut cluster; the cut table and the assignment helpers use it.
- A 30,000-sample tree lays out in about 70 ms.

## Timings (home2, 4 cores, 26,880 rows x 30 columns, release)
| Method | Time | Peak memory |
|---|---|---|
| Average linkage | 37 s | 2.8 GB |
| Ward.D2 | 46 s | 2.8 GB |
| PAM k=6 | 120 s | 14 MB |
| DIANA | 39 s | 2.8 GB |

DIANA at 4,000 rows: 0.6 s (was 192 s). The 16-core desktop should be faster for the parallel parts.

## Not changed
- PAM time still grows with n^2: about 30 min at 100,000 rows on home2. The job timeout (600 s default, 1,800 s max) applies.
- The 2026-09-28 Phase 6 validation doc still describes the old 1,000-row limit as of that date.

Related: [[Clustering_Row_Limit_100k_2026-10-26]], [[UMAP_Row_Limit_100k_2026-10-26]], [[Interaction_Log_2026-10-26]]
