# Phase 4 UMAP Naive Port Golden 07 — 2026-09-20

Related: [[Phase_4_Ordination_PCA_LDA_Research_2026-09-17]], [[Phase_4_LDA_HTTP_Adapter_And_Group_Gate_Test_2026-09-18]], [[R_Oracle_Baseline_Capture_2026-09-14]], [[Implementation_Plan_Open_Question_Recommendations_2026-09-09]]

## Summary

Ported legacy `umap::umap(method = "naive")` (R `umap` 0.2.10.0) to
`crates/analysis/src/umap.rs` (procedure 7, class D), added the golden-07
parity test, wired UMAP through the contracts/application/HTTP adapters
(same scope as the PCA/LDA slice; the desktop/Tauri layer has no ordination
commands yet), and recorded the Section 17.1 item-3 `umap_rs` spike decision.

## Spike decision (IMPLEMENTATION.md 17.1 item 3)

- `umap-rs` 0.4.x and similar crates not adopted: no legacy naive-method
  semantics, no caller-side determinism/validation guarantees.
- Hand-ported pipeline on `faer` (self-adjoint eigen, `Parallelism::Serial`)
  + seeded ChaCha12 standing in for MT19937 (spectral jitter, negative
  sampling) - documented class-D divergence.
- Class-D gate: R-vs-R cross-seed calibration band pdist corr 0.893-0.974,
  10-NN overlap 0.852-0.863; Rust measures 0.900-0.962 / 0.854-0.856 on seeds
  20260914-20260916 vs `fixtures/golden/07_umap.json`. Provisional thresholds
  corr >= 0.75, overlap >= 0.70 hold with margin; plus same-seed
  bit-identical determinism test.

## Key findings

- **RSpectra `eigs(k = d + 1, which = "SM")` returns the selected eigenpairs
  in DESCENDING modulus order** (probed: 0.00874, 0.00357, ~0). R's
  `[, seq_len(d)]` therefore keeps the d largest-modulus of the d+1 smallest
  and DROPS the trivial near-null eigenvector. The first port sorted
  ascending and kept the null vector + lambda2 vector: column 2 correlated
  -0.9999999 with R (sign flip only) but column 1 correlated -0.10, giving
  parity corr 0.70-0.81 (below band). Reversing the picked order fixed it
  (corr 0.90-0.96, overlap 0.854+). Dense solvers (faer) sort ascending, so
  the reorder is explicit in `one_embedding`.
- Eigenvalues of the normalized Laplacian match R to <1e-6 (2.2e-16,
  0.0035720, 0.0087370, ...), confirming the graph construction.
- The legacy SGD epoch schedule (`adjust`/`eons`/`epns`/`eon2s` from
  `make.epochs.per.sample`), clip4 gradient clamping, and the negative
  sampling in C `optimize.cpp` were ported exactly; distribution-level
  parity confirms the trajectory statistics.
- faer eigenvector signs are arbitrary per column; pairwise-distance and
  k-NN metrics are invariant, so class-D metrics absorb the sign ambiguity.

## Implementation

- `crates/analysis/src/umap.rs`: knn_brute_force, smooth_knn_dist,
  fuzzy_simplicial_set, laplacian_smallest_eigenvalues, epochs_per_sample,
  find_ab_params (a=1.5769436126945664, b=0.8950607181519281 pinned to the R
  grid search), umap() + Umap/UmapConfig, DEFAULT_SEED = 20260914; unit
  tests pinned to R probe values.
- `tests/parity/tests/golden_07_umap.rs`: pinned pipeline stages (knn row 1,
  sigma/rho rows, edge count 5598, Laplacian eigenvalues, a/b), determinism,
  and the 3-seed distributional parity.
- `crates/contracts`: UmapRequest (path, columns, transformation, optional
  seed defaulting to DEFAULT_SEED) / UmapResponse (score_names V1/V2,
  embedding, config echo, warnings) + round-trip tests.
- `crates/application/src/ordination.rs`: OrdinationService::umap (empty
  column gate `ordination_empty`, NaN gate `ordination_missing_values`,
  legacy k-in-2..n `umap_neighbors`, <2048 rows `umap_input_too_large`).
- `crates/api`: POST /api/v1/ordination/umap + round-trip/determinism/422
  test (20-row synthetic group; legacy n_neighbors=15 needs >15 rows).
- Gates: cargo fmt --check, clippy -D warnings, all 43 workspace test suites
  green.

## Deferred

- Desktop/Tauri ordination commands (PCA/LDA/UMAP all still HTTP-only,
  matching the existing slice scope).
- Trustworthiness metric (manifest lists only pdist corr + knn overlap for
  golden 07).
