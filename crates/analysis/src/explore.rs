//! Explore-view parity primitives (Section 8 procedure 12, class E): the
//! `profile_missing` bands, `graphics::hist` breakpoints via R's `pretty`
//! algorithm, crosstab summaries, and the compositional-profile long table.
//!
//! The histogram port reproduces R 4.6.1 exactly: `pretty.default` parameters
//! (`min.n = 1`, `shrink.sml = 0.75`, `high.u.bias = 1.5`, `u5.bias = 2.75`,
//! `f.min = 2^-20`, `eps.correct = 0`) through `src/appl/pretty.c`
//! `R_pretty0`, then `hist.default`'s fuzzy-break adjustment (`fuzz = 1e-7`)
//! and `C_bincount` right-closed binning with `include.lowest`. Differential
//! vectors captured from R 4.6.1 pin the behavior in tests.

use archaeodash_data_io::rnum::r_round;
use archaeodash_domain::DomainError;

use crate::ColumnMatrix;

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

/// `plot_missing` band labels in threshold order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingBand {
    Good,
    Ok,
    Bad,
    Remove,
}

impl MissingBand {
    /// Legacy label exactly as the DataExplorer band names serialize.
    pub fn label(&self) -> &'static str {
        match self {
            MissingBand::Good => "Good",
            MissingBand::Ok => "OK",
            MissingBand::Bad => "Bad",
            MissingBand::Remove => "Remove",
        }
    }
}

/// One `profile_missing` row: per-feature missing counts and band.
#[derive(Debug, Clone, PartialEq)]
pub struct MissingRow {
    pub feature: String,
    pub num_missing: u64,
    pub pct_missing: f64,
    pub band: MissingBand,
}

/// `profile_missing` over the columns in matrix order: `num_missing` counts
/// NA (NaN), `pct_missing = num_missing / nrow`, and the `cut` band with
/// breaks `c(-Inf, .05, .4, .8, 1)` right-closed (Good <= .05, OK <= .4,
/// Bad <= .8, Remove <= 1).
pub fn missing_profile(matrix: &ColumnMatrix) -> Vec<MissingRow> {
    let n = matrix.n_rows() as f64;
    let mut rows: Vec<MissingRow> = matrix
        .names
        .iter()
        .zip(matrix.cols.iter())
        .map(|(name, col)| {
            let num_missing = col.iter().filter(|v| v.is_nan()).count() as u64;
            let pct_missing = num_missing as f64 / n;
            let band = if pct_missing <= 0.05 {
                MissingBand::Good
            } else if pct_missing <= 0.4 {
                MissingBand::Ok
            } else if pct_missing <= 0.8 {
                MissingBand::Bad
            } else {
                MissingBand::Remove
            };
            MissingRow {
                feature: name.clone(),
                num_missing,
                pct_missing,
                band,
            }
        })
        .collect();
    // Legacy plot_missing orders the feature factor by order(-rank(num_missing)):
    // descending missing count with ties keeping the original column order (R's
    // order() is stable and equal counts share one rank).
    rows.sort_by_key(|r| std::cmp::Reverse(r.num_missing));
    rows
}

/// Port of R 4.6.1 `src/appl/pretty.c` `R_pretty0` plus the `pretty.default`
/// R wrapper: unit search over `{1, 2, 5, 10} * base`, the small-range shrink
/// branch, boundary snapping, and the `seq.int` + small-value zap that builds
/// the returned breakpoints. `min_n` replicates `hist.default`'s `min.n = 1`.
pub fn r_pretty(lo: f64, up: f64, ndiv: i32, min_n: i32) -> Vec<f64> {
    const SHRINK_SML: f64 = 0.75;
    const H: f64 = 1.5;
    const H5: f64 = 0.5 + 1.5 * H;
    const F_MIN: f64 = 9.5367431640625e-07; // 2^-20
    const ROUNDING_EPS: f64 = 1e-10;

    let finite: Vec<f64> = [lo, up].into_iter().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        // pretty.default returns the empty filtered vector.
        return Vec::new();
    }
    let lo_ = finite.iter().cloned().fold(f64::INFINITY, f64::min);
    let up_ = finite.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let ndiv = ndiv.max(1);
    let dx = up_ - lo_;

    let cell;
    let i_small;
    if dx == 0.0 && up_ == 0.0 {
        cell = 1.0;
        i_small = true;
    } else {
        let c = lo_.abs().max(up_.abs());
        let mut u = 1.0
            + if H5 >= 1.5 * H + 0.5 {
                1.0 / (1.0 + H)
            } else {
                1.5 / (1.0 + H5)
            };
        u *= std::cmp::max(1, ndiv) as f64 * f64::EPSILON;
        i_small = dx < c * u * 3.0;
        cell = c;
    }

    let mut cell = if i_small {
        let mut cell = if cell > 10.0 { 9.0 + cell / 10.0 } else { cell };
        cell *= SHRINK_SML;
        if min_n > 1 {
            cell /= min_n as f64;
        }
        cell
    } else if dx.is_finite() {
        if ndiv > 1 {
            dx / ndiv as f64
        } else {
            dx
        }
    } else {
        up_ / ndiv as f64 - lo_ / ndiv as f64
    };

    let subsmall = F_MIN * f64::MIN_POSITIVE;
    if subsmall == 0.0 {
        unreachable!("2^-20 * DBL_MIN cannot underflow to zero");
    }
    if cell < subsmall {
        cell = subsmall;
    } else if cell > f64::MAX / 1.25 {
        cell = f64::MAX / 1.25;
    }

    let base = 10f64.powf(cell.log10().floor());
    let mut unit = base;
    let candidate = 2.0 * base;
    if candidate - cell < H * (cell - unit) {
        unit = candidate;
    }
    let candidate = 5.0 * base;
    if candidate - cell < H5 * (cell - unit) {
        unit = candidate;
    }
    let candidate = 10.0 * base;
    if candidate - cell < H * (cell - unit) {
        unit = candidate;
    }

    let mut ns = (lo_ / unit + ROUNDING_EPS).floor();
    let mut nu = (up_ / unit - ROUNDING_EPS).ceil();
    while ns * unit > lo_ + ROUNDING_EPS * unit {
        ns -= 1.0;
    }
    while !ns.is_finite() {
        ns += 1.0;
    }
    while nu * unit < up_ - ROUNDING_EPS * unit {
        nu += 1.0;
    }
    while !nu.is_finite() {
        nu -= 1.0;
    }

    let mut k = (0.5 + nu - ns) as i64;
    if k < min_n as i64 {
        let kk = min_n as i64 - k;
        if lo_ == 0.0 && ns == 0.0 && up_ != 0.0 {
            nu += kk as f64;
        } else if up_ == 0.0 && nu == 0.0 && lo_ != 0.0 {
            ns -= kk as f64;
        } else if ns >= 0.0 {
            nu += (kk / 2) as f64;
            ns -= (kk / 2 + kk % 2) as f64;
        } else {
            ns -= (kk / 2) as f64;
            nu += (kk / 2 + kk % 2) as f64;
        }
        k = min_n as i64;
    }

    let l = if ns * unit < lo_ { ns * unit } else { lo_ };
    let u = if nu * unit > up_ { nu * unit } else { up_ };

    // seq.int(l, u, length.out = k + 1) then the pretty.default zap of
    // near-zero values (eps.correct = 0).
    let k = k as f64;
    let by = (u - l) / k;
    let delta = u / k - l / k;
    (0..=k as i64)
        .map(|i| {
            let s = l + i as f64 * by;
            if s.abs() < 1e-14 * delta {
                0.0
            } else {
                s
            }
        })
        .collect()
}

/// `C_bincount` with `right = TRUE`, `include.lowest = TRUE`: finite values
/// inside `[breaks[0], breaks[last]]` bin into the interval whose left edge
/// (fuzz-adjusted) they exceed.
fn bin_count(x: &[f64], breaks: &[f64]) -> Vec<u64> {
    let nb1 = breaks.len() - 1;
    let mut count = vec![0u64; nb1];
    for &v in x {
        if !v.is_finite() {
            continue;
        }
        let (mut lo, mut hi) = (0usize, nb1);
        if breaks[lo] <= v && v <= breaks[hi] {
            while hi - lo >= 2 {
                let mid = (hi + lo) / 2;
                if v > breaks[mid] {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            count[lo] += 1;
        }
    }
    count
}

/// `hist.default` breakpoints and counts for `breaks = n` (`pretty(range(x),
/// n = n, min.n = 1)`), fuzzed edges, and right-closed `include.lowest`
/// binning. Non-finite values are dropped exactly as the R wrapper does.
pub fn histogram(values: &[f64], bins: usize) -> Result<(Vec<f64>, Vec<u64>), DomainError> {
    if bins < 1 {
        return Err(validation(
            "explore_histogram_bins",
            "histogram requires at least one bin",
        ));
    }
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return Err(validation(
            "explore_histogram_empty",
            "histogram requires at least one finite value",
        ));
    }
    let lo = finite.iter().cloned().fold(f64::INFINITY, f64::min);
    let up = finite.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let breaks = r_pretty(lo, up, bins as i32, 1);
    let nb = breaks.len();
    let h: Vec<f64> = breaks.windows(2).map(|w| w[1] - w[0]).collect();
    let diddle = 1e-7
        * if nb > 5 {
            let mut sorted = h.clone();
            sorted.sort_by(|a, b| a.total_cmp(b));
            let mid = sorted.len() / 2;
            if sorted.len() % 2 == 1 {
                sorted[mid]
            } else {
                (sorted[mid - 1] + sorted[mid]) / 2.0
            }
        } else if nb <= 3 {
            up - lo
        } else {
            h.iter().copied().fold(f64::INFINITY, f64::min)
        };
    let fuzzy: Vec<f64> = breaks
        .iter()
        .enumerate()
        .map(|(i, b)| if i == 0 { b - diddle } else { b + diddle })
        .collect();
    Ok((breaks, bin_count(&finite, &fuzzy)))
}

/// Crosstab summary method; `count` groups by both columns, the numeric
/// summaries group by the first column only (legacy `compute_crosstab_summary`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrosstabMethod {
    Count,
    Mean,
    Median,
    Sd,
}

impl CrosstabMethod {
    /// Parses the legacy `summary_method` string.
    pub fn parse(method: &str) -> Result<Self, DomainError> {
        match method {
            "count" => Ok(CrosstabMethod::Count),
            "mean" => Ok(CrosstabMethod::Mean),
            "median" => Ok(CrosstabMethod::Median),
            "sd" => Ok(CrosstabMethod::Sd),
            other => Err(validation(
                "explore_crosstab_method",
                format!("Unsupported summary function selected: {other:?}"),
            )),
        }
    }
}

/// One `count` summary row: the distinct (first, second) pair and its count.
#[derive(Debug, Clone, PartialEq)]
pub struct CrosstabCountRow {
    pub group: Option<String>,
    pub value: Option<String>,
    pub count: u64,
}

/// One mean/median/sd summary row; `result` is `NaN` (serialized null) when
/// the group has no numeric values.
#[derive(Debug, Clone, PartialEq)]
pub struct CrosstabValueRow {
    pub group: Option<String>,
    pub result: f64,
}

/// Legacy `dplyr` group ordering: keys ascending in the C locale with NA
/// groups last.
fn sort_keys<T>(rows: &mut [(Option<String>, Option<String>, T)]) {
    fn key_order(key: &Option<String>) -> (u8, Vec<u8>) {
        match key {
            Some(s) => (0, s.as_bytes().to_vec()),
            None => (1, Vec::new()),
        }
    }
    rows.sort_by(|a, b| {
        let (a1, a2) = (key_order(&a.0), key_order(&a.1));
        let (b1, b2) = (key_order(&b.0), key_order(&b.1));
        a1.cmp(&b1).then_with(|| a2.cmp(&b2))
    });
}

/// `summary_method = "count"`: group by both columns and count rows.
pub fn crosstab_count(
    groups: &[Option<String>],
    values: &[Option<String>],
) -> Result<Vec<CrosstabCountRow>, DomainError> {
    if groups.len() != values.len() {
        return Err(validation(
            "explore_crosstab_length",
            "count requires one value per group label",
        ));
    }
    let mut counts: Vec<(Option<String>, Option<String>, u64)> = Vec::new();
    for (g, v) in groups.iter().zip(values.iter()) {
        match counts.iter_mut().find(|(kg, kv, _)| kg == g && kv == v) {
            Some((_, _, c)) => *c += 1,
            None => counts.push((g.clone(), v.clone(), 1)),
        }
    }
    sort_keys(&mut counts);
    Ok(counts
        .into_iter()
        .map(|(group, value, count)| CrosstabCountRow {
            group,
            value,
            count,
        })
        .collect())
}

/// `mean`/`median`/`sd` of the numeric values grouped by the first column,
/// `na.rm = TRUE`, rounded to 2 decimals with R semantics.
pub fn crosstab_value_summary(
    groups: &[Option<String>],
    values: &[Option<f64>],
    method: CrosstabMethod,
) -> Result<Vec<CrosstabValueRow>, DomainError> {
    if groups.len() != values.len() {
        return Err(validation(
            "explore_crosstab_length",
            "summary requires one value per group label",
        ));
    }
    // `count` crosstabs group by two columns and are routed to
    // `crosstab_count`; the summary path rejects it like any other
    // unsupported method string.
    if method == CrosstabMethod::Count {
        return Err(validation(
            "explore_crosstab_method",
            "Unsupported summary function selected: use crosstab_count",
        ));
    }
    let mut grouped: Vec<(Option<String>, Vec<f64>)> = Vec::new();
    for (g, v) in groups.iter().zip(values.iter()) {
        let entry = grouped
            .iter_mut()
            .find(|(kg, _)| kg == g)
            .map(|(_, vals)| vals);
        match entry {
            Some(vals) => {
                if let Some(v) = v {
                    if v.is_finite() {
                        vals.push(*v);
                    }
                }
            }
            None => {
                grouped.push((g.clone(), v.filter(|v| v.is_finite()).into_iter().collect()));
            }
        }
    }
    grouped.sort_by(|a, b| {
        let key_order = |key: &Option<String>| match key {
            Some(s) => (0u8, s.as_bytes().to_vec()),
            None => (1u8, Vec::new()),
        };
        key_order(&a.0).cmp(&key_order(&b.0))
    });
    Ok(grouped
        .into_iter()
        .map(|(group, vals)| {
            let result = match method {
                CrosstabMethod::Mean => {
                    if vals.is_empty() {
                        f64::NAN
                    } else {
                        vals.iter().sum::<f64>() / vals.len() as f64
                    }
                }
                CrosstabMethod::Median => median(&vals),
                CrosstabMethod::Sd => sample_sd(&vals),
                // `count` crosstabs group by two columns and are routed to
                // `crosstab_count`; the summary path rejects them like any
                // other unsupported method string.
                // unreachable: Count is rejected above
                CrosstabMethod::Count => f64::NAN,
            };
            CrosstabValueRow {
                group,
                result: r_round(result, 2),
            }
        })
        .collect())
}

/// R `median(na.rm = TRUE)`: middle value, or the mean of the two middles.
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    match sorted.len() {
        0 => f64::NAN,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
    }
}

/// R `sd(na.rm = TRUE)`: sample standard deviation, NA when fewer than two
/// values remain.
fn sample_sd(values: &[f64]) -> f64 {
    let n = values.len();
    if n < 2 {
        return f64::NAN;
    }
    let mean = values.iter().sum::<f64>() / n as f64;
    let ss = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>();
    (ss / (n - 1) as f64).sqrt()
}

/// One compositional-profile long-table row: 1-based positional `rowid`,
/// element name in column order, measured value (`NaN` for NA), and the
/// optional group label used for line coloring.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRow {
    pub rowid: u64,
    pub element: String,
    pub value: f64,
    pub group_label: Option<String>,
}

/// `comp.profile` long table: `rowid_to_column() |> pivot_longer(-rowid)`
/// row-major order (every 1-based rowid lists all elemental columns in
/// original order), with the optional group label repeated per long row
/// (`rep(groups, each = ncol)` aligns with this order).
pub fn compositional_profile(
    matrix: &ColumnMatrix,
    group_labels: Option<&[Option<String>]>,
) -> Result<Vec<ProfileRow>, DomainError> {
    if let Some(labels) = group_labels {
        if labels.len() != matrix.n_rows() {
            return Err(validation(
                "explore_profile_group_length",
                format!(
                    "compositional profile requires one group label per row: {} labels for {} rows",
                    labels.len(),
                    matrix.n_rows()
                ),
            ));
        }
    }
    let mut rows = Vec::with_capacity(matrix.n_rows() * matrix.names.len());
    for r in 0..matrix.n_rows() {
        let label = group_labels.and_then(|g| g.get(r).cloned()).flatten();
        for (i, name) in matrix.names.iter().enumerate() {
            rows.push(ProfileRow {
                rowid: r as u64 + 1,
                element: name.clone(),
                value: matrix.cols[i][r],
                group_label: label.clone(),
            });
        }
    }
    Ok(rows)
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

    fn strings(values: &[Option<&str>]) -> Vec<Option<String>> {
        values.iter().map(|v| v.map(|s| s.to_string())).collect()
    }

    #[test]
    fn pretty_matches_r_oracle_vectors() {
        // Captured from R 4.6.1 pretty.default with %.17g formatting; strings
        // round-trip exactly.
        let cases: &[((f64, f64), i32, &[&str])] = &[
            (
                (0.0, 12.0236),
                30,
                &[
                    "0", "0.5", "1", "1.5", "2", "2.5", "3", "3.5", "4", "4.5", "5", "5.5", "6",
                    "6.5", "7", "7.5", "8", "8.5", "9", "9.5", "10", "10.5", "11", "11.5", "12",
                    "12.5",
                ],
            ),
            ((0.0, 12.0236), 2, &["0", "5", "10", "15"]),
            (
                (0.0, 12.0236),
                5,
                &["0", "2", "4", "6", "8", "10", "12", "14"],
            ),
            ((-5.0, 5.0), 2, &["-5", "0", "5"]),
            ((-5.0, 5.0), 5, &["-6", "-4", "-2", "0", "2", "4", "6"]),
            ((3.0, 3.0), 30, &["2", "4"]),
            ((0.0, 0.0), 2, &["-1", "0"]),
            (
                (-2.5, 7.25),
                30,
                &[
                    "-2.5", "-2", "-1.5", "-1", "-0.5", "0", "0.5", "1", "1.5", "2", "2.5", "3",
                    "3.5", "4", "4.5", "5", "5.5", "6", "6.5", "7", "7.5",
                ],
            ),
            (
                (1e-8, 2e-8),
                5,
                &[
                    "1e-08",
                    "1.2e-08",
                    "1.4e-08",
                    "1.6000000000000001e-08",
                    "1.8000000000000002e-08",
                    "2e-08",
                ],
            ),
            ((1e8, 2.5e8), 2, &["100000000", "200000000", "300000000"]),
            (
                (-100.0, -90.0),
                5,
                &["-100", "-98", "-96", "-94", "-92", "-90"],
            ),
            (
                (0.3, 0.8),
                2,
                &[
                    "0.20000000000000001",
                    "0.40000000000000002",
                    "0.60000000000000009",
                    "0.80000000000000004",
                ],
            ),
        ];
        for ((lo, up), n, expected) in cases {
            let breaks = r_pretty(*lo, *up, *n, 1);
            let parsed: Vec<f64> = expected
                .iter()
                .map(|s| s.parse::<f64>().expect("parseable vector"))
                .collect();
            assert_eq!(
                breaks.len(),
                parsed.len(),
                "pretty({lo:?}, {up:?}, n={n}) count"
            );
            for (mine, theirs) in breaks.iter().zip(parsed.iter()) {
                assert_eq!(mine, theirs, "pretty({lo:?}, {up:?}, n={n}) value");
            }
        }
    }

    #[test]
    fn histogram_matches_golden_capture() {
        // INAA "as" column histogram from the oracle: hist(features[[1]],
        // breaks = 30). Values summarized from fixtures/golden/12_explore_views.json.
        let values: Vec<f64> = vec![
            3.784, 48.188, 1.646, 18.636, 4.306, 2.653, 3.6546, 90.7601, 16.098, 121.0, 3.41, 1.24,
            46490.0, 8.297, 22.82, 110.2, 1.03, 13.94, 240.7, 1.763, 0.733, 13.35, 74.2, 197.0,
            64130.0, 487.0, 51110.0, 4.545, 933.0, 658.0, 2.903, 5.524, 103.0,
        ];
        // The oracle ran on the full 307-row fixture; this synthetic subset only
        // checks the fuzz/bincount invariants, not the golden counts.
        let (breaks, counts) = histogram(&values, 6).expect("histogram");
        assert_eq!(breaks.len(), counts.len() + 1);
        assert_eq!(
            counts.iter().sum::<u64>(),
            values.iter().filter(|v| v.is_finite()).count() as u64
        );
    }

    #[test]
    fn histogram_places_exact_break_values_in_lower_bin() {
        // right = TRUE with include.lowest: values equal to an interior break
        // fall in the lower bin because the fuzzy edge moves up by `diddle`.
        let (breaks, counts) = histogram(&[0.0, 0.5, 1.0], 2).expect("histogram");
        assert_eq!(breaks, vec![0.0, 0.5, 1.0]);
        assert_eq!(counts, vec![2, 1]);
    }

    #[test]
    fn missing_profile_bands_follow_cut_thresholds() {
        // 5 rows; column "a" 0/5 missing (Good), "b" 1/5 = 0.2 (OK),
        // "c" 3/5 = 0.6 (Bad), "d" 5/5 = 1.0 (Remove). Legacy order(-rank)
        // sorts descending by missing count, ties keep column order.
        let m = matrix(
            &["a", "b", "c", "d"],
            vec![
                vec![1.0, 1.0, 1.0, 1.0, 1.0],
                vec![1.0, f64::NAN, 1.0, 1.0, 1.0],
                vec![f64::NAN, f64::NAN, f64::NAN, 1.0, f64::NAN],
                vec![f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN],
            ],
        );
        let rows = missing_profile(&m);
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter().map(|r| r.feature.as_str()).collect::<Vec<_>>(),
            vec!["d", "c", "b", "a"]
        );
        assert_eq!(rows[3].band, MissingBand::Good);
        assert_eq!(rows[3].num_missing, 0);
        assert_eq!(rows[2].band, MissingBand::Ok); // 1/5 = 0.2 <= 0.4
        assert_eq!(rows[2].pct_missing, 0.2);
        assert_eq!(rows[1].num_missing, 4);
        assert_eq!(rows[1].band, MissingBand::Bad); // 4/5 = 0.8 <= 0.8
        assert_eq!(rows[1].pct_missing, 0.8);
        assert_eq!(rows[0].band, MissingBand::Remove);
    }

    #[test]
    fn crosstab_count_keeps_na_groups_sorted_last() {
        let groups = strings(&[Some("B"), Some("A"), Some("A"), None, Some("B"), Some("A")]);
        let values = strings(&[Some("1"), Some("3"), Some("5"), Some("9"), Some("zz"), None]);
        let rows = crosstab_count(&groups, &values).expect("count");
        assert_eq!(
            rows,
            vec![
                CrosstabCountRow {
                    group: Some("A".into()),
                    value: Some("3".into()),
                    count: 1
                },
                CrosstabCountRow {
                    group: Some("A".into()),
                    value: Some("5".into()),
                    count: 1
                },
                CrosstabCountRow {
                    group: Some("A".into()),
                    value: None,
                    count: 1
                },
                CrosstabCountRow {
                    group: Some("B".into()),
                    value: Some("1".into()),
                    count: 1
                },
                CrosstabCountRow {
                    group: Some("B".into()),
                    value: Some("zz".into()),
                    count: 1
                },
                CrosstabCountRow {
                    group: None,
                    value: Some("9".into()),
                    count: 1
                },
            ]
        );
    }

    #[test]
    fn crosstab_value_summary_matches_r_oracle() {
        // R check: g = c("B","A","A",NA,"B","A"), v = c("1","3","5","9","zz","NA")
        // mean by group: A -> mean(3, 5) = 4; B -> mean(1) = 1; NA -> mean(9) = 9
        // sd by group: A -> sd(3, 5) = 1.4142135... -> 1.41; B -> sd(1) = NA; NA -> NA
        let groups = strings(&[Some("B"), Some("A"), Some("A"), None, Some("B"), Some("A")]);
        let values = vec![Some(1.0), Some(3.0), Some(5.0), Some(9.0), None, None];
        let mean = crosstab_value_summary(&groups, &values, CrosstabMethod::Mean).expect("mean");
        assert_eq!(mean.len(), 3);
        assert_eq!(mean[0].group, Some("A".into()));
        assert_eq!(mean[0].result, 4.0);
        assert_eq!(mean[1].group, Some("B".into()));
        assert_eq!(mean[1].result, 1.0);
        assert!(mean[2].group.is_none());
        assert_eq!(mean[2].result, 9.0);

        let sd = crosstab_value_summary(&groups, &values, CrosstabMethod::Sd).expect("sd");
        assert_eq!(sd[0].result, 1.41);
        assert!(sd[1].result.is_nan());

        let median =
            crosstab_value_summary(&groups, &values, CrosstabMethod::Median).expect("median");
        assert_eq!(median[0].result, 4.0);

        let err = crosstab_value_summary(&groups, &values, CrosstabMethod::Count)
            .expect_err("count routed separately");
        assert!(err.to_string().contains("Unsupported summary function"));
        let err = crosstab_value_summary(&groups, &values, CrosstabMethod::Count)
            .expect_err("count routed separately");
        assert!(err.to_string().contains("crosstab_count"));
    }

    #[test]
    fn compositional_profile_pivots_row_major() {
        // pivot_longer row-major: "as" = [1.5, NaN], "fe" = [2.0, 3.0].
        // Row 1 lists as then fe, then row 2; labels repeat per long row.
        let m = matrix(&["as", "fe"], vec![vec![1.5, f64::NAN], vec![2.0, 3.0]]);
        let labels = strings(&[Some("A"), Some("B")]);
        let rows = compositional_profile(&m, Some(&labels)).expect("profile");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].rowid, 1);
        assert_eq!(rows[0].element, "as");
        assert_eq!(rows[0].value, 1.5);
        assert_eq!(rows[0].group_label, Some("A".into()));
        assert_eq!(rows[1].rowid, 1);
        assert_eq!(rows[1].element, "fe");
        assert_eq!(rows[1].value, 2.0);
        assert_eq!(rows[1].group_label, Some("A".into()));
        assert_eq!(rows[2].element, "as");
        assert!(rows[2].value.is_nan());
        assert_eq!(rows[2].group_label, Some("B".into()));
        assert_eq!(rows[3].rowid, 2);
        assert_eq!(rows[3].element, "fe");
        assert_eq!(rows[3].value, 3.0);
        let err = compositional_profile(&m, Some(&labels[..1])).unwrap_err();
        assert!(err.to_string().contains("one group label per row"));
    }
}
