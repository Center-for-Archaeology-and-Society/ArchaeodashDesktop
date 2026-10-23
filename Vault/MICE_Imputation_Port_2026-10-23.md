# MICE imputation port (2026-10-23)

Up: [[Index]]

## What
- The legacy imputation methods are back: random forest (`rf`), predictive mean matching (`pmm`), and weighted predictive mean matching (`midastouch`), chosen in the Data Manager's Analysis setup with a visible, editable seed (default 20260914). Stage 2 of the Data Manager proposals; see [[Analysis_Setup_Elements_Log_2026-10-23]].
- Imputation runs on demand inside the transformation engine (impute base predictors, then transform, then ratios); group files keep measured values.

## Port
- `crates/analysis/src/impute.rs`, transcribed from mice 3.16.0's R sources: sampler (random-observed-value init, left-to-right visits, maxit 5, `find.collinear` / constant-column exclusion, `remove.lindep` screening each step), `.norm.draw` + `matchindex` for pmm, the bootstrap-weighted leave-one-out midastouch, and a ranger-style 10-tree regression forest (mtry = floor(sqrt(p)), min node size 5, variance splits at midpoints) for rf.
- Random stream: seeded ChaCha12 (parity class D: R's Mersenne-Twister and ranger's C++ generator are not replicated).
- `TransformService::apply_definition` imputes when `imputation_method != none` (seed required); the old "imputation_not_gated" rejection is removed.

## Parity evidence (golden procedure 5)
- Oracle: `scripts/capture_imputation_distribution.R` (R 4.3.3 on home2, mice 3.16.0 + ranger 0.16.0 via apt) masks 8% of the eight golden elements on the 307 complete INAA rows (209 cells) and records 100 seeded runs per method → `fixtures/golden/05_imputation_distribution.json`. (The original 05 golden came from mice 3.19.0 on R 4.6.1, an environment not on home2; the imputation code paths are unchanged between those versions.)
- `tests/parity/tests/golden_05_imputation.rs` (100 Rust seeds vs 100 R runs, against the R-vs-R half-split band):

| method | error ratio | centre corr | spread ratio | KS (R band) |
|---|---|---|---|---|
| pmm | 1.003 | 0.997 | 1.004 | 0.005 (0.010) |
| midastouch | 0.987 | 0.996 | 1.015 | 0.008 (0.018) |
| rf | 0.980 | 0.996 | 0.959 | 0.013 (0.008) |

- The rf parity test takes ~70 s in debug builds.

## Verification
- Unit tests: donors-only property, observed cells untouched, seed replay, constant/collinear exclusion, helper maths; engine test `apply_imputes_missing_cells_before_transforming`.
- Web: 139/139 tests; dialog checked in a browser.
