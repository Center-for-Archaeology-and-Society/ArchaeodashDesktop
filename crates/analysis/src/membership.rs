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

use crate::{sum, CancellationToken, ColumnMatrix};

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
    group_mem_probs_tracked(ids, groups, group, chem, chem_select, eligible, method)
        .map(|(table, _)| table)
}

/// [`group_mem_probs`] plus the method that was actually effective: the
/// legacy `tryCatch` falls back from Hotellings to the whole-table
/// Mahalanobis path on any per-pair failure, and callers surface which one
/// produced the table.
pub fn group_mem_probs_tracked(
    ids: &[String],
    groups: &[String],
    group: &str,
    chem: &ColumnMatrix,
    chem_select: &[String],
    eligible: &[String],
    method: MembershipMethod,
) -> Result<(MembershipTable, MembershipMethod), DomainError> {
    group_mem_probs_tracked_cancellable(
        ids,
        groups,
        group,
        chem,
        chem_select,
        eligible,
        method,
        &CancellationToken::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn group_mem_probs_tracked_cancellable(
    ids: &[String],
    groups: &[String],
    group: &str,
    chem: &ColumnMatrix,
    chem_select: &[String],
    eligible: &[String],
    method: MembershipMethod,
    cancel: &CancellationToken,
) -> Result<(MembershipTable, MembershipMethod), DomainError> {
    cancel.check()?;
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

    let (probs, effective) = match method {
        MembershipMethod::Hotellings => {
            // The R tryCatch: any per-pair error (na.fail on NA cells,
            // singular pooled covariance) retries the whole table via the
            // Mahalanobis path.
            match hotellings_table(&cells, groups, &eligible, cancel) {
                Some(table) => (table, MembershipMethod::Hotellings),
                None if cancel.is_cancelled() => {
                    return Err(DomainError::validation(
                        "analysis_cancelled",
                        "analysis cancelled",
                    ))
                }
                None => {
                    let table = mahalanobis_cells(&cells, groups, &eligible, cancel)?;
                    (non_finite_to_inf(table), MembershipMethod::Mahalanobis)
                }
            }
        }
        MembershipMethod::Mahalanobis => {
            let table = mahalanobis_cells(&cells, groups, &eligible, cancel)?;
            (non_finite_to_inf(table), MembershipMethod::Mahalanobis)
        }
    };

    // probsAll[!is.finite(probsAll)] <- Inf for Mahalanobis was applied above;
    // the legacy recursive fallback calls getBestGroup with method="Mahalanobis",
    // so selection must follow the effective method rather than the request.
    let hotellings = effective == MembershipMethod::Hotellings;
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
    cancel.check()?;

    Ok((
        MembershipTable {
            eligible,
            group: group.to_string(),
            rows,
        },
        effective,
    ))
}

/// Mahalanobis-path cell mapping: non-finite distances become `Inf`
/// (`probsAll[!is.finite(probsAll)] <- Inf`).
fn non_finite_to_inf(cells: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    cells
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|v| if v.is_finite() { v } else { f64::INFINITY })
                .collect()
        })
        .collect()
}

/// Rows per parallel block in the membership tables. A row outside a group
/// costs one quadratic form per group; a row inside a group also rebuilds its
/// leave-one-out covariance, so blocks stay small for load balance.
const MEMBERSHIP_ROW_BLOCK: usize = 16;

/// Row indices of each eligible group (ascending, so leave-one-out row sets
/// keep R's `which()` order) and each row's eligible-group index, if any.
fn group_members(groups: &[String], eligible: &[String]) -> (Vec<Vec<usize>>, Vec<Option<usize>>) {
    let index: std::collections::HashMap<&str, usize> = eligible
        .iter()
        .enumerate()
        .map(|(gi, grp)| (grp.as_str(), gi))
        .collect();
    let mut members = vec![Vec::new(); eligible.len()];
    let row_group: Vec<Option<usize>> = groups
        .iter()
        .enumerate()
        .map(|(i, grp)| {
            let gi = index.get(grp.as_str()).copied();
            if let Some(gi) = gi {
                members[gi].push(i);
            }
            gi
        })
        .collect();
    (members, row_group)
}

/// Every row of `members`, in order: what a row outside the group sees.
fn rows_of<'a>(cells: &'a [Vec<f64>], members: &[usize]) -> Vec<&'a [f64]> {
    members.iter().map(|&i| cells[i].as_slice()).collect()
}

/// The rows of `members` other than `skip`, in order: R's
/// `setdiff(which(data[[group]] == grp), r)`.
fn rows_except<'a>(cells: &'a [Vec<f64>], members: &[usize], skip: usize) -> Vec<&'a [f64]> {
    members
        .iter()
        .filter(|&&i| i != skip)
        .map(|&i| cells[i].as_slice())
        .collect()
}

/// Internal marker for a Hotellings pair that hit an R error; it stops the
/// parallel pass and becomes the whole-table Mahalanobis fallback.
fn hotellings_pair_failed() -> DomainError {
    DomainError::validation("membership_hotellings_failed", "Hotellings pair failed")
}

/// Hotellings pass over every (row, group) pair; `None` replicates the R
/// error path that triggers the whole-table Mahalanobis fallback (and is
/// also returned on cancellation, which the caller checks).
///
/// A row outside a group is tested against the whole group, so that group's
/// mean and inverse pooled covariance are computed once and shared; a row
/// inside a group gets its own leave-one-out statistics. Rows run in
/// parallel and each cell uses the same arithmetic as a direct
/// [`hotellings_p_value`] call, so results do not depend on scheduling.
fn hotellings_table(
    cells: &[Vec<f64>],
    groups: &[String],
    eligible: &[String],
    cancel: &CancellationToken,
) -> Option<Vec<Vec<f64>>> {
    let n = cells.len();
    // na.fail on any pair's X or Y fails whenever any cell is NA.
    if cells.iter().any(|row| row.iter().any(|v| v.is_nan())) {
        return None;
    }
    let p = cells.first().map_or(0, Vec::len);
    let (members, row_group) = group_members(groups, eligible);
    // A failed whole-group solve only matters once a row outside the group
    // uses it, exactly as in the per-pair R loop.
    let whole = crate::pairwise::par_row_blocks(eligible.len(), 1, |range| {
        cancel.check()?;
        let gi = range.start;
        Ok(hotellings_group(&rows_of(cells, &members[gi]), p))
    })
    .ok()?;
    let blocks = crate::pairwise::par_row_blocks(n, MEMBERSHIP_ROW_BLOCK, |rows| {
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            cancel.check()?;
            let mut row_cells = vec![f64::NAN; eligible.len()];
            for (gi, cell) in row_cells.iter_mut().enumerate() {
                let pair = if row_group[r] == Some(gi) {
                    hotellings_p_value(&cells[r], &rows_except(cells, &members[gi], r), p)
                } else {
                    whole[gi]
                        .as_ref()
                        .map(|group| hotellings_row(&cells[r], group, p))
                        .map_err(|_| ())
                };
                let p_value = pair.map_err(|()| hotellings_pair_failed())?;
                *cell = if p_value.is_finite() {
                    p_value
                } else {
                    f64::NAN
                };
            }
            out.push(row_cells);
        }
        Ok(out)
    })
    .ok()?;
    Some(blocks.into_iter().flatten().collect())
}

/// The group (`Y`) side of the Hotellings test: its size, mean, and inverse
/// pooled covariance.
struct HotellingsGroup {
    n2: f64,
    ymean: Vec<f64>,
    s_inv: Vec<Vec<f64>>,
}

/// Builds the [`HotellingsGroup`] for `y_rows`. `Err(())` mirrors an R
/// error (singular `solve`) reaching the fallback.
fn hotellings_group(y_rows: &[&[f64]], p: usize) -> Result<HotellingsGroup, ()> {
    let n1 = 1.0_f64;
    let n2 = y_rows.len() as f64;
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
    let s_inv = r_solve_inverse(&s, R_DOUBLE_EPS).ok_or(())?;
    Ok(HotellingsGroup { n2, ymean, s_inv })
}

/// The `X` side of the Hotellings test for one row against a prepared group.
fn hotellings_row(x_row: &[f64], group: &HotellingsGroup, p: usize) -> f64 {
    let n1 = 1.0_f64;
    let n2 = group.n2;
    let pf_df1 = p as f64;
    let pf_df2 = n1 + n2 - pf_df1 - 1.0;
    let d: Vec<f64> = (0..p).map(|j| x_row[j] - group.ymean[j]).collect();
    let s_inv = &group.s_inv;
    let q: f64 = sum((0..p).map(|j| sum((0..p).map(|k| d[k] * s_inv[k][j])) * d[j]));
    let base = n1 * n2 / (n1 + n2);
    let t2 = base * q * (n1 + n2 - pf_df1 - 1.0) / (pf_df1 * (n1 + n2 - 2.0));
    let p_value = 1.0 - pf(t2, pf_df1, pf_df2);
    r_round(p_value, 5) * 100.0
}

/// `ICSNP::HotellingsT2.default(X, Y)$p.value %>% round(., 5) * 100` for the
/// one-row `X` versus the group rows `Y` (two-sample `test = "f"` path).
/// `Err(())` mirrors an R error (singular `solve`) reaching the fallback.
fn hotellings_p_value(x_row: &[f64], y_rows: &[&[f64]], p: usize) -> Result<f64, ()> {
    Ok(hotellings_row(x_row, &hotellings_group(y_rows, p)?, p))
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
    mahalanobis_row(row, &mahalanobis_group(&finite_columns(row), data))
}

/// `keep_cols <- is.finite(row_vec)`.
fn finite_columns(row: &[f64]) -> Vec<usize> {
    (0..row.len()).filter(|&j| row[j].is_finite()).collect()
}

/// Everything in `getMahalanobis` that does not use the row's own values:
/// it depends only on the group rows and the row's finite columns, so rows
/// with the same group rows and finite-column pattern share it.
enum MahalanobisGroup {
    /// A fallback-ladder step that returns `Inf` for any such row.
    Infinite,
    Ready {
        /// Kept, variable columns (indices into the full row).
        cols: Vec<usize>,
        means: Vec<f64>,
        s_inv: Vec<Vec<f64>>,
    },
}

/// The group-side steps of [`mahalanobis_distance`] for the finite columns
/// `keep` (callers handle `data.len() < 2`).
fn mahalanobis_group(keep: &[usize], data: &[&[f64]]) -> MahalanobisGroup {
    use MahalanobisGroup::Infinite;
    if data.len() < 2 || keep.is_empty() {
        return Infinite;
    }
    // complete.cases over the kept columns (NaN dropped; Inf is not NA).
    let cases: Vec<&[f64]> = data
        .iter()
        .filter(|r| keep.iter().all(|&j| !r[j].is_nan()))
        .copied()
        .collect();
    if cases.len() < 2 {
        return Infinite;
    }
    // variable_cols: at least two finite values and var(na.rm = TRUE) > 0.
    let mut variable: Vec<bool> = Vec::with_capacity(keep.len());
    for &j in keep {
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
        return Infinite;
    }
    let cols: Vec<usize> = keep
        .iter()
        .zip(&variable)
        .filter(|&(_, &v)| v)
        .map(|(&j, _)| j)
        .collect();
    let mat: Vec<Vec<f64>> = cases
        .iter()
        .map(|r| cols.iter().map(|&j| r[j]).collect())
        .collect();
    let p = cols.len();
    let n = mat.len() as f64;
    let means: Vec<f64> = (0..p).map(|j| sum(mat.iter().map(|r| r[j])) / n).collect();
    let mut cov = vec![vec![0.0_f64; p]; p];
    for j in 0..p {
        for k in 0..p {
            cov[j][k] = sum(mat.iter().map(|r| (r[j] - means[j]) * (r[k] - means[k]))) / (n - 1.0);
        }
    }
    if cov.iter().any(|c| c.iter().any(|v| !v.is_finite())) {
        return Infinite;
    }
    let s_inv = match r_solve_inverse(&cov, 1e-8) {
        Some(s_inv) => s_inv,
        None => {
            // cov + diag(1e-8) retry, else Inf.
            let mut reg = cov;
            for (j, row) in reg.iter_mut().enumerate() {
                row[j] += 1e-8;
            }
            match r_solve_inverse(&reg, 1e-8) {
                Some(s_inv) => s_inv,
                None => return Infinite,
            }
        }
    };
    MahalanobisGroup::Ready { cols, means, s_inv }
}

/// The row-side step of [`mahalanobis_distance`]: the quadratic form of the
/// row's deviation from the prepared group mean.
fn mahalanobis_row(row: &[f64], group: &MahalanobisGroup) -> f64 {
    let MahalanobisGroup::Ready { cols, means, s_inv } = group else {
        return f64::INFINITY;
    };
    let p = cols.len();
    let d: Vec<f64> = (0..p).map(|j| row[cols[j]] - means[j]).collect();
    let value = sum((0..p).map(|j| sum((0..p).map(|k| d[k] * s_inv[k][j])) * d[j]));
    if value.is_finite() {
        value
    } else {
        f64::INFINITY
    }
}

/// `getMahalanobis` for every (row, eligible group) pair.
///
/// A row outside a group is measured against the whole group, so the group
/// side is prepared once per (group, finite-column pattern) and shared; a
/// row inside a group gets its own leave-one-out preparation. Rows run in
/// parallel and each cell uses the same arithmetic as a direct
/// [`mahalanobis_distance`] call, so results do not depend on scheduling.
fn mahalanobis_cells(
    cells: &[Vec<f64>],
    groups: &[String],
    eligible: &[String],
    cancel: &CancellationToken,
) -> Result<Vec<Vec<f64>>, DomainError> {
    let n = cells.len();
    let (members, row_group) = group_members(groups, eligible);
    // Distinct finite-column patterns (usually just one: every column).
    let mut pattern_index: std::collections::HashMap<Vec<usize>, usize> =
        std::collections::HashMap::new();
    let mut patterns: Vec<Vec<usize>> = Vec::new();
    let row_pattern: Vec<usize> = cells
        .iter()
        .map(|row| {
            let keep = finite_columns(row);
            *pattern_index.entry(keep.clone()).or_insert_with(|| {
                patterns.push(keep);
                patterns.len() - 1
            })
        })
        .collect();
    // Prepare only the (group, pattern) pairs some outside row uses, so the
    // work never exceeds the per-pair loop even when every row differs.
    let mut needed = vec![vec![false; patterns.len()]; eligible.len()];
    for r in 0..n {
        for (gi, flags) in needed.iter_mut().enumerate() {
            if row_group[r] != Some(gi) {
                flags[row_pattern[r]] = true;
            }
        }
    }
    let tasks: Vec<(usize, usize)> = needed
        .iter()
        .enumerate()
        .flat_map(|(gi, flags)| {
            flags
                .iter()
                .enumerate()
                .filter(|&(_, &used)| used)
                .map(move |(pi, _)| (gi, pi))
        })
        .collect();
    let prepared = crate::pairwise::par_row_blocks(tasks.len(), 1, |range| {
        cancel.check()?;
        let (gi, pi) = tasks[range.start];
        Ok(mahalanobis_group(
            &patterns[pi],
            &rows_of(cells, &members[gi]),
        ))
    })?;
    let mut whole: Vec<Vec<Option<MahalanobisGroup>>> = (0..eligible.len())
        .map(|_| (0..patterns.len()).map(|_| None).collect())
        .collect();
    for (&(gi, pi), group) in tasks.iter().zip(prepared) {
        whole[gi][pi] = Some(group);
    }
    let blocks = crate::pairwise::par_row_blocks(n, MEMBERSHIP_ROW_BLOCK, |rows| {
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            cancel.check()?;
            let row_cells: Vec<f64> = (0..eligible.len())
                .map(|gi| {
                    if row_group[r] == Some(gi) {
                        mahalanobis_distance(&cells[r], &rows_except(cells, &members[gi], r))
                    } else {
                        // Every outside row's pair was prepared above; the
                        // direct call is only a defensive equivalent. The
                        // data.len() < 2 check of getMahalanobis lives in
                        // mahalanobis_group, and rows are never empty here.
                        match whole[gi][row_pattern[r]].as_ref() {
                            Some(group) => mahalanobis_row(&cells[r], group),
                            None => mahalanobis_distance(&cells[r], &rows_of(cells, &members[gi])),
                        }
                    }
                })
                .collect();
            out.push(row_cells);
        }
        Ok(out)
    })?;
    Ok(blocks.into_iter().flatten().collect())
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
    /// Stable key of the matched input row, aligned with the rowid input.
    pub match_rowid: String,
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
    calc_e_distance_cancellable(
        rowids,
        ids,
        groups,
        chem,
        projection,
        limit,
        within_group,
        &CancellationToken::default(),
    )
}

/// Cancellation-aware, bounded-memory nearest-neighbour implementation. It
/// retains only the exact stable top-k candidates for each legacy rowid key.
#[allow(clippy::too_many_arguments)]
pub fn calc_e_distance_cancellable(
    rowids: &[String],
    ids: &[String],
    groups: &[String],
    chem: &ColumnMatrix,
    projection: &[String],
    limit: usize,
    within_group: bool,
    cancel: &CancellationToken,
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
    use crate::cluster::DistanceMetric;
    use crate::pairwise::{par_row_blocks, tile_distances, TILE_ROWS};

    #[derive(Clone)]
    struct Candidate {
        sequence: usize,
        i: usize,
        j: usize,
        distance: f64,
    }
    let compare = |a: &Candidate, b: &Candidate| {
        cmp_distance(a.distance, b.distance).then_with(|| a.sequence.cmp(&b.sequence))
    };
    let cap = limit;
    // Keeps `rows` sorted by (distance, sequence) and at most `cap` long.
    let offer = |rows: &mut Vec<Candidate>, candidate: Candidate| {
        let pos = rows
            .binary_search_by(|current| compare(current, &candidate))
            .unwrap_or_else(|p| p);
        if pos < cap {
            rows.insert(pos, candidate);
            if rows.len() > cap {
                rows.pop();
            }
        }
    };
    let projection: std::collections::HashSet<&str> =
        projection.iter().map(String::as_str).collect();
    let in_projection: Vec<bool> = groups
        .iter()
        .map(|g| projection.contains(g.as_str()))
        .collect();
    // Integer keys for the rowid self-exclusion test.
    let mut key_ids: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let row_keys: Vec<usize> = rowids
        .iter()
        .map(|r| {
            let next = key_ids.len();
            *key_ids.entry(r.as_str()).or_insert(next)
        })
        .collect();
    // The old full matrix/table order is i-major then j-major, so
    // `sequence = i * n + j` stays the stable tie-break. Each row keeps its own
    // top `limit`; merging those per rowid key gives the same top `limit` as
    // one pass over every pair.
    const ROW_BLOCK: usize = 16;
    let per_block = par_row_blocks(n, ROW_BLOCK, |rows| {
        let mut tile = vec![0.0f64; TILE_ROWS];
        let mut best: Vec<Vec<Candidate>> = rows.clone().map(|_| Vec::new()).collect();
        let mut j0 = 0;
        while j0 < n {
            cancel.check()?;
            let j1 = (j0 + TILE_ROWS).min(n);
            let tile = &mut tile[..j1 - j0];
            for (r, i) in rows.clone().enumerate() {
                tile_distances(&chem.cols, DistanceMetric::Euclidean, i, j0..j1, tile);
                for (jj, &distance) in tile.iter().enumerate() {
                    let j = j0 + jj;
                    if row_keys[i] == row_keys[j] || !in_projection[j] {
                        continue;
                    }
                    offer(
                        &mut best[r],
                        Candidate {
                            sequence: i.wrapping_mul(n).wrapping_add(j),
                            i,
                            j,
                            distance,
                        },
                    );
                }
            }
            j0 = j1;
        }
        Ok(best)
    })?;
    cancel.check()?;
    let mut candidates: std::collections::BTreeMap<String, Vec<Candidate>> =
        std::collections::BTreeMap::new();
    for (i, row_best) in per_block.into_iter().flatten().enumerate() {
        if row_best.is_empty() {
            continue;
        }
        let rows = candidates.entry(rowids[i].clone()).or_default();
        for candidate in row_best {
            offer(rows, candidate);
        }
    }
    let mut out: Vec<EuclideanMatch> = candidates
        .values()
        .flat_map(|rows| rows.iter())
        .map(|candidate| EuclideanMatch {
            rowid: rowids[candidate.i].clone(),
            match_rowid: rowids[candidate.j].clone(),
            id: ids[candidate.i].clone(),
            match_id: ids[candidate.j].clone(),
            distance: candidate.distance,
            group: groups[candidate.i].clone(),
            match_group: groups[candidate.j].clone(),
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
    fn euclidean_top_k_preserves_duplicate_rowid_grouping_and_post_limit_filter() {
        let rows = vec!["a", "a", "b", "c"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let ids = vec!["i0", "i1", "i2", "i3"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let groups = vec!["X", "Y", "X", "Y"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let projection = vec!["X".into(), "Y".into()];
        let chem = ColumnMatrix {
            names: vec!["v".into()],
            cols: vec![vec![0.0, 1.0, 2.0, 3.0]],
        };
        let all = calc_e_distance(&rows, &ids, &groups, &chem, &projection, 1, true).unwrap();
        assert_eq!(all.len(), 3); // one top result per legacy rowid key
        assert_eq!(
            all.iter().map(|m| m.rowid.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(all[0].id, "i1"); // duplicate rowid key chooses the nearest member
        assert_eq!(all[0].match_id, "i2");
        let cross = calc_e_distance(&rows, &ids, &groups, &chem, &projection, 1, false).unwrap();
        assert_eq!(cross.len(), 3); // filtering happens after each rowid's top-1 is chosen
        assert!(cross.iter().all(|m| m.group != m.match_group));
    }

    #[test]
    fn euclidean_cooperative_cancel_stops_before_pair_scan() {
        let token = CancellationToken::new();
        token.cancel();
        let rows = vec!["a".into(), "b".into()];
        let chem = ColumnMatrix {
            names: vec!["v".into()],
            cols: vec![vec![0.0, 1.0]],
        };
        let err = calc_e_distance_cancellable(&rows, &rows, &rows, &chem, &rows, 1, true, &token)
            .unwrap_err();
        assert!(err.to_string().contains("analysis_cancelled"));
    }

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

    /// The original per-(row, group) loop, kept as the reference the shared
    /// and parallel tables must match bit for bit.
    fn reference_rows<'a>(
        cells: &'a [Vec<f64>],
        groups: &[String],
        grp: &str,
        r: usize,
    ) -> Vec<&'a [f64]> {
        (0..cells.len())
            .filter(|&i| i != r && groups[i] == grp)
            .map(|i| cells[i].as_slice())
            .collect()
    }

    fn reference_hotellings(
        cells: &[Vec<f64>],
        groups: &[String],
        eligible: &[String],
    ) -> Option<Vec<Vec<f64>>> {
        if cells.iter().any(|row| row.iter().any(|v| v.is_nan())) {
            return None;
        }
        let p = cells[0].len();
        let mut out = vec![vec![f64::NAN; eligible.len()]; cells.len()];
        for r in 0..cells.len() {
            for (gi, grp) in eligible.iter().enumerate() {
                let v = hotellings_p_value(&cells[r], &reference_rows(cells, groups, grp, r), p)
                    .ok()?;
                out[r][gi] = if v.is_finite() { v } else { f64::NAN };
            }
        }
        Some(out)
    }

    fn reference_mahalanobis(
        cells: &[Vec<f64>],
        groups: &[String],
        eligible: &[String],
    ) -> Vec<Vec<f64>> {
        (0..cells.len())
            .map(|r| {
                eligible
                    .iter()
                    .map(|grp| {
                        mahalanobis_distance(&cells[r], &reference_rows(cells, groups, grp, r))
                    })
                    .collect()
            })
            .collect()
    }

    fn assert_same_bits(actual: &[Vec<f64>], expected: &[Vec<f64>]) {
        assert_eq!(actual.len(), expected.len());
        for (r, (a, e)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(a.len(), e.len());
            for (gi, (x, y)) in a.iter().zip(e).enumerate() {
                assert!(
                    x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()),
                    "row {r} group {gi}: {x} vs {y}"
                );
            }
        }
    }

    /// 150 rows (several parallel blocks) in four groups; D is not eligible.
    fn membership_fixture() -> (Vec<Vec<f64>>, Vec<String>, Vec<String>) {
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut cells = Vec::new();
        let mut groups = Vec::new();
        for (grp, count, shift) in [
            ("A", 60, 0.0),
            ("B", 50, 3.0),
            ("C", 30, -2.0),
            ("D", 10, 1.0),
        ] {
            for _ in 0..count {
                cells.push(
                    (0..4)
                        .map(|j| shift + j as f64 + next() * 2.0)
                        .collect::<Vec<f64>>(),
                );
                groups.push(grp.to_string());
            }
        }
        // Interleave the groups so members are not contiguous.
        let mut order: Vec<usize> = (0..cells.len()).collect();
        order.sort_by_key(|&i| (i * 37) % cells.len());
        let cells = order.iter().map(|&i| cells[i].clone()).collect();
        let groups = order.iter().map(|&i| groups[i].clone()).collect();
        let eligible = ["A", "B", "C"].iter().map(|s| s.to_string()).collect();
        (cells, groups, eligible)
    }

    #[test]
    fn shared_hotellings_table_matches_per_pair_loop_bit_for_bit() {
        let (cells, groups, eligible) = membership_fixture();
        let token = CancellationToken::default();
        let table = hotellings_table(&cells, &groups, &eligible, &token).expect("solvable");
        let expected = reference_hotellings(&cells, &groups, &eligible).expect("solvable");
        assert_same_bits(&table, &expected);

        // A constant column in group C makes its covariance singular: both
        // paths fail and fall back.
        let mut singular = cells.clone();
        for (row, grp) in singular.iter_mut().zip(&groups) {
            if grp == "C" {
                row[2] = 5.0;
            }
        }
        assert!(reference_hotellings(&singular, &groups, &eligible).is_none());
        assert!(hotellings_table(&singular, &groups, &eligible, &token).is_none());
    }

    #[test]
    fn shared_mahalanobis_table_matches_per_pair_loop_bit_for_bit() {
        let (mut cells, groups, eligible) = membership_fixture();
        // Missing and infinite values give several finite-column patterns;
        // a constant column in C exercises the zero-variance drop.
        cells[3][1] = f64::NAN;
        cells[17][0] = f64::NAN;
        cells[17][3] = f64::NAN;
        cells[40][2] = f64::INFINITY;
        cells[41] = vec![f64::NAN; 4];
        cells[90][1] = f64::NEG_INFINITY;
        for (row, grp) in cells.iter_mut().zip(&groups) {
            if grp == "C" {
                row[3] = 7.0;
            }
        }
        let token = CancellationToken::default();
        let table = mahalanobis_cells(&cells, &groups, &eligible, &token).expect("not cancelled");
        assert_same_bits(&table, &reference_mahalanobis(&cells, &groups, &eligible));
        assert!(table[41].iter().all(|v| *v == f64::INFINITY));
    }

    #[test]
    fn membership_tables_stop_on_cancellation() {
        let (cells, groups, eligible) = membership_fixture();
        let token = CancellationToken::new();
        token.cancel();
        let err = mahalanobis_cells(&cells, &groups, &eligible, &token).unwrap_err();
        assert!(err.to_string().contains("analysis_cancelled"));
        assert!(hotellings_table(&cells, &groups, &eligible, &token).is_none());
    }

    #[test]
    fn hotellings_fallback_selects_smallest_mahalanobis_distance() {
        let ids = (0..7).map(|i| format!("s{i}")).collect::<Vec<_>>();
        let groups = vec!["A", "A", "A", "B", "B", "B", "A"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let chem = ColumnMatrix {
            names: vec!["Ti".into()],
            cols: vec![vec![0.0, 2.0, f64::NAN, 10.0, 11.0, 12.0, 1.0]],
        };

        let (result, effective) = group_mem_probs_tracked(
            &ids,
            &groups,
            "Group",
            &chem,
            &["Ti".into()],
            &["A".into(), "B".into()],
            MembershipMethod::Hotellings,
        )
        .expect("membership result");

        assert_eq!(effective, MembershipMethod::Mahalanobis);
        // The final observation is closest to A (distance 0 vs. 100 to B).
        // Picking the maximum here would expose use of the originally requested
        // Hotellings method after the fallback had already switched methods.
        assert_eq!(result.rows[6].best_group.as_deref(), Some("A"));
        assert!(result.rows[6].best_value.unwrap().abs() < 1e-12);
    }
}
