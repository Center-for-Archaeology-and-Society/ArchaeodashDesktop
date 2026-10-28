# UMAP selectable dimensions (2026-10-28)

Up: [[Index]]

## Request
UMAP was fixed at 2 dimensions everywhere; clustering on a UMAP source rejected more than 2. Allow more, with a sensible (dynamic) limit.

## Limit
`umap_max_components(p) = clamp(p, 2, 10)` for `p` analysis columns (after ratios/transform):
- **Never more than the input columns**: extra axes past the data's own width add nothing.
- **At least 2**: the plotting default, also the legacy `n_components`.
- **At most 10** (`UMAP_MAX_COMPONENTS`): the usual ceiling when UMAP feeds clustering; SGD time grows linearly with dimensions and further axes add little structure.
- Rows are not a constraint: every UMAP input has more than `n_neighbors` (15) rows, and each connected component has at least 15, so the spectral init's `d + 1` eigenvectors always fit. A guard falls back if a component ever has `<= d` rows.

The web UI mirrors this as `umapMaxDimensions(columns)` in `analysis-setup/setup.ts`.

## Changes
- `archaeodash-analysis::umap`: `umap_with_components(m, seed, d, cancel)`; `umap_cancellable` = `d = 2`. Out-of-range `d` is `umap_components`.
- Disconnected-graph offsets: the old code built R's full `expand.grid` (`(m+1)^d` rows) then sorted it. That is fine at `d = 2` but explodes past it (e.g. 17 components at `d = 10` is 60M rows). `component_offsets` now enumerates by row-sum level and keeps only the first `n_components` rows, in the identical order (unit test checks it against the full grid for `d = 2..4`).
- Contracts: `UmapRequest.n_components: Option<u32>` (serde default; TS `n_components?`).
- Ordination cache key includes `n_components`, omitted when 2 so existing cached layouts stay valid. Each count is its own layout, not extra axes on the 2-D one.
- Clustering UMAP source: `pc_count` is the dimension count (`1` still means the first axis of the 2-D layout); limit is `umap_max_components` of the real input width. Shares the cache with the Ordination page at the same count.
- Ordination page: "Dimensions" select (2..max); X/Y axis selects appear past 2 dimensions.
- Clustering page: "UMAP dimensions" select (2..max), sent as `pc_count`; eligibility feature count uses it.
- Visualize and precompute stay 2-D (unchanged requests, same cache keys).
- Help text and demo fetch updated.

## Verification
- New tests: `component_offsets_match_full_grid_order`, `max_components_follow_input_width`, `embeds_into_more_dimensions` (5-D, disconnected graph, deterministic), `rejects_dimensions_outside_the_range`; ordination cache test covers a 3-D layout shared with clustering and the `umap_components` error; clustering test covers the dimension limit; web tests cover the axis selects and `pc_count` for UMAP.
- home2 `check.sh rust` (analysis, contracts, application, api, desktop): fmt, tests, clippy pass. `check.sh web`: typecheck + 297 tests pass. `check.sh parity`: golden UMAP (07) and the rest pass, so 2-D output is unchanged.
- Not tried by hand in the desktop app (exe not rebuilt).

Related: [[UMAP_Row_Limit_100k_2026-10-26]], [[Phase_4_UMAP_Naive_Port_Golden_07_2026-09-20]], [[Ordination_Precompute_Cache_2026-10-23]]
