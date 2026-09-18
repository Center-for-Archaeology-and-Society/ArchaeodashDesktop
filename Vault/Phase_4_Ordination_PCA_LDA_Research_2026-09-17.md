# Phase 4 Ordination PCA LDA Research 2026-09-17

Up: [[../IMPLEMENTATION]]

## Status snapshot

Phase 2 is complete (group ops + file catalog through both adapters, commit
`7cbde63`). Phase 3 is partially complete (transform definitions, batch
ratios, ephemeral apply — commit `d2b416c`); MICE imputation (`pmm`,
`midastouch`, `rf`) remains gated behind `imputation_not_gated` until the R
oracle exports its §8.3 fixtures. Workspace: 103 tests green, fmt/clippy
clean.

Phase 4 (Ordination and Explore) is next. First slice: **PCA and LDA with
golden parity tests** (goldens #6 and #8, class-T tolerance with sign/axis
alignment). UMAP (class D) needs the `umap_rs` spike and comes later; Explore
views are a separate slice.

## Golden fixture contents (verified)

`fixtures/golden/06_pca.json`: `sdev` (8 values), `rotation` (8×8),
`scores` (307 rows, PC1–PC8), `center` (8), `scale: false`.
`fixtures/golden/08_lda.json`: `prior` (5 groups), `means` (5×8),
`scaling` (8×4 → rank 4), `svd` (4 values), `scores` (307 rows, LD1–LD4).
Both from `INAA_test.csv` (307 rows, first 8 default chem columns, group
column `CORE`, 5 levels), R 4.6.1, config in `manifest.json`.

## Oracle semantics (extracted from `tools/legacy-export-r/capture_baselines.R` and `MASS:::lda.default`)

PCA (capture 6, `ordinationTab.R`): `stats::prcomp(features, center=TRUE,
scale=FALSE)` — center = colMeans; rotation/scores from the SVD of centered
X; `sdev = singular_values / sqrt(n-1)`; `scores = Xc %*% rotation`.

LDA (capture 8, moment method — `lda.default` deparse, verbatim algorithm):

1. Group means `group.means` (per level), priors = level proportions in
   **sorted level order**, `ng` = number of levels, `n` = rows.
2. `f1 = sqrt(diag(var(x - group.means[g,])))` — R `var` uses the n−1
   denominator; error if any `f1 < tol` ("variable appears to be constant
   within groups").
3. `scaling0 = diag(1/f1)`.
4. Stage-1 SVD of `sqrt(1/(n-ng)) * (x - group.means[g,]) %*% scaling0` →
   `scaling1 = diag(1/f1) %*% V1 %*% diag(1/d1[1:rank])`, rank = count of
   `d1 > tol` (tol = 1e-4).
5. `xbar = colSums(prior %*% group.means)`; `fac = 1/(ng-1)`;
   stage-2 SVD of `sqrt(n*prior*fac) * scale(group.means, center=xbar,
   scale=FALSE) %*% scaling1` → `scaling = scaling1 %*% V2`, `svd =
   d2[1:rank2]`, `rank2 = count(d2 > tol*d2[1])`.
6. Scores (per `lda_capture` in the oracle script, NOT `predict.lda`):
   globally centered features `%*% scaling`, then each score column
   re-centered by its own column mean.

Sign-alignment note: SVD/eigenvector signs are arbitrary (LAPACK vs faer);
parity tests must flip signs deterministically (e.g., make each component's
max-|entry| loading positive) on both computed and golden values, then
compare with `assert_within_serialization` (5.1e-5, already in
`tests/parity/src/lib.rs`).

## faer 0.24.4 API (vendored source confirmed)

`MatRef::svd() -> Svd { U, V, S }` with `.s()/.u()/.v()` accessors,
`MatRef::thin_svd()`, `MatRef::self_adjoint_eigen(side)`; matrix building via
`Mat::from_fn`; multiplication via `*` with `Mat`/`MatRef`. Both PCA and the
LDA two-stage SVD are implementable without hand-rolled Jacobi.

## Next steps

1. `crates/analysis/src/pca.rs`: `Pca { sdev, rotation, scores, center,
   scale: Option<Vec<f64>> }` + `pca(m: &ColumnMatrix)` — reject rows with
   NaN (prcomp na-action semantics), center, thin-SVD, `sdev = d/√(n−1)`,
   `scores = Xc·V`.
2. `crates/analysis/src/lda.rs`: `lda(m, groups, min_groups=3)` replicating
   the MASS moment-method algorithm above exactly; `lda_scores` helper with
   the capture-script re-centering; reject <3 groups (legacy
   `validate_lda_groups` message) and constant-within-group columns.
3. Golden tests `tests/parity/tests/golden_06_pca.rs` and `golden_08_lda.rs`
   using `load_inaa()`/`numeric_frame`/`base_chem` helpers from
   `golden_transforms.rs`, with sign alignment.
4. Wire adapters: contracts DTOs (`PcaRequest/Response`, `LdaRequest/Response`
   with group column + optional `TransformationDefinition` input),
   `OrdinationService` in `crates/application`, routes
   `POST /api/v1/ordination/pca|lda`, Tauri commands — same pattern as the
   transform slice.
5. Verify (fmt, clippy `-D warnings`, workspace tests), vault note, commit,
   push.

Risk: if the golden `svd` values miss, re-check the stage-1 `sqrt(fac)`
factor — it rescales rows uniformly, so it only rescales `d1`, not `V1`,
propagating into `scaling` and downstream `svd` (a single scalar to tune).

## Related

- [[Phase_3_Transform_Definitions_2026-09-17]]
- [[Phase_2_Source_File_Catalog_2026-09-17]]
- [[Node_Rust_Migration_Implementation_Plan_2026-09-08]]
