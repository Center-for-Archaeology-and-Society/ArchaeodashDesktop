//! Group membership probabilities and Euclidean nearest matches (golden
//! parity procedures 10 and 11, classes T and E; IMPLEMENTATION.md
//! Section 15.4), ported from `R/Group_probs.R` (`getEligible`,
//! `group.mem.probs`, `getBestGroup`, `getMahalanobis`) and
//! `R/EuclideanDistance.R` (`calcEDistance`).
//!
//! Semantics replicated exactly:
//!
//! * `getEligible`: `dplyr::group_by |> count |> filter(n > max(nc, ng) + 1)`
//!   with dplyr's sorted character group keys (byte order).
//! * `group.mem.probs` Hotellings path: `ICSNP::HotellingsT2.default` per
//!   (row, eligible group) pair with the single row as `X` and the group's
//!   remaining rows as `Y` — pooled covariance
//!   `(cross(X.diff) + cross(Y.diff)) / (n1 + n2 - 2)`,
//!   `T2 = n1*n2/(n1+n2) * d' S^-1 d * (n1+n2-p-1)/(p*(n1+n2-2))`,
//!   `p.value = 1 - pf(T2, p, n1+n2-p-1)`, cell = `round(p, 5) * 100`. Any
//!   NA cell (`na.fail`) or `solve` failure falls back to the whole-table
//!   Mahalanobis path, exactly like the R `tryCatch` wrappers.
//! * Mahalanobis path (`getMahalanobis`): finite-column filter on the row,
//!   `complete.cases` rows (NaN dropped, `Inf` kept), zero-variance column
//!   drop (`var(na.rm = TRUE) > 0`), sample covariance, `solve(cov,
//!   tol = 1e-8)` with the `diag(1e-8)` ridge retry, `Inf` when both solves
//!   fail; non-finite cells become `Inf` before best-group selection.
//! * `getBestGroup`: first `which.max` (Hotellings) / `which.min`
//!   (Mahalanobis) over non-NA cells per row.
//! * `calcEDistance`: full pairwise Euclidean matrix over the chem columns,
//!   self-pairs dropped, matches restricted to projection-group rowids,
//!   stable per-observation distance sort with `slice_head(limit)` taken
//!   before the `withinGroup = FALSE` cross-group filter, then a final
//!   stable `arrange(observation, distance)` (dplyr C-locale byte order).

use std::cmp::Ordering;

use archaeodash_data_io::rnum::r_round;
use archaeodash_domain::DomainError;

use crate::{sum, ColumnMatrix};

/// R `.Machine$double.eps`, the default `solve` tolerance.
const R_DOUBLE_EPS: f64 = 2.220_446_049_250_313e-16;

/// `getEligible`: groups with more than `max(nchem, ngroups) + 1` rows, in
/// dplyr's sorted group-key order. Group labels are complete character
/// values (the loader rejects blank group columns).
pub fn get_eligible(groups: &[String], chem_count: usize) -> Vec<String> {
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for g in groups {
        *counts.entry(g.as_str()).or_insert(0) += 1;
    }
    let ng = counts.len();
    let m = chem_count.max(ng);
    counts
        .into_iter()
        .filter(|(_, count)| *count > m + 1)
        .map(|(group, _)| group.to_string())
        .collect()
}

/// `method = "Hotellings"` or `"Mahalanobis"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipMethod {
    Hotellings,
    Mahalanobis,
}

/// One row of the `group.mem.probs` result table (in data order).
#[derive(Debug, Clone, PartialEq)]
pub struct MembershipRow {
    /// `ID` column (`as.character(data[[ID]])`).
    pub id: String,
    /// `GroupVal` (`as.character(data[[group]])`).
    pub group_val: String,
    /// Probability cells aligned with [`MembershipTable::eligible`]; `NaN`
    /// is the R `NA` of the Hotellings path, `Inf` survives only on the
    /// Mahalanobis path (the golden serialiser maps both to `null`).
    pub probs: Vec<f64>,
    /// `BestGroup` (`None` when the whole row is `NA`).
    pub best_group: Option<String>,
    /// `BestValue`.
    pub best_value: Option<f64>,
    /// `InGroup` (`None` when `BestGroup` is `NA`).
    pub in_group: Option<bool>,
}

/// `group.mem.probs` result: the eligibility vector used and one row per
/// input row, in data order.
#[derive(Debug, Clone, PartialEq)]
pub struct MembershipTable {
    pub eligible: Vec<String>,
    /// The grouping column label (`Group` column of the R table).
    pub group: String,
    pub rows: Vec<MembershipRow>,
}

/// `group.mem.probs`: per-row membership probabilities against every
/// eligible group.
///
/// `ids`, `groups`, and `chem` are positionally aligned (one entry per data
/// row); `chem_select` is the requested `chem` name vector, intersected with
/// the matrix columns (or replaced by every `PC*` column when the matrix has
/// a `PC1` column, replicating the legacy principal-components branch).
pub fn group_mem_probs(
    ids: &[String],
    groups: &[String],
    group: &str,
    chem: &ColumnMatrix,
    chem_select: &[String],
    eligible: &[String],
    method: MembershipMethod,
) -> Result<MembershipTable, DomainError> {
    let n = chem.n_rows();
    if ids.len() != n || groups.len() != n {
        return Err(DomainError::validation(
            "membership_length_mismatch",
            "ids, groups, and chem rows must be aligned",
        ));
    }
    // eligible <- eligible[!is.na(eligible) & nzchar(eligible)]; unique()
    let mut unique: Vec<String> = Vec::new();
    for e in eligible {
        if !e.is_empty() && !unique.contains(e) {
            unique.push(e.clone());
        }
    }
    let eligible = unique;
    if eligible.is_empty() {
        return Err(DomainError::validation(
            "membership_no_eligible_groups",
            "No eligible groups are available for membership probabilities.",
        ));
    }
    // chem <- intersect(chem, names(data)); PC1 switches to every PC* column.
    let chem_names: Vec<String> = if chem.names.iter().any(|name| name == "PC1") {
        chem.names
            .iter()
            .filter(|name| name.contains("PC"))
            .cloned()
            .collect()
    } else {
        chem_select
            .iter()
            .filter(|c| chem.names.contains(c))
            .cloned()
            .collect()
    };
    if chem_names.is_empty() {
        return Err(DomainError::validation(
            "membership_no_chem_columns",
            "No valid analysis columns are available for membership probabilities.",
        ));
    }
    let chem_idx: Vec<usize> = chem_names
        .iter()
        .filter_map(|name| chem.names.iter().position(|n| n == name))
        .collect();
    let cells: Vec<Vec<f64>> = (0..n)
        .map(|i| chem_idx.iter().map(|&j| chem.cols[j][i]).collect())
        .collect();

    let probs = match method {
        MembershipMethod::Hotellings => {
            // The R tryCatch: any per-pair error (na.fail on NA cells,
            // singular pooled covariance) retries the whole table via the
            // Mahalanobis path.
            hotellings_table(&cells, groups, &eligible).unwrap_or_else(|| {
                mahalanobis_cells(&cells, groups, &eligible)
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|v| if v.is_finite() { v } else { f64::INFINITY })
                            .collect()
                    })
                    .collect()
            })
        }
        MembershipMethod::Mahalanobis => mahalanobis_cells(&cells, groups, &eligible)
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|v| if v.is_finite() { v } else { f64::INFINITY })
                    .collect()
            })
            .collect(),
    };

    // probsAll[!is.finite(probsAll)] <- Inf for Mahalanobis was applied above;
    // getBestGroup picks first which.max / which.min over non-NA cells.
    let hotellings = method == MembershipMethod::Hotellings;
    let rows = (0..n)
        .map(|i| {
            let row = &probs[i];
            let valid: Vec<usize> = (0..eligible.len()).filter(|&j| !row[j].is_nan()).collect();
            let (best_group, best_value, in_group) = if valid.is_empty() {
                (None, None, None)
            } else {
                let mut pick = valid[0];
                for &j in &valid[1..] {
                    let better = if hotellings {
                        row[j] > row[pick]
                    } else {
                        row[j] < row[pick]
                    };
                    if better {
                        pick = j;
                    }
                }
                let name = eligible[pick].clone();
                let value = row[pick];
                let in_group = if groups[i] == name {
                    Some(true)
                } else {
                    Some(false)
                };
                (Some(name), Some(value), in_group)
            };
            MembershipRow {
                id: ids[i].clone(),
                group_val: groups[i].clone(),
                probs: row.clone(),
                best_group,
                best_value,
                in_group,
            }
        })
        .collect();

    Ok(MembershipTable {
        eligible,
        group: group.to_string(),
        rows,
    })
}

/// Hotellings pass over every (row, group) pair; `None` replicates the R
/// error path that triggers the whole-table Mahalanobis fallback.
fn hotellings_table(
    cells: &[Vec<f64>],
    groups: &[String],
    eligible: &[String],
) -> Option<Vec<Vec<f64>>> {
    let n = cells.len();
    // na.fail on any pair's X or Y fails whenever any cell is NA.
    if cells.iter().any(|row| row.iter().any(|v| v.is_nan())) {
        return None;
    }
    let p = cells.first().map_or(0, Vec::len);
    let mut out = vec![vec![f64::NAN; eligible.len()]; n];
    for r in 0..n {
        for (gi, grp) in eligible.iter().enumerate() {
            // grpindx <- setdiff(which(data[[group]] == grp), r)
            let grp_rows: Vec<&[f64]> = (0..n)
                .filter(|&i| i != r && &groups[i] == grp)
                .map(|i| cells[i].as_slice())
                .collect();
            let p_value = match hotellings_p_value(&cells[r], &grp_rows, p) {
                Ok(v) => v,
                Err(()) => return None,
            };
            out[r][gi] = if p_value.is_finite() {
                p_value
            } else {
                f64::NAN
            };
        }
    }
    Some(out)
}

/// `ICSNP::HotellingsT2.default(X, Y)$p.value %>% round(., 5) * 100` for the
/// one-row `X` versus the group rows `Y` (two-sample `test = "f"` path).
/// `Err(())` mirrors an R error (singular `solve`) reaching the fallback.
fn hotellings_p_value(x_row: &[f64], y_rows: &[&[f64]], p: usize) -> Result<f64, ()> {
    let n1 = 1.0_f64;
    let n2 = y_rows.len() as f64;
    let pf_df1 = p as f64;
    let pf_df2 = n1 + n2 - pf_df1 - 1.0;
    let denom = n1 + n2 - 2.0;
    let ymean: Vec<f64> = (0..p)
        .map(|j| sum(y_rows.iter().map(|r| r[j])) / n2)
        .collect();
    // S.pooled = (t(X.diff) %*% X.diff + t(Y.diff) %*% Y.diff) / (n1 + n2 - 2).
    // X is a single row, so X.diff is exactly zero and contributes nothing.
    let mut s = vec![vec![0.0_f64; p]; p];
    for j in 0..p {
        for k in 0..p {
            s[j][k] = y_rows
                .iter()
                .map(|row| (row[j] - ymean[j]) * (row[k] - ymean[k]))
                .sum::<f64>()
                / denom;
        }
    }
    // X.diff is exactly zero (single-row X vs its own mean), so
    // t(X.diff) %*% X.diff contributes nothing to S.pooled.
    let d: Vec<f64> = (0..p).map(|j| x_row[j] - ymean[j]).collect();
    let s_inv = r_solve_inverse(&s, R_DOUBLE_EPS).ok_or(())?;
    let q: f64 = sum((0..p).map(|j| sum((0..p).map(|k| d[k] * s_inv[k][j])) * d[j]));
    let base = n1 * n2 / (n1 + n2);
    let t2 = base * q * (n1 + n2 - pf_df1 - 1.0) / (pf_df1 * (n1 + n2 - 2.0));
    let p_value = 1.0 - pf(t2, pf_df1, pf_df2);
    Ok(r_round(p_value, 5) * 100.0)
}

/// R `pf(q, df1, df2)` (central F CDF) via
/// `pbeta(df1*q/(df1*q+df2), df1/2, df2/2)`.
fn pf(q: f64, df1: f64, df2: f64) -> f64 {
    if q.is_nan() || df1.is_nan() || df2.is_nan() {
        return f64::NAN;
    }
    if q <= 0.0 {
        return 0.0;
    }
    if df1 <= 0.0 || df2 <= 0.0 {
        return f64::NAN;
    }
    let x = df1 * q / (df1 * q + df2);
    pbeta_lower(x, df1 / 2.0, df2 / 2.0)
}

/// Natural log gamma (Numerical Recipes `gammln`, g = 7 Lanczos), adequate
/// for the positive half-integer shape parameters of the F CDF.
fn lgamma(x: f64) -> f64 {
    const COF: [f64; 6] = [
        76.180_091_729_471_46,
        -86.505_320_329_416_77,
        24.014_098_240_830_91,
        -1.231_739_572_450_155,
        0.120_865_097_386_617_9e-2,
        -0.539_523_938_495_3e-5,
    ];
    let mut y = x;
    let mut tmp = x + 5.5;
    tmp -= (x + 0.5) * tmp.ln();
    let mut ser = 1.000_000_000_190_015;
    for &coefficient in COF.iter() {
        y += 1.0;
        ser += coefficient / y;
    }
    -tmp + (2.506_628_274_631_000_5 * ser / x).ln()
}

/// Continued fraction for the regularized incomplete beta (NR `betacf`).
fn betacf(a: f64, b: f64, x: f64) -> f64 {
    const MAXIT: usize = 200;
    const EPS: f64 = f64::EPSILON;
    const FPMIN: f64 = 1.0e-300;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=MAXIT {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    h
}

/// `pbeta(x, a, b)` lower tail (`I_x(a, b)`, NR `betai` with the same
/// continued-fraction side selection as R's TOMS 708 `bratio`).
fn pbeta_lower(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ln_bt = (lgamma(a + b) - lgamma(a) - lgamma(b)) + a * x.ln() + b * (1.0 - x).ln();
    let front = ln_bt.exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * betacf(a, b, x) / a
    } else {
        1.0 - front * betacf(b, a, 1.0 - x) / b
    }
}

/// `getMahalanobis(row, data)`: the filtered, complete-cased, zero-variance
/// dropped Mahalanobis distance of `row` against `data` rows, with the
/// `solve(tol = 1e-8)` -> `+ diag(1e-8)` -> `Inf` fallback ladder.
fn mahalanobis_distance(row: &[f64], data: &[&[f64]]) -> f64 {
    if data.len() < 2 || row.is_empty() {
        return f64::INFINITY;
    }
    // keep_cols <- is.finite(row_vec)
    let keep: Vec<usize> = (0..row.len()).filter(|&j| row[j].is_finite()).collect();
    if keep.is_empty() {
        return f64::INFINITY;
    }
    // complete.cases over the kept columns (NaN dropped; Inf is not NA).
    let cases: Vec<&[f64]> = data
        .iter()
        .filter(|r| keep.iter().all(|&j| !r[j].is_nan()))
        .copied()
        .collect();
    if cases.len() < 2 {
        return f64::INFINITY;
    }
    // variable_cols: at least two finite values and var(na.rm = TRUE) > 0.
    let mut variable: Vec<bool> = Vec::with_capacity(keep.len());
    for &j in &keep {
        let finite: Vec<f64> = cases
            .iter()
            .map(|r| r[j])
            .filter(|v| v.is_finite())
            .collect();
        if finite.len() < 2 {
            variable.push(false);
            continue;
        }
        let mean = sum(finite.iter().copied()) / finite.len() as f64;
        let var = sum(finite.iter().map(|v| (v - mean) * (v - mean))) / (finite.len() as f64 - 1.0);
        variable.push(var > 0.0);
    }
    if !variable.iter().any(|&v| v) {
        return f64::INFINITY;
    }
    let row_vec: Vec<f64> = keep
        .iter()
        .zip(&variable)
        .filter(|&(_, &v)| v)
        .map(|(&j, _)| row[j])
        .collect();
    let mat: Vec<Vec<f64>> = cases
        .iter()
        .map(|r| {
            keep.iter()
                .zip(&variable)
                .filter(|&(_, &v)| v)
                .map(|(&j, _)| r[j])
                .collect()
        })
        .collect();
    let p = row_vec.len();
    let n = mat.len() as f64;
    let means: Vec<f64> = (0..p).map(|j| sum(mat.iter().map(|r| r[j])) / n).collect();
    let mut cov = vec![vec![0.0_f64; p]; p];
    for j in 0..p {
        for k in 0..p {
            cov[j][k] = sum(mat.iter().map(|r| (r[j] - means[j]) * (r[k] - means[k]))) / (n - 1.0);
        }
    }
    if cov.iter().any(|c| c.iter().any(|v| !v.is_finite())) {
        return f64::INFINITY;
    }
    let d: Vec<f64> = (0..p).map(|j| row_vec[j] - means[j]).collect();
    let quadratic =
        |s_inv: &[Vec<f64>]| sum((0..p).map(|j| sum((0..p).map(|k| d[k] * s_inv[k][j])) * d[j]));
    let value = match r_solve_inverse(&cov, 1e-8) {
        Some(s_inv) => quadratic(&s_inv),
        None => {
            // cov + diag(1e-8) retry, else Inf.
            let mut reg = cov.clone();
            for (j, row) in reg.iter_mut().enumerate() {
                row[j] += 1e-8;
            }
            match r_solve_inverse(&reg, 1e-8) {
                Some(s_inv) => quadratic(&s_inv),
                None => return f64::INFINITY,
            }
        }
    };
    if value.is_finite() {
        value
    } else {
        f64::INFINITY
    }
}

fn mahalanobis_cells(cells: &[Vec<f64>], groups: &[String], eligible: &[String]) -> Vec<Vec<f64>> {
    let n = cells.len();
    let mut out = vec![vec![f64::NAN; eligible.len()]; n];
    for r in 0..n {
        for (gi, grp) in eligible.iter().enumerate() {
            let grp_rows: Vec<&[f64]> = (0..n)
                .filter(|&i| i != r && &groups[i] == grp)
                .map(|i| cells[i].as_slice())
                .collect();
            out[r][gi] = mahalanobis_distance(&cells[r], &grp_rows);
        }
    }
    out
}

/// R `solve(a, tol)`: LU with partial pivoting (LAPACK `dgesv`); errors when
/// a pivot is exactly zero ("exactly singular") or when the reciprocal
/// condition number `1 / (norm1(a) * norm1(a^-1))` drops below `tol`
/// ("computationally singular"). `NaN` input propagates instead of failing,
/// matching R's `NaN < tol == FALSE` behaviour.
#[allow(clippy::needless_range_loop)] // LU fill mirrors the R/LAPACK index algebra
fn r_solve_inverse(a: &[Vec<f64>], tol: f64) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut lu: Vec<Vec<f64>> = a.to_vec();
    let mut perm: Vec<usize> = (0..n).collect();
    for k in 0..n {
        let mut pivot = k;
        let mut best = lu[k][k].abs();
        for i in k + 1..n {
            let mag = lu[i][k].abs();
            if mag > best {
                best = mag;
                pivot = i;
            }
        }
        if lu[pivot][k] == 0.0 {
            return None;
        }
        if pivot != k {
            lu.swap(pivot, k);
            perm.swap(pivot, k);
        }
        for i in k + 1..n {
            lu[i][k] /= lu[k][k];
            for j in k + 1..n {
                lu[i][j] -= lu[i][k] * lu[k][j];
            }
        }
    }
    let mut inverse = vec![vec![0.0_f64; n]; n];
    for c in 0..n {
        let mut rhs: Vec<f64> = (0..n)
            .map(|i| if perm[i] == c { 1.0 } else { 0.0 })
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
    let norm1 = |m: &[Vec<f64>]| {
        (0..n)
            .map(|j| (0..n).map(|i| m[i][j].abs()).sum::<f64>())
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let rcond = 1.0 / (norm1(a) * norm1(&inverse));
    if rcond.is_finite() && rcond < tol {
        return None;
    }
    Some(inverse)
}

/// One `calcEDistance` result row: `rowid`, the observation and match ID
/// values, the Euclidean distance, and the two group labels.
#[derive(Debug, Clone, PartialEq)]
pub struct EuclideanMatch {
    pub rowid: String,
    pub id: String,
    pub match_id: String,
    pub distance: f64,
    pub group: String,
    pub match_group: String,
}

/// `calcEDistance`: nearest matches by Euclidean distance over `chem`.
///
/// `projection` lists the group labels whose rows may appear as matches;
/// `limit` is the per-observation `slice_head` cap applied before the
/// `withinGroup = FALSE` cross-group filter.
pub fn calc_e_distance(
    rowids: &[String],
    ids: &[String],
    groups: &[String],
    chem: &ColumnMatrix,
    projection: &[String],
    limit: usize,
    within_group: bool,
) -> Result<Vec<EuclideanMatch>, DomainError> {
    let n = chem.n_rows();
    if rowids.len() != n || ids.len() != n || groups.len() != n {
        return Err(DomainError::validation(
            "euclidean_length_mismatch",
            "rowids, ids, groups, and chem rows must be aligned",
        ));
    }
    if chem.cols.is_empty() {
        return Err(DomainError::validation(
            "euclidean_no_chem_columns",
            "No numeric analysis columns found for this dataset source.",
        ));
    }
    let p = chem.cols.len();
    // stats::dist euclidean: sequential squared-difference accumulation; NA
    // cells propagate as NaN distances.
    #[allow(clippy::needless_range_loop)]
    let dist = {
        let mut dist = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for j in i + 1..n {
                let mut acc = 0.0_f64;
                for k in 0..p {
                    let diff = chem.cols[k][i] - chem.cols[k][j];
                    acc += diff * diff;
                }
                let value = acc.sqrt();
                dist[i][j] = value;
                dist[j][i] = value;
            }
        }
        dist
    };
    let projection: std::collections::HashSet<&str> =
        projection.iter().map(String::as_str).collect();
    let in_projection: Vec<bool> = groups
        .iter()
        .map(|g| projection.contains(g.as_str()))
        .collect();
    // as.data.frame(as.table(as.matrix(d))): column-major expansion with the
    // observation (Var1) varying slowest, then the two filters.
    let mut candidates: Vec<(usize, usize, f64)> = Vec::new();
    for i in 0..n {
        for j in 0..n {
            if rowids[i] == rowids[j] || !in_projection[j] {
                continue;
            }
            candidates.push((i, j, dist[i][j]));
        }
    }
    // group_by(observation_rowid) (dplyr sorts character keys), stable
    // arrange(distance), slice_head(n = limit).
    let mut keys: Vec<&str> = candidates
        .iter()
        .map(|(i, _, _)| rowids[*i].as_str())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let mut selected: Vec<(usize, usize, f64)> = Vec::new();
    for key in keys {
        let mut rows: Vec<&(usize, usize, f64)> = candidates
            .iter()
            .filter(|(i, _, _)| rowids[*i].as_str() == key)
            .collect();
        rows.sort_by(|a, b| cmp_distance(a.2, b.2));
        selected.extend(rows.into_iter().take(limit));
    }
    let mut out: Vec<EuclideanMatch> = selected
        .into_iter()
        .map(|(i, j, distance)| EuclideanMatch {
            rowid: rowids[i].clone(),
            id: ids[i].clone(),
            match_id: ids[j].clone(),
            distance,
            group: groups[i].clone(),
            match_group: groups[j].clone(),
        })
        .collect();
    // Final stable arrange(observation, distance); NaN sorts last like R's
    // radix order.
    out.sort_by(|a, b| {
        a.id.cmp(&b.id)
            .then_with(|| cmp_distance(a.distance, b.distance))
    });
    if !within_group {
        out.retain(|m| m.group != m.match_group);
    }
    Ok(out)
}

/// Total order over distances with NaN last (R's `order` NA handling).
fn cmp_distance(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eligible_groups_use_sorted_count_threshold() {
        // m = max(4 chem, 5 groups) = 5, threshold n > 6; sorted byte order.
        // 5 rows per group: m = max(4 chem, 5 groups) = 5, threshold n > 6.
        let groups: Vec<String> = ["D1", "D2", "D4", "D3a", "D3b"]
            .iter()
            .cycle()
            .take(25)
            .map(|s| s.to_string())
            .collect();
        assert_eq!(get_eligible(&groups, 4), Vec::<String>::new());
        let mut groups: Vec<String> = Vec::new();
        for (g, count) in [
            ("D1", 104),
            ("D2", 50),
            ("D4", 63),
            ("D3a", 50),
            ("D3b", 40),
        ] {
            for _ in 0..count {
                groups.push(g.to_string());
            }
        }
        assert_eq!(
            get_eligible(&groups, 4),
            vec!["D1", "D2", "D3a", "D3b", "D4"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn pf_matches_r_reference_values() {
        // R 4.6.1: pf(c(0, 0.5, 2.718281828459045, 15.25, 100), 4, 46)
        for (q, expected) in [
            (0.0, 0.0),
            (0.5, 0.264_185_595_807_845_45),
            (2.718_281_828_459_045, 0.959_004_679_457_155_62),
            (15.25, 0.999_999_947_856_952_34),
            (100.0, 1.0),
        ] {
            assert!((pf(q, 4.0, 46.0) - expected).abs() < 1e-12, "pf({q})");
        }
        assert!(pf(1.0, 4.0, 0.0).is_nan());
    }

    #[test]
    fn hotellings_matches_r_probe() {
        // R probe: X = (1,2,3,4); Y rows as below; T2 = 0.17824074073988058,
        // p.value = 0.92306399794184646, round(5)*100 = 92.306.
        let x = [1.0, 2.0, 3.0, 4.0];
        let y: Vec<Vec<f64>> = vec![
            vec![1.1, 2.2, 2.9, 4.5],
            vec![2.0, 3.0, 4.0, 6.0],
            vec![0.5, 1.5, 2.5, 3.5],
            vec![1.2, 2.1, 3.3, 4.1],
            vec![5.0, 6.0, 7.0, 9.0],
        ];
        let refs: Vec<&[f64]> = y.iter().map(|r| r.as_slice()).collect();
        let value = hotellings_p_value(&x, &refs, 4).expect("solvable");
        assert!((value - 92.306).abs() < 1e-9);
    }

    #[test]
    fn mahalanobis_matches_r_probe() {
        // R probe: mahalanobis(matrix(c(1,2,3,4), 1), colMeans(Y), cov(Y),
        // tol = 1e-8) = 3.4222222222162371 for the same Y.
        let y: Vec<Vec<f64>> = vec![
            vec![1.1, 2.2, 2.9, 4.5],
            vec![2.0, 3.0, 4.0, 6.0],
            vec![0.5, 1.5, 2.5, 3.5],
            vec![1.2, 2.1, 3.3, 4.1],
            vec![5.0, 6.0, 7.0, 9.0],
        ];
        let refs: Vec<&[f64]> = y.iter().map(|r| r.as_slice()).collect();
        let d = mahalanobis_distance(&[1.0, 2.0, 3.0, 4.0], &refs);
        // 1e-9 absolute slack: R's cov accumulates in long double, this port
        // in f64 with compensated sums; the golden fixture compares at 5e-5.
        assert!((d - 3.422_222_222_216_237_1).abs() < 1e-9, "{d}");
    }

    #[test]
    fn mahalanobis_fallback_ladder() {
        // Fewer than two complete rows -> Inf; singular covariance reaches the
        // ridge retry in getMahalanobis via hotellings-style error paths.
        let refs: Vec<&[f64]> = vec![&[1.0, 2.0]];
        assert_eq!(mahalanobis_distance(&[1.0, 2.0], &refs), f64::INFINITY);
        // Zero-variance columns all dropped -> Inf.
        let refs: Vec<&[f64]> = vec![&[1.0, 1.0], &[2.0, 2.0], &[3.0, 3.0]];
        assert_eq!(mahalanobis_distance(&[1.0, 1.0], &refs), f64::INFINITY);
    }
}
