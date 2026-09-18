//! PCA with exact `stats::prcomp` semantics (golden parity #6, class T;
//! IMPLEMENTATION.md Section 15.4): column-mean centering, optional
//! sample-sd scaling (the `scale.` flag), `sdev = d / sqrt(n - 1)`, rotation
//! from the right singular vectors of the centered matrix, and scores
//! `x_centered %*% rotation`.
//!
//! R leaves component signs arbitrary (LAPACK), so every component is
//! canonicalized: the largest-absolute-value entry of each rotation column
//! (first maximum on ties) is made positive, and the matching score column
//! flips with it. Parity tests canonicalize the golden values with the same
//! rule, so faer/LAPACK sign differences never affect comparisons.

use archaeodash_domain::DomainError;
use faer::Mat;

use crate::{sum, ColumnMatrix};

/// Returns one flip flag per component column: `true` when the column has
/// any nonzero entry and its largest-absolute-value entry (first maximum)
/// is negative; all-zero columns are never flipped.
pub fn component_sign_flips(columns: &[Vec<f64>]) -> Vec<bool> {
    columns
        .iter()
        .map(|col| {
            let mut max_index = 0usize;
            let mut max_abs = -1.0f64;
            for (i, &v) in col.iter().enumerate() {
                let magnitude = v.abs();
                if magnitude > max_abs {
                    max_abs = magnitude;
                    max_index = i;
                }
            }
            col.get(max_index).copied().unwrap_or(1.0) < 0.0
        })
        .collect()
}

/// Applies flip flags to column-major component columns in place.
pub fn apply_sign_flips(columns: &mut [Vec<f64>], flips: &[bool]) {
    for (col, &flip) in columns.iter_mut().zip(flips) {
        if flip {
            for v in col.iter_mut() {
                *v = -*v;
            }
        }
    }
}

fn svd_error(e: faer::linalg::svd::SvdError) -> DomainError {
    DomainError::Internal(format!("singular value decomposition failed: {e:?}").into())
}

/// prcomp parity result. `rotation` is variable-major (`rotation[v][k]`,
/// matching R's rotation matrix layout) and `scores` is row-major
/// (`scores[i][k]`, matching R's `pca$x` data frame); both are
/// sign-canonicalized.
#[derive(Debug, Clone, PartialEq)]
pub struct Pca {
    /// `sdev`: component standard deviations `d / sqrt(n - 1)`.
    pub sdev: Vec<f64>,
    /// `center`: column means subtracted before decomposition.
    pub center: Vec<f64>,
    /// `scale`: column sample sds (n-1 denominator) when scaling was
    /// requested, otherwise `None` (`scale. = FALSE`).
    pub scale: Option<Vec<f64>>,
    pub rotation: Vec<Vec<f64>>,
    pub scores: Vec<Vec<f64>>,
}

/// Runs prcomp-parity PCA on a complete (NaN-free) numeric matrix. NaN inputs
/// are rejected because R `prcomp` errors on missing values; the ordination
/// service requires complete cases upstream.
pub fn pca(m: &ColumnMatrix, scale: bool) -> Result<Pca, DomainError> {
    let n = m.n_rows();
    let p = m.cols.len();
    if n < 2 || p == 0 {
        return Err(DomainError::validation(
            "ordination_empty",
            "PCA requires at least two rows and one column",
        ));
    }
    for (j, name) in m.names.iter().enumerate() {
        if m.cols[j].iter().any(|v| !v.is_finite()) {
            return Err(DomainError::validation(
                "ordination_missing_values",
                format!("PCA requires complete cases: column {name:?} has NA values"),
            ));
        }
    }

    // Column means and optional sample-sd scaling (n-1 denominator, R
    // `scale` semantics).
    let mut center = Vec::with_capacity(p);
    for col in &m.cols {
        center.push(sum(col.iter().copied()) / n as f64);
    }
    let scale_values = if scale {
        let mut sds = Vec::with_capacity(p);
        for (j, col) in m.cols.iter().enumerate() {
            let ss = sum(col.iter().map(|&v| (v - center[j]) * (v - center[j])));
            let sd = (ss / (n as f64 - 1.0)).sqrt();
            if !sd.is_finite() || sd == 0.0 {
                return Err(DomainError::validation(
                    "ordination_zero_variance",
                    format!(
                        "column {:?} has zero variance and cannot be scaled",
                        m.names[j]
                    ),
                ));
            }
            sds.push(sd);
        }
        Some(sds)
    } else {
        None
    };

    // Centered (and optionally scaled) matrix; inputs are column-major.
    let a = Mat::from_fn(n, p, |i, j| {
        let centered = m.cols[j][i] - center[j];
        match &scale_values {
            Some(sds) => centered / sds[j],
            None => centered,
        }
    });
    let svd = a.thin_svd().map_err(svd_error)?;
    let s_col = svd.S().column_vector();
    let k = s_col.nrows();
    let d: Vec<f64> = (0..k).map(|i| *s_col.get(i)).collect();
    let sdev: Vec<f64> = d.iter().map(|&v| v / (n as f64 - 1.0).sqrt()).collect();

    // Rotation columns from V (p x k), then canonical signs.
    let mut rotation_cols: Vec<Vec<f64>> = (0..k)
        .map(|c| (0..p).map(|r| *svd.V().get(r, c)).collect())
        .collect();
    let flips = component_sign_flips(&rotation_cols);
    apply_sign_flips(&mut rotation_cols, &flips);

    // Scores = Xc * V, flipped with the matching rotation columns.
    let scores_mat = &a * svd.V();
    let mut score_cols: Vec<Vec<f64>> = (0..k)
        .map(|c| (0..n).map(|r| *scores_mat.get(r, c)).collect())
        .collect();
    apply_sign_flips(&mut score_cols, &flips);

    // Row-major outputs matching the R matrix layouts.
    let rotation = (0..p)
        .map(|v| (0..k).map(|c| rotation_cols[c][v]).collect())
        .collect();
    let scores = (0..n)
        .map(|i| (0..k).map(|c| score_cols[c][i]).collect())
        .collect();

    Ok(Pca {
        sdev,
        center,
        scale: scale_values,
        rotation,
        scores,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(cols: Vec<Vec<f64>>) -> ColumnMatrix {
        ColumnMatrix {
            names: (0..cols.len()).map(|i| format!("c{i}")).collect(),
            cols,
        }
    }

    /// R 4.5.2 oracle: prcomp(matrix(c(2,4,6,8, 1,5,3,7, 2,2,6,6), nrow=4)).
    #[test]
    fn pca_matches_r_reference() {
        let m = matrix(vec![
            vec![2.0, 4.0, 6.0, 8.0],
            vec![1.0, 5.0, 3.0, 7.0],
            vec![2.0, 2.0, 6.0, 6.0],
        ]);
        let pc = pca(&m, false).unwrap();
        let r_sdev = [3.911_033_274_383_225, 1.835_888_175_607_079_5];
        for (i, expected) in r_sdev.iter().enumerate() {
            assert!(
                (pc.sdev[i] - expected).abs() < 1e-9,
                "sdev[{}]: {} vs {expected}",
                i,
                pc.sdev[i]
            );
        }
        assert!(pc.sdev[2].abs() < 1e-12, "third component is degenerate");
        assert_eq!(pc.center, vec![5.0, 4.0, 4.0]);
        assert!(pc.scale.is_none());
        // R rotation columns, sign-canonicalized: PC1 already positive-max;
        // PC2 flipped (R's max-|entry| 0.7394 is negative); PC3 canonical.
        let r_pc1 = [
            0.657_513_171_213_281_5,
            0.561_583_292_368_328_9,
            0.502_295_366_705_489_1,
        ];
        let r_pc2_flipped = [
            -0.126_302_382_202_473_03,
            0.739_387_023_993_516_8,
            -0.661_327_858_932_308_4,
        ];
        let r_pc3 = [
            0.742_781_352_708_207_4,
            -0.371_390_676_354_103_83,
            -0.557_086_014_531_155_8,
        ];
        let expected_cols = [&r_pc1, &r_pc2_flipped, &r_pc3];
        for (c, col_expected) in expected_cols.iter().enumerate() {
            for (v, expected) in col_expected.iter().enumerate() {
                assert!(
                    (pc.rotation[v][c] - expected).abs() < 1e-9,
                    "rotation[{v}][{c}]: {} vs {expected}",
                    pc.rotation[v][c]
                );
            }
        }
        // Scores PC1/PC3 keep R signs; PC2 flipped with its rotation column.
        let r_scores_pc1 = [
            -4.661_880_124_155_809,
            -1.100_520_612_255_930_8,
            1.100_520_612_255_930_8,
            4.661_880_124_155_809,
        ];
        let r_scores_pc2_flipped = [
            -0.516_598_207_508_514_5,
            2.188_345_124_060_606_7,
            -2.188_345_124_060_606_7,
            0.516_598_207_508_514_5,
        ];
        for i in 0..4 {
            assert!((pc.scores[i][0] - r_scores_pc1[i]).abs() < 1e-9);
            assert!((pc.scores[i][1] - r_scores_pc2_flipped[i]).abs() < 1e-9);
            assert!(pc.scores[i][2].abs() < 1e-12);
        }
    }

    #[test]
    fn pca_rejects_missing_values_and_empty_input() {
        let m = matrix(vec![vec![1.0, f64::NAN], vec![2.0, 3.0]]);
        let err = pca(&m, false).unwrap_err();
        assert!(
            matches!(err, DomainError::Validation { code, .. } if code == "ordination_missing_values")
        );
        let empty = matrix(vec![]);
        let err = pca(&empty, false).unwrap_err();
        assert!(matches!(err, DomainError::Validation { code, .. } if code == "ordination_empty"));
    }

    #[test]
    fn sign_flip_helpers_are_deterministic() {
        let cols = vec![vec![-3.0, 1.0], vec![0.0, 0.0], vec![2.0, -1.0]];
        let flips = component_sign_flips(&cols);
        assert_eq!(flips, vec![true, false, false]);
        let mut cols = cols;
        apply_sign_flips(&mut cols, &flips);
        assert_eq!(cols[0], vec![3.0, -1.0]);
        assert_eq!(cols[1], vec![0.0, 0.0]);
        assert_eq!(cols[2], vec![2.0, -1.0]);
    }
}
