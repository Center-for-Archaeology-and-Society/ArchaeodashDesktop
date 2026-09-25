# Phase 6 Membership + Euclidean Goldens (2026-09-24)

## Summary
Implemented procedures 10 and 11 of the Section 15.4 baseline suite as
`crates/analysis/src/membership.rs`, with golden tests
`tests/parity/tests/golden_10_membership.rs` and
`tests/parity/tests/golden_11_euclidean.rs`. Both pass against the
pre-captured R oracle fixtures.

## What landed
- `get_eligible`: dplyr `group_by |> count` yields sorted character keys
  (golden order `D1,D2,D3a,D3b,D4`, not first-appearance); threshold
  `n > max(n_features, n_groups) + 1`.
- `group_mem_probs` Hotellings path: two-sample T2 with n1 = 1, pooled cov
  `=(cross(X.diff)+cross(Y.diff))/(n1+n2-2)`, `p = 1 - pf(T2, p, n1+n2-p-1)`
  via continued-fraction incomplete beta (validated against R `pf` to 1e-12),
  cell = `r_round(p, 5) * 100`. Any NaN cell or `solve` failure falls back to
  the whole-table Mahalanobis path (the legacy `tryCatch` is whole-table,
  not per-cell).
- `solve` port: LU with partial pivoting; fails on an exact-zero pivot or
  rcond `1/(||A||1 * ||A^-1||1) < tol` (R probed: `solve(B, diag(2), tol=1e-8)`
  → "computationally singular").
- Mahalanobis path: finite-column filter → `complete.cases` (drops NaN, keeps
  Inf) → zero-variance drop → `solve(cov, tol=1e-8)` → `+diag(1e-8)` retry →
  `Inf`; non-finite cells become `Inf` before best-group (first index wins
  ties in `which.max`/`which.min`).
- `calc_e_distance`: full `dist` matrix, `as.table` column-major expansion
  (Var1 slowest), self-pairs dropped, per-observation stable sort by distance
  with NaN last, `slice_head(limit)` before the `withinGroup = FALSE`
  cross-group filter, final stable `arrange(anid, distance)` (C locale).
- ICSNP was not installed locally, so `HotellingsT.R`/`HotellingsT.internal.R`
  were extracted from the CRAN tarball (v1.1-3, the version pinned in
  `fixtures/golden/manifest.json`) for the oracle probes; a base-R probe
  reproduced both goldens with 0 mismatches before the Rust port was written.

## Results
`golden_10_membership` (307×2 probability rows: eligibility/ID/BestGroup/InGroup
exact, values within the 4dp serialization tolerance) and `golden_11_euclidean`
(686 rows, keys and order exact) pass; all 27 `archaeodash-analysis` unit tests
pass; clippy clean on the lib.

## Open items
- The fixture exercises only the clean Hotellings path (no NaNs); the
  Mahalanobis fallback ladder is covered by unit probes, not a golden.
- Phase 6 remaining: HTTP/Tauri adapter exposure of cluster/membership/
  Euclidean use cases, then the Phase 6 exit gate (procedure 13 multiplot
  sampling is client-side and already covered by Phase 5 Slice B).

## Related
- [[Golden_09_Clustering_Kmeans_Pam_Parity_Fix_2026-09-24]]
- [[Golden_09_Clustering_Hclust_Diana_Parity_Fix_2026-09-24]]
- [[Cluster_Workflow]]
- [[Interaction_Log_2026-09-24]]
