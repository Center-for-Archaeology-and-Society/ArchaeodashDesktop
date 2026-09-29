//! Two-mean squared Mahalanobis distance from the legacy
//! `calculate_mahalanobis_distance` utility.
#![allow(clippy::needless_range_loop)] // LU pivot/elimination indices must stay synchronized.

use archaeodash_domain::DomainError;

/// Computes
/// `1 / (m - 1) * (f0*x_bar - y_bar)' * (f0^2*Sx + Sy)^-1 * (f0*x_bar-y_bar)`.
///
/// Covariance matrices use row-major nested vectors. The solve follows base
/// R's partial-pivoting `solve()` behavior and default reciprocal-condition
/// tolerance; singular or computationally singular matrices return a
/// validation error.
pub fn calculate_mahalanobis_distance(
    x_bar: &[f64],
    y_bar: &[f64],
    f0: f64,
    s_x: &[Vec<f64>],
    s_y: &[Vec<f64>],
    m: usize,
) -> Result<f64, DomainError> {
    let p = x_bar.len();
    if p == 0 || y_bar.len() != p || s_x.len() != p || s_y.len() != p {
        return Err(DomainError::validation(
            "mahalanobis_mean_dimensions",
            "mean vectors and covariance matrices must have the same nonzero dimension",
        ));
    }
    if m <= 1 {
        return Err(DomainError::validation(
            "mahalanobis_mean_sample_size",
            "m must be greater than one",
        ));
    }
    if !f0.is_finite()
        || x_bar.iter().chain(y_bar).any(|v| !v.is_finite())
        || s_x
            .iter()
            .chain(s_y)
            .any(|row| row.len() != p || row.iter().any(|v| !v.is_finite()))
    {
        return Err(DomainError::validation(
            "mahalanobis_mean_non_finite",
            "means, covariance matrices, and f0 must be finite and square",
        ));
    }

    let mut covariance = vec![vec![0.0; p]; p];
    let mut difference = vec![0.0; p];
    for i in 0..p {
        difference[i] = f0 * x_bar[i] - y_bar[i];
        for j in 0..p {
            covariance[i][j] = f0 * f0 * s_x[i][j] + s_y[i][j];
        }
    }
    if difference.iter().any(|v| !v.is_finite())
        || covariance.iter().flatten().any(|v| !v.is_finite())
    {
        return Err(DomainError::validation(
            "mahalanobis_mean_non_finite",
            "derived difference or combined covariance is not finite",
        ));
    }
    let inverse = inverse(&covariance).ok_or_else(|| {
        DomainError::validation(
            "mahalanobis_mean_singular",
            "combined covariance matrix is singular or computationally singular",
        )
    })?;
    let quadratic = (0..p)
        .map(|i| {
            (0..p)
                .map(|j| difference[i] * inverse[i][j] * difference[j])
                .sum::<f64>()
        })
        .sum::<f64>();
    let result = quadratic / (m as f64 - 1.0);
    if !result.is_finite() {
        return Err(DomainError::validation(
            "mahalanobis_mean_non_finite",
            "distance calculation produced a non-finite value",
        ));
    }
    Ok(result)
}

/// Base R `solve.default` style LU inverse with partial pivoting and
/// reciprocal-condition check. Kept local so this utility does not alter the
/// separate membership fallback's tolerance or regularization ladder.
fn inverse(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut lu = a.to_vec();
    let mut permutation: Vec<usize> = (0..n).collect();
    for k in 0..n {
        let mut pivot = k;
        let mut best = lu[k][k].abs();
        for i in k + 1..n {
            if lu[i][k].abs() > best {
                best = lu[i][k].abs();
                pivot = i;
            }
        }
        if lu[pivot][k] == 0.0 {
            return None;
        }
        if pivot != k {
            lu.swap(pivot, k);
            permutation.swap(pivot, k);
        }
        for i in k + 1..n {
            lu[i][k] /= lu[k][k];
            for j in k + 1..n {
                lu[i][j] -= lu[i][k] * lu[k][j];
            }
        }
    }
    let mut inverse = vec![vec![0.0; n]; n];
    for c in 0..n {
        let mut rhs: Vec<f64> = (0..n)
            .map(|i| if permutation[i] == c { 1.0 } else { 0.0 })
            .collect();
        for i in 0..n {
            for j in 0..i {
                rhs[i] -= lu[i][j] * rhs[j];
            }
        }
        for i in (0..n).rev() {
            for j in i + 1..n {
                rhs[i] -= lu[i][j] * rhs[j];
            }
            rhs[i] /= lu[i][i];
        }
        for i in 0..n {
            inverse[i][c] = rhs[i];
        }
    }
    let norm1 = |matrix: &[Vec<f64>]| {
        (0..n)
            .map(|j| (0..n).map(|i| matrix[i][j].abs()).sum::<f64>())
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let rcond = 1.0 / (norm1(a) * norm1(&inverse));
    if rcond.is_finite() && rcond < 1.0e-7 {
        return None;
    }
    Some(inverse)
}

#[cfg(test)]
mod tests {
    use super::calculate_mahalanobis_distance;

    #[test]
    fn matches_legacy_r_formula_fixture() {
        let x_bar = [1.0, 2.0];
        let y_bar = [2.0, 3.0];
        let s_x = vec![vec![1.0, 0.5], vec![0.5, 1.0]];
        let s_y = vec![vec![1.0, 0.3], vec![0.3, 1.0]];
        let actual =
            calculate_mahalanobis_distance(&x_bar, &y_bar, 1.0, &s_x, &s_y, 10).expect("distance");
        assert!((actual - 0.079_365_079_365_079_347).abs() < 1e-14);
    }

    #[test]
    fn reports_dimension_sample_and_singularity_errors() {
        let identity = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        assert!(
            calculate_mahalanobis_distance(&[1.0], &[1.0, 2.0], 1.0, &identity, &identity, 4)
                .is_err()
        );
        assert!(calculate_mahalanobis_distance(
            &[1.0],
            &[2.0],
            1.0,
            &[vec![1.0]],
            &[vec![-1.0]],
            4
        )
        .is_err());
        assert!(
            calculate_mahalanobis_distance(&[1.0], &[2.0], 1.0, &[vec![1.0]], &[vec![1.0]], 1)
                .is_err()
        );
    }
}
