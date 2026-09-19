//! LDA with exact `MASS:::lda.default` moment-method semantics (golden
//! parity #8, class T; IMPLEMENTATION.md Section 15.4). The algorithm
//! follows the R 4.5.x deparse:
//!
//! 1. factor levels in sorted order; priors are level proportions;
//! 2. `f1 = sqrt(diag(var(x - group.means[g,])))` with the n-1 denominator,
//!    rejecting columns with `f1 < tol` (legacy "constant within groups");
//! 3. stage-1 SVD of `sqrt(1/(n-ng)) * (x - group.means[g,]) %*% diag(1/f1)`;
//!    `scaling1 = diag(1/f1) %*% V1 %*% diag(1/d1[1:rank])`;
//! 4. stage-2 SVD of `sqrt(n*prior/(ng-1)) * scale(group.means, xbar) %*%
//!    scaling1`; `scaling = scaling1 %*% V2[, 1:rank2]`, `svd = d2[1:rank2]`;
//! 5. scores per the oracle capture: globally centered features `%*% scaling`,
//!    each score column then re-centered by its own column mean.
//!
//! Discriminant signs are canonicalized like PCA: the largest-absolute-value
//! entry of each scaling column (first maximum) is made positive, and the
//! matching score column flips with it. The >= 3 group minimum replicates the
//! legacy `validate_lda_groups` default, not `MASS` itself.

use archaeodash_domain::DomainError;
use faer::Mat;

use crate::{apply_sign_flips, component_sign_flips, sum, ColumnMatrix};

/// Legacy `MASS::lda.default` tolerance.
pub const LEGACY_LDA_TOL: f64 = 1e-4;

/// `MASS::lda` moment-method parity result.
#[derive(Debug, Clone, PartialEq)]
pub struct Lda {
    /// Factor levels in sorted order; `prior`, `counts`, and `means` rows
    /// follow this order.
    pub levels: Vec<String>,
    /// Level proportions `counts / n`.
    pub prior: Vec<f64>,
    pub counts: Vec<usize>,
    /// Group means, level-major rows (`means[g][j]`).
    pub means: Vec<Vec<f64>>,
    /// `scaling` variable-major rows (`scaling[v][k]` for LD1..LDk),
    /// sign-canonicalized.
    pub scaling: Vec<Vec<f64>>,
    /// Stage-2 singular values actually kept (`svd[1:rank2]`).
    pub svd: Vec<f64>,
    /// Discriminant names `LD1..LDk`.
    pub score_names: Vec<String>,
    /// Scores row-major, each column re-centered to mean zero.
    pub scores: Vec<Vec<f64>>,
    /// Non-fatal R warnings (collinearity downgrades the rank).
    pub warnings: Vec<String>,
}

fn svd_error(e: faer::linalg::svd::SvdError) -> DomainError {
    DomainError::Internal(format!("singular value decomposition failed: {e:?}").into())
}

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

/// Runs lda-parity discriminant analysis. `groups[i]` labels row `i` of `m`;
/// every label must be non-empty. `min_groups` replicates the legacy
/// `validate_lda_groups` gate (3 in the app).
pub fn lda(m: &ColumnMatrix, groups: &[String], min_groups: usize) -> Result<Lda, DomainError> {
    let n = m.n_rows();
    let p = m.cols.len();
    if n == 0 || p == 0 || groups.len() != n {
        return Err(validation(
            "ordination_empty",
            "LDA requires one group label per row and at least one column",
        ));
    }
    for (j, name) in m.names.iter().enumerate() {
        if m.cols[j].iter().any(|v| !v.is_finite()) {
            return Err(validation(
                "ordination_missing_values",
                format!("LDA requires complete cases: column {name:?} has NA values"),
            ));
        }
    }
    for (i, g) in groups.iter().enumerate() {
        if g.is_empty() {
            return Err(validation(
                "lda_missing_group",
                format!(
                    "LDA requires complete group labels: row {} has an empty label",
                    i + 1
                ),
            ));
        }
    }
    // R factor levels sort as strings; duplicates collapse with counts.
    let mut levels = groups.to_vec();
    levels.sort();
    levels.dedup();
    if levels.len() < 2 {
        return Err(validation(
            "lda_group_levels",
            "grouping factor must have at least 2 levels",
        ));
    }
    if levels.len() < min_groups {
        return Err(validation(
            "lda_min_groups",
            format!(
                "LDA requires at least {min_groups} groups. Current selection has {}. \
                 Please include more groups and rerun.",
                levels.len()
            ),
        ));
    }
    let ng = levels.len();
    let mut counts = vec![0usize; ng];
    let group_index: Vec<usize> = groups
        .iter()
        .map(|g| match levels.binary_search(g) {
            Ok(i) => i,
            // Levels come from sorted unique group labels.
            Err(_) => unreachable!("levels are sorted and unique"),
        })
        .collect();
    for &gi in &group_index {
        counts[gi] += 1;
    }
    let prior: Vec<f64> = counts.iter().map(|&c| c as f64 / n as f64).collect();

    let mut group_rows: Vec<Vec<usize>> = vec![Vec::new(); ng];
    for (r, &gi) in group_index.iter().enumerate() {
        group_rows[gi].push(r);
    }
    // Group means (R tapply mean per level/column, compensated accumulation).
    let mut means = vec![vec![0.0f64; p]; ng];
    for (gi, rows) in group_rows.iter().enumerate() {
        for (j, col) in m.cols.iter().enumerate() {
            means[gi][j] = sum(rows.iter().map(|&r| col[r])) / counts[gi] as f64;
        }
    }

    // Within-group sd per column: sqrt(sum((x - gm)^2) / (n - 1)). The
    // centered matrix has exactly zero column means analytically, so R's
    // `var` re-centering is a no-op up to floating point.
    let mut f1 = Vec::with_capacity(p);
    for (j, col) in m.cols.iter().enumerate() {
        let ss = sum(group_index.iter().enumerate().map(|(i, &g)| {
            let centered = col[i] - means[g][j];
            centered * centered
        }));
        f1.push((ss / (n as f64 - 1.0)).sqrt());
    }
    let constant: Vec<usize> = f1
        .iter()
        .enumerate()
        .filter(|(_, &v)| v < LEGACY_LDA_TOL)
        .map(|(i, _)| i + 1)
        .collect();
    if !constant.is_empty() {
        let listed = constant
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let verb = if constant.len() == 1 {
            "variable"
        } else {
            "variables"
        };
        let ending = if constant.len() == 1 { "s" } else { "" };
        return Err(validation(
            "lda_constant_within_groups",
            format!("{verb} {listed} appear{ending} to be constant within groups"),
        ));
    }

    // Stage 1: SVD of sqrt(1/(n-ng)) * (x - means[g,]) %*% diag(1/f1).
    let factor1 = (1.0 / (n as f64 - ng as f64)).sqrt();
    let a1 = Mat::from_fn(n, p, |i, j| {
        factor1 * (m.cols[j][i] - means[group_index[i]][j]) / f1[j]
    });
    let s1 = a1.thin_svd().map_err(svd_error)?;
    let s1_col = s1.S().column_vector();
    let k1 = s1_col.nrows();
    let d1: Vec<f64> = (0..k1).map(|i| *s1_col.get(i)).collect();
    let rank1 = d1.iter().filter(|&&v| v > LEGACY_LDA_TOL).count();
    if rank1 == 0 {
        return Err(validation(
            "lda_rank_zero",
            "rank = 0: variables are numerically constant",
        ));
    }
    let mut warnings = Vec::new();
    if rank1 < p {
        warnings.push("variables are collinear".to_string());
    }
    // scaling1 (p x rank1, column-major) = diag(1/f1) %*% V1[, 1:rank1] %*% diag(1/d1[1:rank1]).
    let scaling1 = Mat::from_fn(p, rank1, |a, b| *s1.V().get(a, b) / (f1[a] * d1[b]));

    // Stage 2: SVD of sqrt(n*prior/(ng-1)) * scale(means, xbar) %*% scaling1.
    let xbar: Vec<f64> = (0..p)
        .map(|j| sum((0..ng).map(|g| prior[g] * means[g][j])))
        .collect();
    let factor2 = |g: usize| (n as f64 * prior[g] / (ng as f64 - 1.0)).sqrt();
    let b_mat = Mat::from_fn(ng, rank1, |g, b| {
        factor2(g) * sum((0..p).map(|a| (means[g][a] - xbar[a]) * scaling1.get(a, b)))
    });
    let s2 = b_mat.thin_svd().map_err(svd_error)?;
    let s2_col = s2.S().column_vector();
    let k2 = s2_col.nrows();
    let d2: Vec<f64> = (0..k2).map(|i| *s2_col.get(i)).collect();
    let rank2 = d2.iter().filter(|&&v| v > LEGACY_LDA_TOL * d2[0]).count();
    if rank2 == 0 {
        return Err(validation(
            "lda_identical_means",
            "group means are numerically identical",
        ));
    }
    let svd_values: Vec<f64> = d2[..rank2].to_vec();

    // scaling (p x rank2, column-major) = scaling1 %*% V2[, 1:rank2].
    let mut scaling_cols: Vec<Vec<f64>> = (0..rank2)
        .map(|c| {
            (0..p)
                .map(|a| sum((0..rank1).map(|b| *scaling1.get(a, b) * *s2.V().get(b, c))))
                .collect()
        })
        .collect();
    let flips = component_sign_flips(&scaling_cols);
    apply_sign_flips(&mut scaling_cols, &flips);

    // Oracle score convention (`lda_capture` in the R oracle and legacy
    // `getLDA`): `scale(features, center = colMeans(features))` uses R`s
    // default `scale. = TRUE`, so each centered column is also divided by its
    // sample sd (n-1; zero sd becomes 1 exactly like `scale.default`), then
    // `%*% scaling`, then each score column is re-centered by its own mean.
    let col_means: Vec<f64> = m
        .cols
        .iter()
        .map(|col| sum(col.iter().copied()) / n as f64)
        .collect();
    let mut col_sds = Vec::with_capacity(p);
    for (j, col) in m.cols.iter().enumerate() {
        let ss = sum(col.iter().map(|&v| (v - col_means[j]) * (v - col_means[j])));
        let sd = (ss / (n as f64 - 1.0)).sqrt();
        col_sds.push(if sd.is_finite() && sd != 0.0 { sd } else { 1.0 });
    }
    let centered = Mat::from_fn(n, p, |i, j| (m.cols[j][i] - col_means[j]) / col_sds[j]);
    let scaling_mat = Mat::from_fn(p, rank2, |a, c| scaling_cols[c][a]);
    let raw = &centered * &scaling_mat;
    // Scores inherit the canonical scaling signs: they are computed from the
    // already-flipped scaling matrix, so no second flip is applied here.
    let score_cols: Vec<Vec<f64>> = (0..rank2)
        .map(|c| {
            let column: Vec<f64> = (0..n).map(|i| *raw.get(i, c)).collect();
            let mean = sum(column.iter().copied()) / n as f64;
            column.into_iter().map(|v| v - mean).collect()
        })
        .collect();

    let scaling = (0..p)
        .map(|v| (0..rank2).map(|c| scaling_cols[c][v]).collect())
        .collect();
    let scores = (0..n)
        .map(|i| (0..rank2).map(|c| score_cols[c][i]).collect())
        .collect();
    let score_names = (1..=rank2).map(|k| format!("LD{k}")).collect();

    Ok(Lda {
        levels,
        prior,
        counts,
        means,
        scaling,
        svd: svd_values,
        score_names,
        scores,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_matrix() -> ColumnMatrix {
        ColumnMatrix {
            names: vec!["c1".into(), "c2".into(), "c3".into()],
            // Column-major: c1=[1,2,4,3,5,6], c2=[10,14,9,16,18,13],
            // c3=[2,5,1,3,6,4]; groups a,a,b,b,c,c.
            cols: vec![
                vec![1.0, 2.0, 4.0, 3.0, 5.0, 6.0],
                vec![10.0, 14.0, 9.0, 16.0, 18.0, 13.0],
                vec![2.0, 5.0, 1.0, 3.0, 6.0, 4.0],
            ],
        }
    }

    fn reference_groups() -> Vec<String> {
        ["a", "a", "b", "b", "c", "c"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// R 4.5.2 MASS:::lda.default(y, g) oracle for the reference matrix,
    /// captured at 17 significant digits.
    #[test]
    fn lda_matches_r_reference() {
        let result = lda(&reference_matrix(), &reference_groups(), 3).unwrap();
        let third = 1.0 / 3.0;
        assert_eq!(result.levels, vec!["a", "b", "c"]);
        assert_eq!(result.counts, vec![2, 2, 2]);
        for p in &result.prior {
            assert!((p - third).abs() < 1e-12);
        }
        // Level-major means from the column-major R printout.
        let r_means = [[1.5, 12.0, 3.5], [3.5, 12.5, 2.0], [5.5, 15.5, 5.0]];
        for (g, row) in r_means.iter().enumerate() {
            for (j, expected) in row.iter().enumerate() {
                assert!(
                    (result.means[g][j] - expected).abs() < 1e-12,
                    "means[{g}][{j}]: {} vs {expected}",
                    result.means[g][j]
                );
            }
        }
        // R scaling columns (already sign-canonical: max |entry| positive).
        let r_ld1 = [
            3.737_721_903_175_393_7,
            1.642_453_457_371_838_6,
            -3.248_477_887_053_868,
        ];
        let r_ld2 = [
            0.171_284_692_377_223_99,
            -0.460_575_130_502_011_82,
            1.333_996_478_907_869_3,
        ];
        let expected_cols = [&r_ld1, &r_ld2];
        for (c, col_expected) in expected_cols.iter().enumerate() {
            for (v, expected) in col_expected.iter().enumerate() {
                assert!(
                    (result.scaling[v][c] - expected).abs() < 1e-9,
                    "scaling[{v}][{c}]: {} vs {expected}",
                    result.scaling[v][c]
                );
            }
        }
        let r_svd = [11.985_833_954_250_142, 2.121_269_530_527_540_9];
        for (v, expected) in result.svd.iter().zip(r_svd.iter()) {
            assert!((v - expected).abs() < 1e-9, "svd {v} vs {expected}");
        }
        let r_scores_ld1 = [
            -3.979_470_001_547_849,
            -5.283_560_153_359_891_5,
            3.273_811_939_093_213_2,
            1.140_688_609_433_469_6,
            0.880_911_423_568_032_65,
            3.967_618_182_813_024_1,
        ];
        let r_scores_ld2 = [
            -0.852_793_970_171_296_21,
            0.843_109_089_196_285_69,
            -1.157_477_075_408_441_8,
            -0.758_840_511_132_628_07,
            1.296_020_964_346_291_7,
            0.629_981_503_169_788_74,
        ];
        for i in 0..6 {
            assert!(
                (result.scores[i][0] - r_scores_ld1[i]).abs() < 1e-9,
                "score LD1 row {i}: {} vs {}",
                result.scores[i][0],
                r_scores_ld1[i]
            );
            assert!(
                (result.scores[i][1] - r_scores_ld2[i]).abs() < 1e-9,
                "score LD2 row {i}: {} vs {}",
                result.scores[i][1],
                r_scores_ld2[i]
            );
        }
        assert_eq!(result.score_names, vec!["LD1", "LD2"]);
        assert!(result.warnings.is_empty());
        // Oracle score convention: every score column re-centered to zero mean.
        for c in 0..2 {
            let mean: f64 = result.scores.iter().map(|r| r[c]).sum::<f64>() / 6.0;
            assert!(mean.abs() < 1e-12);
        }
    }

    #[test]
    fn lda_rejects_invalid_inputs() {
        // Fewer than three groups: the legacy validate_lda_groups message.
        let two_groups: Vec<String> = ["a", "a", "b", "b", "a", "a"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let err = lda(&reference_matrix(), &two_groups, 3).unwrap_err();
        assert!(
            matches!(err, DomainError::Validation { ref code, .. } if code == "lda_min_groups")
        );
        assert!(err.to_string().contains("LDA requires at least 3 groups"));

        // Constant-within-group column: the MASS error, 1-based index.
        let mut constant = reference_matrix();
        constant.cols[0] = vec![1.0; 6];
        let err = lda(&constant, &reference_groups(), 3).unwrap_err();
        assert!(
            matches!(err, DomainError::Validation { ref code, .. } if code == "lda_constant_within_groups")
        );
        assert!(err
            .to_string()
            .contains("variable 1 appears to be constant within groups"));

        // Missing group labels and length mismatches.
        let err = lda(&reference_matrix(), &[], 3).unwrap_err();
        assert!(matches!(err, DomainError::Validation { code, .. } if code == "ordination_empty"));
        let mut blank = reference_groups();
        blank[2] = String::new();
        let err = lda(&reference_matrix(), &blank, 3).unwrap_err();
        assert!(matches!(err, DomainError::Validation { code, .. } if code == "lda_missing_group"));

        // NA in the matrix is rejected (MASS: "infinite, NA or NaN values").
        let mut with_na = reference_matrix();
        with_na.cols[1][3] = f64::NAN;
        let err = lda(&with_na, &reference_groups(), 3).unwrap_err();
        assert!(
            matches!(err, DomainError::Validation { code, .. } if code == "ordination_missing_values")
        );
    }
}
