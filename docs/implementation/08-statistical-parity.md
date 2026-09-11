# IMPLEMENTATION Section 8 - Transformation and statistical parity specification

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 8. Transformation and statistical parity specification

### 8.0 Parity classes

Every statistical output is assigned exactly one parity class, approved at Phase 0 and recorded in the golden manifest (Section 15.4). A method may not silently move between classes; reclassification requires the parity sign-off in Section 15.3.

- **Class E (exact):** IDs, strings, schemas, counts, row/selection filtering, assignments, deterministic integer outputs, and rounded outputs whose rounding is part of the behavioral contract (for example `zScore` three-decimal output and the log/log10 rounding plus non-finite-to-zero rule).
- **Class T (tolerance):** deterministic floating-point results compared with documented absolute/relative tolerances, sign/axis alignment, and label-permutation alignment: PCA eigenvalues/scores/loadings, LDA priors/scaling/scores, Hotelling and Mahalanobis values, Euclidean distances, and seeded k-means/k-medoids/WSS/silhouette outputs.
- **Class D (distributional):** stochastic methods where the current R application records no seed and/or no pinned Rust equivalent exists: UMAP embeddings and MICE `pmm`/`midastouch`/`rf` imputation. Parity targets the documented algorithm and output distribution — verified through fixed-seed goldens on the new implementation, neighborhood/Procrustes metrics, and distribution tests against R oracle outputs — never byte-identical trajectories with a historical R run.

All new implementations are seeded and record algorithm version, RNG kind, and seed with every result. Legacy unseeded R runs are documented as non-reproducible and are not parity targets; the R oracle captures what can still be captured (package versions, defaults, seeded re-runs) per Section 15.4.

### 8.1 Pipeline order

The Rust `TransformationEngine` must preserve this order:

1. validate the named transformation, schema, group field, selected groups, predictors, ratios, and algorithm prerequisites;
2. revalidate and read the selected group Parquet files, de-duplicate/resolve overlapping `analytical_uuid`s, then select ratio source columns, predictor fields, and requested metadata;
3. coerce predictors to `f64` with a structured parse report;
4. apply the import-time NA-as-zero rule where configured;
5. run imputation on base predictors only;
6. add ratio columns (`numerator / denominator`; null when denominator is null or zero);
7. resolve final predictors: base + ratios for `append`, ratios only for `only`, and warn/fall back to base fields if ratio-only has no valid ratios;
8. apply the selected transform;
9. retain the unfiltered `selected_all` table only in job memory or disposable cache;
10. filter rows to selected group values and normalize grouping levels;
11. compute requested PCA, UMAP, and LDA results;
12. save only the definition, ordered input group checksums, configuration, algorithm version, and seed; keep computed matrices/results ephemeral unless the user explicitly exports a separate result file;
13. refresh UI/resource queries and show a completion/error result.

Never autosave transformed predictor values into group Parquet files. Log/log10, compositional/z-score, ratios, imputations, permutations, normalized/scaled matrices, and model inputs are recomputed on demand for each analysis. A cache is disposable and must not carry group-profile metadata. Current code's removal of `imputation` and `transformation` on autosave becomes a strict storage invariant rather than a UI convention.

### 8.2 Ratios

- `RatioSpec { output_column_id/name, numerator_column_id, denominator_column_id }`.
- Support one-to-one and Cartesian batch creation from multiple numerator/denominator selections.
- Exclude numerator-equals-denominator combinations.
- Normalize custom single-ratio names; batch names are deterministic `numerator_denominator` equivalents.
- Reject duplicate/colliding output names unless an explicit overwrite decision is made.
- Persist specs as structured JSON, replacing the unit-separator/pipe encoding.

### 8.3 Imputation

The present choices are `none`, random forest (`rf`), predictive mean matching (`pmm`), and weighted predictive mean matching (`midastouch`) via one completed `mice` dataset.

Rust does not have a drop-in MICE implementation that should be trusted without validation. Implement an `Imputer` trait and gate each method separately:

- `none`: direct parity.
- `pmm`: chained equations, regression prediction, nearest donor matching, deterministic seeded donor choice, configurable burn-in/iterations; document defaults captured from the current R environment.
- `midastouch`: weighted donor selection using the documented MIDAS touch distance/weighting; validate distributional and missing-cell outputs against fixed R seeds.
- `rf`: per-target chained imputation using a pinned, audited Rust random-forest regressor; categorical handling is irrelevant for the selected numeric predictor matrix but missing predictors and constant columns must be specified.

Before implementation, the oracle tool must export the exact current MICE package version, default arguments, RNG kind, and seeded golden outputs for small/large missingness fixtures. The new UI adds a visible/replayable seed. A method stays behind an “experimental parity” flag until all golden, property, and statistical-distribution tests pass. Do not silently map one method to another. The final production runtime contains no R oracle/service; R remains CI/migration tooling only.

### 8.4 Transformations

- `none`: retain numeric values.
- `log10`: base-10 log, round to three decimals, then convert non-finite results to zero for compatibility; also return warning counts because this behavior can conceal invalid inputs.
- `log`: natural log with the same rounding/non-finite rule.
- `zScore`: exactly preserve the current unusual semantics: convert each row to proportions of its row sum, multiply by 100, then standardize each column across rows and round to three decimals. Name it “Compositional percent + column z-score” in help while keeping `zScore` as the migration identifier.
- Constant columns, zero row sums, all-null fields, overflow, and NaN/Infinity receive explicit error/warning codes and golden tests.

### 8.5 PCA

- Match `stats::prcomp` defaults used by the app: centered, not scaled, SVD-based PCA over final predictors.
- Persist center, singular values/standard deviations, rotation/loadings, scores, explained variance, cumulative variance, predictor order, and sign convention metadata.
- Component signs are mathematically arbitrary. Parity tests align signs before comparing scores/loadings and compare reconstruction/explained variance within tolerance.
- UI exposes individual score plot colored by quality/cos², variable loading/contribution plot, eigenvalue plot, contribution sum over the first up-to-four PCs, axis variance labels, and PC-count labels `(individual variance / cumulative variance)`.
- Linfa documents a tested PCA implementation, but a numerical spike must confirm semantics and BLAS portability before adoption; otherwise implement on a pinned linear-algebra backend. See [Linfa capabilities](https://rust-ml.github.io/linfa/about/).

### 8.6 UMAP

- Capture current R `umap::umap` defaults and seed behavior before porting; the current app does not record a seed, so existing runs are not reproducible.
- New definitions persist neighbor count, components, metric, minimum distance/spread or their actual library equivalents, initialization, epochs, and seed.
- `umap_rs` is a candidate only after a spike: its documentation states that callers must provide KNN and initialization, it currently fits dense data only, lacks transform-for-new-points, and can panic on invalid input. Wrap validation, isolate panics at a safe job boundary, and provide deterministic KNN/init. See [umap_rs limitations](https://docs.rs/umap-rs).
- Preserve output naming/migration aliases `V1`, `V2`, while exposing stable dimension IDs.
- Add an actual UMAP view to Ordination or remove the orphaned server header. Recommended decision: add the view because UMAP is a first-class existing analysis source in Visualize, Cluster, Membership, and Euclidean Distance.

### 8.7 LDA

- Preserve the product rule requiring at least three nonblank groups.
- Fit classical linear discriminant analysis using the selected group and final predictors, matching MASS priors/covariance/scaling defaults captured by the oracle.
- Persist group priors, means, scaling coefficients, singular values, scores, and predictor/group order.
- Reproduce the current centered-score calculation and LD column names; validate scores modulo sign/axis ambiguity.
- LDA vector view requires at least LD1/LD2; handle fewer dimensions with a clear alternate view instead of indexing failure.

### 8.8 Membership probabilities and Mahalanobis

- Eligibility remains `group_size > max(number_of_features, number_of_groups) + 1`.
- Sources remain elements, selected PCA components, UMAP dimensions, or linear discriminants.
- Hotelling mode removes the observation from its own comparison group, calculates the applicable T² p-value, rounds to five decimals, multiplies by 100, and selects the largest value as `BestGroup`.
- If Hotelling computation is unavailable for a numerical reason, current behavior falls back globally/recursively to Mahalanobis. The new result must state `requested_method`, `actual_method`, and fallback reason rather than silently changing interpretation.
- Mahalanobis mode removes unusable/constant columns, uses complete group rows, calculates squared distance to group centroid with covariance regularization fallback, uses infinity for non-computable groups, and selects the smallest distance.
- Preserve result fields: `ID`, `Group`, `GroupVal`, `BestGroup`, `BestValue`, `InGroup`, `ProjectionIncluded`, one column per eligible group, and hidden internal `analytical_uuid`.
- Port and retain the separately tested two-mean `calculate_mahalanobis_distance` formula as an internal analysis utility even though no current UI calls it.

### 8.9 Euclidean nearest matches

- Preserve sources, selectable PC count, projection groups, unique visible ID fallback to displayed row number, within-group toggle, top-N limit (current UI 1–100), and assignment paths.
- Match current semantics: candidates are rows in projection groups; exclude self; optionally exclude same-group matches; sort ascending; return up to N per observation.
- Return hidden `analytical_uuid`, observation display ID when selected, match display ID, distance, observation group, and `<group>_match` equivalent.
- Replace the current full `n x n` distance matrix with blocked/vectorized top-k search to bound memory. Exact Euclidean results are required initially; approximate nearest neighbors require a later opt-in ADR.

### 8.10 Clustering

Preserve these algorithms and controls:

| Current choice | Required new behavior |
|---|---|
| optimal count | k-means and k-medoids elbow/WSS plus silhouette series; record evaluated k range and invalid-k handling |
| HCA | Euclidean, Manhattan, Minkowski, Maximum distances; Average, Complete, Ward.D, Ward.D2 linkage; leaf text size; cut k; horizontal colored dendrogram |
| HDCA | DIANA-compatible divisive clustering with Euclidean/Manhattan, cut k, leaf size, horizontal dendrogram |
| k-means | centers 1–20, starts 1–100, max iterations 1–200, deterministic seed; plot colored by new clusters or existing groups |
| k-medoids | PAM-compatible Euclidean/Manhattan, k 1–20, deterministic tie behavior; same color choice |

Candidate libraries are Linfa for k-means, `kodama` for agglomerative clustering, and `kmedoids` for FasterPAM, but parity—not crate availability—decides final adoption. `kodama` exposes a stepwise dendrogram and multiple linkage methods; `kmedoids` explicitly implements a faster PAM family. See [Linfa clustering](https://rust-ml.github.io/linfa/rustdocs/linfa_clustering/), [kodama](https://docs.rs/kodama/latest/kodama/), and [kmedoids](https://docs.rs/kmedoids/latest/kmedoids/).

DIANA, Ward.D compatibility, factoextra diagnostic defaults, and dendrogram cut/color ordering require custom adapters or implementation if candidates do not match R. Cluster output always carries hidden `analytical_uuid`; recording assignments moves analytical units between group files only after confirmation and creates validated group revisions. It never writes calculated values into a group file.
