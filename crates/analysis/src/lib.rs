//! Transformations with golden parity semantics (IMPLEMENTATION.md
//! Section 8.4): `zScore` (compositional percent + column z-score, 3-decimal
//! rounding), `log10`/`log` (rounding + non-finite-to-zero with warning
//! counts), and ratio application (null when the denominator is null or zero).

pub mod cluster;
pub mod explore;
pub mod lda;
pub mod membership;
pub mod pca;
pub mod umap;

use archaeodash_data_io::rnum::r_round;
use archaeodash_domain::DomainError;

pub use cluster::{
    cluster_diagnostics, diana, hclust_ward_d2, kmeans, pam, silhouette_mean, ClusterDiagnostics,
    Diana, Hclust, Kmeans, Pam,
};
pub use explore::{
    compositional_profile, crosstab_count, crosstab_value_summary, histogram, missing_profile,
    r_pretty, CrosstabCountRow, CrosstabMethod, CrosstabValueRow, MissingBand, MissingRow,
    ProfileRow,
};
pub use lda::{lda, Lda, LEGACY_LDA_TOL};
pub use membership::{
    calc_e_distance, get_eligible, group_mem_probs, EuclideanMatch, MembershipMethod,
    MembershipRow, MembershipTable,
};
pub use pca::{apply_sign_flips, component_sign_flips, pca, Pca};
pub use umap::{
    find_ab_params, fuzzy_simplicial_set, knn_brute_force, laplacian_smallest_eigenvalues,
    smooth_knn_dist, umap, Umap, UmapConfig, DEFAULT_SEED,
};

/// 3-decimal R-compatible rounding used by the z-score and log contracts.
pub(crate) fn r_round3(v: f64) -> f64 {
    r_round(v, 3)
}

/// An ordered column matrix with `f64::NAN` marking missing values, matching
/// the R numeric frame the oracle feeds to every transform.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnMatrix {
    pub names: Vec<String>,
    /// Column-major values; `NaN` is NA.
    pub cols: Vec<Vec<f64>>,
}

impl ColumnMatrix {
    pub fn n_rows(&self) -> usize {
        self.cols.first().map(|c| c.len()).unwrap_or(0)
    }

    pub fn column(&self, name: &str) -> Option<&Vec<f64>> {
        self.names
            .iter()
            .position(|n| n == name)
            .map(|i| &self.cols[i])
    }
}

/// Neumaier compensated sum, approximating R's long-double accumulation.
pub(crate) fn sum(values: impl Iterator<Item = f64>) -> f64 {
    let mut sum = 0.0f64;
    let mut c = 0.0f64;
    for v in values {
        let t = sum + v;
        if sum.abs() >= v.abs() {
            c += (sum - t) + v;
        } else {
            c += (v - t) + sum;
        }
        sum = t;
    }
    sum + c
}

/// Legacy `zScore`: row proportions of the row sum, times 100, then column
/// standardization (center = mean, scale = sample sd, n-1) and rounding to
/// three decimals (Section 8.4: compositional percent + column z-score).
pub fn z_score(m: &ColumnMatrix) -> Result<ColumnMatrix, DomainError> {
    let n = m.n_rows();
    if n == 0 {
        return Err(DomainError::validation("transform", "empty matrix"));
    }
    // Row proportions * 100 with NA propagation.
    let mut props: Vec<Vec<f64>> = vec![vec![f64::NAN; m.cols.len()]; n];
    for row in 0..n {
        let row_sum = sum(m.cols.iter().map(|col| col[row]));
        for (j, col) in m.cols.iter().enumerate() {
            props[row][j] = if row_sum.is_finite() && row_sum != 0.0 && col[row].is_finite() {
                col[row] / row_sum * 100.0
            } else {
                f64::NAN
            };
        }
    }
    // scale(): center by colMeans(na.rm = TRUE), then sd over the non-NA
    // centered values with denominator max(1, n_finite - 1) — verified against
    // the R 4.6.1 oracle (scale.default uses na.rm for both mean and sd).
    let mut out_cols: Vec<Vec<f64>> = Vec::with_capacity(m.cols.len());
    for (j, _col) in m.cols.iter().enumerate() {
        let finite: Vec<f64> = props
            .iter()
            .map(|r| r[j])
            .filter(|v| v.is_finite())
            .collect();
        if finite.is_empty() {
            out_cols.push(vec![f64::NAN; n]);
            continue;
        }
        let mean = sum(finite.iter().copied()) / finite.len() as f64;
        let sq = sum(finite.iter().map(|v| (v - mean) * (v - mean)));
        let denom = (finite.len() as f64 - 1.0).max(1.0);
        let sd = (sq / denom).sqrt();
        let out_col: Vec<f64> = (0..n)
            .map(|i| {
                let v = props[i][j];
                if v.is_nan() {
                    f64::NAN
                } else {
                    r_round3((v - mean) / sd)
                }
            })
            .collect();
        out_cols.push(out_col);
    }
    Ok(ColumnMatrix {
        names: m.names.clone(),
        cols: out_cols,
    })
}

/// `log10`/`log` transforms: round to three decimals, convert non-finite
/// results to zero, and report the non-finite count (Section 8.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogBase {
    Natural,
    Base10,
}

pub struct LogResult {
    pub matrix: ColumnMatrix,
    /// Number of cells whose transformed value was non-finite before zeroing.
    pub non_finite_to_zero: u64,
}

pub fn log_transform(m: &ColumnMatrix, base: LogBase) -> Result<LogResult, DomainError> {
    let n = m.n_rows();
    let mut non_finite = 0u64;
    let mut out_cols = Vec::with_capacity(m.cols.len());
    for col in &m.cols {
        let mut out = Vec::with_capacity(n);
        for &v in col {
            let raw = match base {
                LogBase::Base10 => v.log10(),
                LogBase::Natural => v.ln(),
            };
            if !raw.is_finite() {
                non_finite += 1;
            }
            out.push(if raw.is_finite() { r_round3(raw) } else { 0.0 });
        }
        out_cols.push(out);
    }
    Ok(LogResult {
        matrix: ColumnMatrix {
            names: m.names.clone(),
            cols: out_cols,
        },
        non_finite_to_zero: non_finite,
    })
}

/// One ratio definition (Section 8.2).
#[derive(Debug, Clone, PartialEq)]
pub struct RatioSpec {
    pub output_name: String,
    pub numerator: String,
    pub denominator: String,
}

/// Applies ratio specs in order, appending `numerator / denominator` columns;
/// the result is null when the denominator is null or zero (Section 8.2).
/// A NaN numerator propagates to null, matching R's `ifelse` NA output.
pub fn apply_ratios(m: &ColumnMatrix, specs: &[RatioSpec]) -> Result<ColumnMatrix, DomainError> {
    let mut names = m.names.clone();
    let mut cols = m.cols.clone();
    for spec in specs {
        if names.contains(&spec.output_name) {
            return Err(DomainError::validation(
                "ratio_duplicate_name",
                format!("ratio output name {} already exists", spec.output_name),
            ));
        }
        let num = m.column(&spec.numerator).ok_or_else(|| {
            DomainError::validation("ratio_missing_column", spec.numerator.clone())
        })?;
        let den = m.column(&spec.denominator).ok_or_else(|| {
            DomainError::validation("ratio_missing_column", spec.denominator.clone())
        })?;
        let out: Vec<f64> = num
            .iter()
            .zip(den.iter())
            .map(|(&n, &d)| {
                if d == 0.0 || d.is_nan() || n.is_nan() {
                    f64::NAN
                } else {
                    n / d
                }
            })
            .collect();
        names.push(spec.output_name.clone());
        cols.push(out);
    }
    Ok(ColumnMatrix { names, cols })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(names: &[&str], cols: Vec<Vec<f64>>) -> ColumnMatrix {
        ColumnMatrix {
            names: names.iter().map(|s| s.to_string()).collect(),
            cols,
        }
    }

    #[test]
    fn zscore_matches_r_probe() {
        // Oracle: scale(prop.table(as.matrix(x), 1) * 100) for
        // x = matrix(c(1,2,3,4,5,6), nrow = 3) gives col1
        // -1.0806342671903613 0.18793639429397577 0.89269787289638491;
        // the legacy zScore contract then rounds to 3 decimals (Section 8.4).
        let m = matrix(&["a", "b"], vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
        let z = z_score(&m).unwrap();
        assert!((z.cols[0][0] - (-1.081)).abs() < 1e-12);
        assert!((z.cols[0][1] - 0.188).abs() < 1e-12);
        assert!((z.cols[0][2] - 0.893).abs() < 1e-12);
        log10_probe();
    }

    #[test]
    fn zscore_handles_na_like_r_scale() {
        // Oracle (R 4.6.1): zScore on matrix a=[1,2,NA,4], b=[4,5,6,8].
        // prop.table runs first (row proportions * 100), then scale() with
        // colMeans/colVars na.rm = TRUE, then round(3). NA rows propagate.
        let m = matrix(
            &["a", "b"],
            vec![vec![1.0, 2.0, f64::NAN, 4.0], vec![4.0, 5.0, 6.0, 8.0]],
        );
        let z = z_score(&m).unwrap();
        assert!((z.cols[0][0] - (-1.081)).abs() < 1e-12);
        assert!((z.cols[0][1] - 0.188).abs() < 1e-12);
        assert!(z.cols[0][2].is_nan());
        assert!((z.cols[0][3] - 0.893).abs() < 1e-12);
        assert!((z.cols[1][0] - 1.081).abs() < 1e-12);
    }

    #[test]
    fn zscore_zero_row_sum_is_nan() {
        // Oracle: a zero row sum makes prop.table produce NaN for that row.
        let m = matrix(
            &["a", "b"],
            vec![vec![0.0, 2.0, f64::NAN, 4.0], vec![0.0, 5.0, 6.0, 8.0]],
        );
        let z = z_score(&m).unwrap();
        assert!(z.cols[0][0].is_nan());
        assert!((z.cols[0][1] - (-0.707)).abs() < 1e-12);
        assert!(z.cols[0][2].is_nan());
        assert!((z.cols[0][3] - 0.707).abs() < 1e-12);
    }

    fn log10_probe() {
        let m = matrix(&["a"], vec![vec![1.0, 100.0, 0.0, -2.0]]);
        let r = log_transform(&m, LogBase::Base10).unwrap();
        assert_eq!(r.non_finite_to_zero, 2);
        assert_eq!(r.matrix.cols[0][0], 0.0);
        assert_eq!(r.matrix.cols[0][1], 2.0);
        assert_eq!(r.matrix.cols[0][2], 0.0);
    }

    #[test]
    fn ratios_null_on_zero_or_null_denominator() {
        let m = matrix(&["as", "la"], vec![vec![3.784, 2.0], vec![0.0, 4.0]]);
        let specs = vec![RatioSpec {
            output_name: "as_la".into(),
            numerator: "as".into(),
            denominator: "la".into(),
        }];
        let out = apply_ratios(&m, &specs).unwrap();
        assert!(out.cols[2][0].is_nan()); // denominator 0 -> null
        assert!((out.cols[2][1] - 0.5).abs() < 1e-15);
        let dup = RatioSpec {
            output_name: "as".into(),
            ..specs[0].clone()
        };
        assert!(apply_ratios(&m, &[dup]).is_err());
    }
}
