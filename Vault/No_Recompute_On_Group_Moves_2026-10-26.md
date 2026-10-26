# No recompute when samples change group (2026-10-26)

Up: [[Index]]

## Problem
Assigning samples to another group on Visualize recomputed UMAP (minutes at 100k rows), even though UMAP does not depend on group membership. Causes:
- The ordination cache was keyed by each input file's path, size and modification time, and any move rewrites the files.
- Pages re-ran their effects on every reload (`files`/`combined` change) and matched results to rows by position.
- Clustering with a UMAP source computed UMAP itself with no cache: twice per diagnostics + fit, and again for membership or matches.

## Rule
Results depend on one of two things:
- **Content**: which samples are loaded (analytical UUID) and their measured values, plus the settings. These survive moves: PCA, UMAP, element transforms (including seeded imputation), cluster fits and diagnostics, and histograms.
- **Membership**: content plus each sample's group. These rerun after a move, which is correct: LDA by group file, membership probabilities, Euclidean matches, and the Explore profile by group.

## Backend (`crates/application/src/ordination.rs`)
- PCA and UMAP run over the pooled rows in a canonical order (ascending UUID; copies tie by position), so the result is the same however rows are split across files. They are returned in request row order.
  - Transforms and seeded imputation also run in that order.
- Cache key is the content: kind, settings (`PcaKey`/`UmapKey`), elemental column names, and each row's UUID and value bits. Paths, file modification times and descriptive values are not part of it. `CACHE_VERSION` is now `ordination-cache-2`.
- `path`/`revision_id` in a cached response are overwritten from the current request.
- Single-flight per cache entry (`cached_or_compute`): Visualize and the Data Manager precompute asking at once compute once.
- `cached_umap_layout` lets clustering's UMAP source (`clustering.rs` `source_matrix`) share the same entries as the Visualize/Ordination UMAP with the same seed.
- LDA keeps the file-keyed cache, since it depends on membership.
- `serde_json` `float_roundtrip` (application crate): cached floats read back bit-identical to fresh ones. Without it the last bit can differ.

## Web
- `apps/web/src/datasets/rowKeys.ts`:
  - `rowContentKey`: order-independent hash of UUIDs, values and column names.
  - `rowGroupKey`: the same plus each row's group.
  - `realignByUuid` / `realignRowsByUuid`.
- Visualize:
  - Scores and transformed values are requested on content key + setup (LDA on group key), stored with their row UUIDs, and realigned to the current rows.
  - The selection clears only when the samples change.
  - No forced full reload after a move; the rescan reloads only files whose revision changed.
- Ordination: the PCA/UMAP views are keyed on content and realigned by UUID. Panels remount only when the samples change, not on file revisions.
- Analysis:
  - Cluster fit/diagnostics results are kept after a move, so you can assign more clusters without recomputing. Membership/matches (and an LDA source grouped by file) still clear.
  - Settings reset only when the samples or the setup change.
  - The PCA used for component labels is keyed on content.
  - No forced reload.
- Explore: histograms are cached by content key, not by file revisions or paths.
- `useGroupFiles` listens to `notifyGroupsChanged` and rescans. Pages kept alive in the background no longer show stale rows after another page moves samples.

## Not changed
- Sidebar (Data Manager) structural edits still remount the routed pages (`AppShell` `dataVersion`). PCA/UMAP then come from the cache, but page settings reset.
- The precompute still re-requests after group changes; PCA/UMAP are cache hits, and LDA reruns.
- Cluster-plot group colors from a kept fit reflect the groups at fit time.

## Verification
- Tests:
  - `moving_rows_between_groups_reuses_pca_and_umap`: a move, reversed file order, and clustering's layout all hit one entry with identical per-sample values.
  - `content_key_tracks_rows_and_values_but_not_files_or_labels`.
  - Updated `results_are_cached_per_request_and_row_content`.
  - `rowKeys.test.ts`.
- Workspace tests, clippy, fmt, golden parity and web typecheck/tests pass on home2.

Related: [[UMAP_Row_Limit_100k_2026-10-26]], [[Clustering_Row_Limit_100k_2026-10-26]], [[Interaction_Log_2026-10-26]]
