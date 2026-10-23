//! Multiple imputation by chained equations: a port of the legacy
//! `mice::complete(mice::mice(x, method = m))` call (Section 8.3) for the
//! three methods the legacy app offered — predictive mean matching (`pmm`),
//! weighted predictive mean matching (`midastouch`), and random forest
//! (`rf`, ranger backend). Ported from mice 3.16.0's R sources.
//!
//! Parity class D: the algorithms follow mice step by step, but the random
//! stream is a seeded ChaCha12 generator rather than R's Mersenne-Twister
//! (and ranger's own C++ generator), so outputs match R's *distribution* of
//! imputations, verified against R oracle runs, not R's exact draws.
//!
//! Sampler (mice defaults): every column predicts every other; columns that
//! mice drops (constant, or collinear with |r| >= 0.999) are neither imputed
//! nor used as predictors; missing cells start as random draws from their
//! column's observed values; then `maxit` (5) passes visit the incomplete
//! columns left to right, each re-imputed from the current values of the
//! others after mice's `remove.lindep` predictor screening. One completed
//! dataset is returned (the legacy app used the first of mice's chains; each
//! chain has the same distribution).

#![allow(clippy::needless_range_loop)] // matrix index algebra mirrors mice's R code

use archaeodash_domain::DomainError;
use rand::RngExt;
use rand::SeedableRng;
use rand_chacha::ChaCha12Rng;

use crate::ColumnMatrix;

/// Imputation methods offered by the legacy app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImputeMethod {
    /// Predictive mean matching (`mice.impute.pmm`, type-1 matching, 5 donors).
    Pmm,
    /// Weighted predictive mean matching (`mice.impute.midastouch`).
    MidasTouch,
    /// Random-forest donors (`mice.impute.rf`, 10 ranger trees).
    Rf,
}

/// mice's default number of sampler iterations.
pub const MICE_MAXIT: usize = 5;

/// Result of one imputation run.
#[derive(Debug, Clone, PartialEq)]
pub struct Imputation {
    /// The matrix with imputed cells filled.
    pub matrix: ColumnMatrix,
    /// Number of cells that were missing and are now filled.
    pub imputed_cells: u64,
    /// Columns mice leaves untouched (constant or collinear); their missing
    /// cells stay missing.
    pub excluded: Vec<String>,
}

const RIDGE: f64 = 1e-5;
const LINDEP_EPS: f64 = 1e-4;
const LINDEP_MAXCOR: f64 = 0.99;
const COLLINEAR_THRESHOLD: f64 = 0.999;
const CONSTANT_EPS: f64 = 1e-8;
const PMM_DONORS: usize = 5;
const RF_TREES: usize = 10;
const RF_MIN_NODE_SIZE: usize = 5;

fn failure(message: impl Into<String>) -> DomainError {
    DomainError::validation("imputation_failed", message)
}

/// Imputes every missing (non-finite) cell of `m` with `method`, using a
/// replayable `seed`.
pub fn impute(
    m: &ColumnMatrix,
    method: ImputeMethod,
    seed: u64,
    maxit: usize,
) -> Result<Imputation, DomainError> {
    let n = m.n_rows();
    let p = m.cols.len();
    let observed: Vec<Vec<bool>> = m
        .cols
        .iter()
        .map(|c| c.iter().map(|v| v.is_finite()).collect())
        .collect();
    let nobs: Vec<usize> = observed
        .iter()
        .map(|o| o.iter().filter(|&&b| b).count())
        .collect();
    for (j, &count) in nobs.iter().enumerate() {
        if count == 0 && n > 0 {
            return Err(failure(format!(
                "column {:?} has no observed values to impute from",
                m.names[j]
            )));
        }
    }

    // mice's remove.constant / remove.collinear: such columns are not
    // imputed and do not predict.
    let mut excluded = vec![false; p];
    for j in 0..p {
        let obs: Vec<f64> = m.cols[j]
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        if obs.len() < 2 || variance(&obs) < CONSTANT_EPS {
            excluded[j] = true;
        }
    }
    for j in find_collinear(&m.cols, &nobs) {
        excluded[j] = true;
    }

    let mut cols = m.cols.clone();
    let mut rng = ChaCha12Rng::seed_from_u64(seed);
    let targets: Vec<usize> = (0..p).filter(|&j| !excluded[j] && nobs[j] < n).collect();

    // Initialization (mice.impute.sample): random observed values.
    for &j in &targets {
        let obs: Vec<f64> = cols[j].iter().copied().filter(|v| v.is_finite()).collect();
        for i in 0..n {
            if !observed[j][i] {
                cols[j][i] = obs[rng.random_range(0..obs.len())];
            }
        }
    }

    for _ in 0..maxit {
        for &j in &targets {
            let ry = &observed[j];
            let predictors: Vec<usize> = (0..p).filter(|&k| k != j && !excluded[k]).collect();
            let x: Vec<&[f64]> = predictors.iter().map(|&k| cols[k].as_slice()).collect();
            let keep = remove_lindep(&x, &cols[j], ry);
            let x: Vec<&[f64]> = x
                .into_iter()
                .zip(&keep)
                .filter_map(|(c, &k)| k.then_some(c))
                .collect();
            let imputes = match method {
                ImputeMethod::Pmm => pmm(&cols[j], ry, &x, &mut rng)?,
                ImputeMethod::MidasTouch => midastouch(&cols[j], ry, &x, &mut rng)?,
                ImputeMethod::Rf => rf(&cols[j], ry, &x, &mut rng),
            };
            let mut it = imputes.into_iter();
            for i in 0..n {
                if !ry[i] {
                    cols[j][i] = it.next().unwrap_or(f64::NAN);
                }
            }
        }
    }

    let imputed_cells = targets.iter().map(|&j| (n - nobs[j]) as u64).sum();
    Ok(Imputation {
        matrix: ColumnMatrix {
            names: m.names.clone(),
            cols,
        },
        imputed_cells,
        excluded: (0..p)
            .filter(|&j| excluded[j] && nobs[j] < n)
            .map(|j| m.names[j].clone())
            .collect(),
    })
}

// ---------------------------------------------------------------- methods

/// `mice.impute.pmm` with type-1 matching: observed predictions use the
/// least-squares fit, missing ones a posterior draw; each missing case takes
/// the observed value of one of its 5 nearest donors.
fn pmm(
    y: &[f64],
    ry: &[bool],
    x: &[&[f64]],
    rng: &mut ChaCha12Rng,
) -> Result<Vec<f64>, DomainError> {
    let design = Design::new(x, ry);
    let yobs: Vec<f64> = obs_values(y, ry);
    let draw = norm_draw(&design.obs, &yobs, rng)?;
    let yhat_obs = design
        .obs
        .iter()
        .map(|row| dot(row, &draw.coef))
        .collect::<Vec<_>>();
    let yhat_mis = design
        .mis
        .iter()
        .map(|row| dot(row, &draw.beta))
        .collect::<Vec<_>>();
    let idx = match_index(&yhat_obs, &yhat_mis, PMM_DONORS, rng);
    Ok(idx.into_iter().map(|i| yobs[i]).collect())
}

/// Posterior draw of `.norm.draw`: least squares (QR), residual sigma drawn
/// from a scaled inverse chi-square, coefficients from N(coef, sigma² V).
struct NormDraw {
    coef: Vec<f64>,
    beta: Vec<f64>,
}

fn norm_draw(
    xobs: &[Vec<f64>],
    yobs: &[f64],
    rng: &mut ChaCha12Rng,
) -> Result<NormDraw, DomainError> {
    let p = xobs.first().map(|r| r.len()).unwrap_or(1);
    let n = xobs.len();
    let df = (n as f64 - p as f64).max(1.0);
    let fit = least_squares(xobs, yobs);
    let xtx = crossprod(xobs);
    let v = if fit.full_rank { invert(&xtx) } else { None }
        .or_else(|| {
            // estimice: near multi-collinearity gets a small ridge penalty.
            let mut pen = xtx.clone();
            for (k, row) in pen.iter_mut().enumerate() {
                row[k] += xtx[k][k] * RIDGE;
            }
            invert(&pen)
        })
        .ok_or_else(|| failure("the regression for imputation is singular"))?;
    let rss: f64 = fit.residuals.iter().map(|r| r * r).sum();
    let sigma = (rss / chisq(df, rng)).sqrt();
    let sym: Vec<Vec<f64>> = (0..p)
        .map(|a| (0..p).map(|b| (v[a][b] + v[b][a]) / 2.0).collect())
        .collect();
    let l = cholesky(&sym)
        .ok_or_else(|| failure("the coefficient covariance is not positive definite"))?;
    let z: Vec<f64> = (0..p).map(|_| normal(rng)).collect();
    let beta = (0..p)
        .map(|a| fit.coef[a] + (0..=a).map(|b| l[a][b] * z[b]).sum::<f64>() * sigma)
        .collect();
    Ok(NormDraw {
        coef: fit.coef,
        beta,
    })
}

/// mice's `matchindex`: donors are shuffled (random tie-breaking) and
/// sorted by prediction; each target takes one of its `k` nearest uniformly.
fn match_index(donors: &[f64], targets: &[f64], k: usize, rng: &mut ChaCha12Rng) -> Vec<usize> {
    let n1 = donors.len();
    let k = k.clamp(1, n1.max(1));
    let mut order: Vec<usize> = (0..n1).collect();
    for i in (1..n1).rev() {
        let j = rng.random_range(0..=i);
        order.swap(i, j);
    }
    order.sort_by(|&a, &b| donors[a].total_cmp(&donors[b]));
    let sorted: Vec<f64> = order.iter().map(|&i| donors[i]).collect();
    targets
        .iter()
        .map(|&t| {
            let pos = sorted.partition_point(|&d| d < t);
            let (mut lo, mut hi) = (pos, pos);
            let mut picked = Vec::with_capacity(k);
            while picked.len() < k {
                let take_low = match (lo > 0, hi < n1) {
                    (true, true) => (t - sorted[lo - 1]).abs() <= (sorted[hi] - t).abs(),
                    (true, false) => true,
                    (false, true) => false,
                    (false, false) => break,
                };
                if take_low {
                    lo -= 1;
                    picked.push(order[lo]);
                } else {
                    picked.push(order[hi]);
                    hi += 1;
                }
            }
            picked[rng.random_range(0..picked.len())]
        })
        .collect()
}

/// `mice.impute.midastouch` (outout = TRUE): bootstrap-weighted ridge
/// regression, leave-one-out donor predictions, and donor selection with
/// probabilities proportional to distance^-kappa times bootstrap weight.
fn midastouch(
    y: &[f64],
    ry: &[bool],
    x: &[&[f64]],
    rng: &mut ChaCha12Rng,
) -> Result<Vec<f64>, DomainError> {
    let sminx = f64::EPSILON.powf(0.25);
    let design = Design::new(x, ry);
    let xobs = &design.obs;
    let xmis = &design.mis;
    let yobs = obs_values(y, ry);
    let nobs = xobs.len();
    let m = xobs.first().map(|r| r.len()).unwrap_or(1);

    // bootfunc.plain: bootstrap counts.
    let mut omega = vec![0.0; nobs];
    for _ in 0..nobs {
        omega[rng.random_range(0..nobs)] += 1.0;
    }

    let mut xcx = vec![vec![0.0; m]; m];
    let mut xy = vec![0.0; m];
    for (i, row) in xobs.iter().enumerate() {
        for a in 0..m {
            xy[a] += row[a] * yobs[i] * omega[i];
            for b in 0..m {
                xcx[a][b] += row[a] * row[b] * omega[i];
            }
        }
    }
    for (a, row) in xcx.iter_mut().enumerate().skip(1) {
        row[a] *= 1.0 + RIDGE;
    }
    for (a, row) in xcx.iter_mut().enumerate() {
        if row[a] == 0.0 {
            row[a] = sminx.max(RIDGE);
        }
    }
    let beta = solve(&xcx, &xy)
        .ok_or_else(|| failure("the weighted regression for imputation is singular"))?;
    let yhat_obs: Vec<f64> = xobs.iter().map(|row| dot(row, &beta)).collect();

    let wsum = |f: &dyn Fn(usize) -> f64| (0..nobs).map(|i| omega[i] * f(i)).sum::<f64>();
    let mean_y = wsum(&|i| yobs[i]) / nobs as f64;
    let r2 =
        1.0 - wsum(&|i| (yobs[i] - yhat_obs[i]).powi(2)) / wsum(&|i| (yobs[i] - mean_y).powi(2));
    let mut kappa = (50.0 * r2 / (1.0 - r2)).powf(3.0 / 8.0).min(100.0);
    if kappa.is_nan() {
        kappa = 3.0;
    }

    // Leave-one-out coefficients per donor.
    let nmis = xmis.len();
    let mut dist = vec![vec![0.0; nmis]; nobs];
    for i in 0..nobs {
        let row = &xobs[i];
        let mut xx = xcx.clone();
        for a in 0..m {
            for b in 0..m {
                let mut pre = row[a] * row[b] * omega[i];
                if a == b && a > 0 {
                    pre *= 1.0 + RIDGE;
                }
                xx[a][b] -= pre;
            }
        }
        for (a, r) in xx.iter_mut().enumerate().skip(1) {
            if r[a] == 0.0 {
                r[a] = sminx.max(RIDGE);
            }
        }
        let rhs: Vec<f64> = (0..m)
            .map(|a| xy[a] - row[a] * yobs[i] * omega[i])
            .collect();
        let b_i = solve(&xx, &rhs)
            .ok_or_else(|| failure("a leave-one-out regression for imputation is singular"))?;
        let yhat_don = dot(row, &b_i);
        for (k, mrow) in xmis.iter().enumerate() {
            dist[i][k] = yhat_don - dot(mrow, &b_i);
        }
    }

    let maxx = f64::MAX.sqrt();
    let minx = f64::EPSILON.sqrt();
    let clamp = |v: f64| if v.is_nan() { v } else { v.min(maxx).max(minx) };
    let mut out = Vec::with_capacity(nmis);
    for k in 0..nmis {
        let probs: Vec<f64> = (0..nobs)
            .map(|i| clamp(1.0 / dist[i][k].abs().powf(kappa)) * omega[i])
            .collect();
        let total = clamp(probs.iter().filter(|v| !v.is_nan()).sum());
        let mut u = rng.random::<f64>() * probs.iter().map(|p| p / total).sum::<f64>();
        let mut chosen = nobs - 1;
        for (i, p) in probs.iter().enumerate() {
            let w = p / total;
            if w <= 0.0 {
                continue;
            }
            if u < w {
                chosen = i;
                break;
            }
            u -= w;
            chosen = i;
        }
        out.push(yobs[chosen]);
    }
    Ok(out)
}

/// `mice.impute.rf` with the ranger backend: a 10-tree regression forest on
/// the observed cases; each missing case draws one donor from the observed
/// cases that share its terminal node in any tree.
fn rf(y: &[f64], ry: &[bool], x: &[&[f64]], rng: &mut ChaCha12Rng) -> Vec<f64> {
    let yobs = obs_values(y, ry);
    let obs_rows: Vec<usize> = (0..y.len()).filter(|&i| ry[i]).collect();
    let mis_rows: Vec<usize> = (0..y.len()).filter(|&i| !ry[i]).collect();
    let nobs = obs_rows.len();
    if x.is_empty() {
        // No usable predictors: every observed case is a donor.
        return mis_rows
            .iter()
            .map(|_| yobs[rng.random_range(0..nobs)])
            .collect();
    }
    let feature = |row: usize, f: usize| x[f][row];
    let mtry = ((x.len() as f64).sqrt().floor() as usize).max(1);
    let mut pools: Vec<Vec<f64>> = vec![Vec::new(); mis_rows.len()];
    for _ in 0..RF_TREES {
        let sample: Vec<usize> = (0..nobs).map(|_| rng.random_range(0..nobs)).collect();
        let tree = Tree::grow(&sample, &obs_rows, &yobs, x.len(), mtry, &feature, rng);
        let mut leaves: std::collections::HashMap<usize, Vec<f64>> =
            std::collections::HashMap::new();
        for (k, &row) in obs_rows.iter().enumerate() {
            leaves
                .entry(tree.leaf(row, &feature))
                .or_default()
                .push(yobs[k]);
        }
        for (k, &row) in mis_rows.iter().enumerate() {
            if let Some(donors) = leaves.get(&tree.leaf(row, &feature)) {
                pools[k].extend_from_slice(donors);
            }
        }
    }
    pools
        .into_iter()
        .map(|pool| {
            if pool.is_empty() {
                yobs[rng.random_range(0..nobs)]
            } else {
                pool[rng.random_range(0..pool.len())]
            }
        })
        .collect()
}

/// A ranger-style regression tree: at each node `mtry` random features are
/// tried at every midpoint between consecutive distinct values, keeping the
/// split with the largest `sumL²/nL + sumR²/nR`; nodes of at most
/// `RF_MIN_NODE_SIZE` cases, or with a constant response, are terminal.
enum Node {
    Leaf,
    Split {
        feature: usize,
        value: f64,
        left: usize,
        right: usize,
    },
}

struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    /// `sample` indexes the observed cases (bootstrap, with repeats);
    /// `rows[k]` maps case `k` to its data row.
    fn grow(
        sample: &[usize],
        rows: &[usize],
        yobs: &[f64],
        n_features: usize,
        mtry: usize,
        feature: &dyn Fn(usize, usize) -> f64,
        rng: &mut ChaCha12Rng,
    ) -> Self {
        let mut tree = Tree { nodes: Vec::new() };
        let mut stack: Vec<(usize, Vec<usize>)> = vec![(0, sample.to_vec())];
        tree.nodes.push(Node::Leaf);
        while let Some((id, cases)) = stack.pop() {
            let first = yobs[cases[0]];
            if cases.len() <= RF_MIN_NODE_SIZE || cases.iter().all(|&c| yobs[c] == first) {
                continue;
            }
            let mut candidates: Vec<usize> = (0..n_features).collect();
            for i in 0..mtry.min(n_features) {
                let j = rng.random_range(i..n_features);
                candidates.swap(i, j);
            }
            let mut best: Option<(f64, usize, f64)> = None;
            for &f in &candidates[..mtry.min(n_features)] {
                let mut pairs: Vec<(f64, f64)> = cases
                    .iter()
                    .map(|&c| (feature(rows[c], f), yobs[c]))
                    .collect();
                pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
                let total: f64 = pairs.iter().map(|p| p.1).sum();
                let n = pairs.len() as f64;
                let mut sum_left = 0.0;
                for i in 0..pairs.len() - 1 {
                    sum_left += pairs[i].1;
                    if pairs[i].0 == pairs[i + 1].0 {
                        continue;
                    }
                    let n_left = (i + 1) as f64;
                    let sum_right = total - sum_left;
                    let decrease =
                        sum_left * sum_left / n_left + sum_right * sum_right / (n - n_left);
                    if best.is_none_or(|(d, _, _)| decrease > d) {
                        best = Some((decrease, f, (pairs[i].0 + pairs[i + 1].0) / 2.0));
                    }
                }
            }
            let Some((_, f, value)) = best else { continue };
            let (left_cases, right_cases): (Vec<usize>, Vec<usize>) =
                cases.iter().partition(|&&c| feature(rows[c], f) <= value);
            let left = tree.nodes.len();
            tree.nodes.push(Node::Leaf);
            let right = tree.nodes.len();
            tree.nodes.push(Node::Leaf);
            tree.nodes[id] = Node::Split {
                feature: f,
                value,
                left,
                right,
            };
            stack.push((left, left_cases));
            stack.push((right, right_cases));
        }
        tree
    }

    fn leaf(&self, row: usize, feature: &dyn Fn(usize, usize) -> f64) -> usize {
        let mut id = 0;
        while let Node::Split {
            feature: f,
            value,
            left,
            right,
        } = self.nodes[id]
        {
            id = if feature(row, f) <= value {
                left
            } else {
                right
            };
        }
        id
    }
}

// ------------------------------------------------------ predictor screening

/// mice's `find.collinear`: columns (ordered by observed count, most first)
/// whose pairwise-complete |r| with an earlier column is at least 0.999.
fn find_collinear(cols: &[Vec<f64>], nobs: &[usize]) -> Vec<usize> {
    let p = cols.len();
    let mut order: Vec<usize> = (0..p).collect();
    order.sort_by(|&a, &b| nobs[b].cmp(&nobs[a]));
    let mut out = Vec::new();
    for (b_pos, &b) in order.iter().enumerate() {
        let hit = order[..b_pos].iter().any(|&a| {
            let (xa, xb): (Vec<f64>, Vec<f64>) = cols[a]
                .iter()
                .zip(&cols[b])
                .filter(|(u, v)| u.is_finite() && v.is_finite())
                .map(|(u, v)| (*u, *v))
                .unzip();
            correlation(&xa, &xb).is_some_and(|r| r.abs() >= COLLINEAR_THRESHOLD)
        });
        if hit {
            out.push(b);
        }
    }
    out
}

/// mice's `remove.lindep`: drops near-constant predictors, predictors with
/// correlation >= 0.99 with the target, and then (via the correlation
/// matrix's eigenvalues) linearly dependent ones.
fn remove_lindep(x: &[&[f64]], y: &[f64], ry: &[bool]) -> Vec<bool> {
    if x.is_empty() {
        return Vec::new();
    }
    let yobs = obs_values(y, ry);
    if variance(&yobs) < LINDEP_EPS {
        return vec![false; x.len()];
    }
    let xobs: Vec<Vec<f64>> = x.iter().map(|c| obs_values(c, ry)).collect();
    let mut keep: Vec<bool> = xobs
        .iter()
        .map(|c| {
            variance(c) > LINDEP_EPS && correlation(c, &yobs).is_some_and(|r| r < LINDEP_MAXCOR)
        })
        .collect();
    let mut kept: Vec<usize> = (0..x.len()).filter(|&j| keep[j]).collect();
    if kept.len() <= 1 {
        return keep;
    }
    loop {
        let k = kept.len();
        let cx: Vec<Vec<f64>> = kept
            .iter()
            .map(|&a| {
                kept.iter()
                    .map(|&b| correlation(&xobs[a], &xobs[b]).unwrap_or(0.0))
                    .collect()
            })
            .collect();
        let (values, vectors) = symmetric_eigen(&cx);
        if k <= 1 || values[k - 1] / values[0] >= LINDEP_EPS {
            break;
        }
        let drop = (0..k)
            .max_by(|&a, &b| {
                vectors[a][k - 1]
                    .abs()
                    .total_cmp(&vectors[b][k - 1].abs())
                    .then(b.cmp(&a))
            })
            .unwrap_or(0);
        keep[kept[drop]] = false;
        kept.remove(drop);
    }
    keep
}

// --------------------------------------------------------------- helpers

/// Design matrices with an intercept: observed rows and missing rows.
struct Design {
    obs: Vec<Vec<f64>>,
    mis: Vec<Vec<f64>>,
}

impl Design {
    fn new(x: &[&[f64]], ry: &[bool]) -> Self {
        let row = |i: usize| {
            std::iter::once(1.0)
                .chain(x.iter().map(|c| c[i]))
                .collect::<Vec<f64>>()
        };
        Design {
            obs: (0..ry.len()).filter(|&i| ry[i]).map(row).collect(),
            mis: (0..ry.len()).filter(|&i| !ry[i]).map(row).collect(),
        }
    }
}

fn obs_values(values: &[f64], ry: &[bool]) -> Vec<f64> {
    values
        .iter()
        .zip(ry)
        .filter(|(_, &o)| o)
        .map(|(v, _)| *v)
        .collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// Sample variance (n - 1); 0 for fewer than two values.
fn variance(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return 0.0;
    }
    let m = mean(v);
    v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64
}

fn correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() < 2 || a.len() != b.len() {
        return None;
    }
    let (ma, mb) = (mean(a), mean(b));
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma).powi(2);
        sbb += (y - mb).powi(2);
    }
    let r = sab / (saa * sbb).sqrt();
    r.is_finite().then_some(r)
}

struct LeastSquares {
    /// Coefficients; aliased columns get 0 (mice's rank adjustment).
    coef: Vec<f64>,
    residuals: Vec<f64>,
    full_rank: bool,
}

/// `lm.fit`-style least squares: Gram-Schmidt QR in column order, treating a
/// column as aliased when its remaining norm is below 1e-7 of its original.
fn least_squares(x: &[Vec<f64>], y: &[f64]) -> LeastSquares {
    let n = x.len();
    let p = x.first().map(|r| r.len()).unwrap_or(0);
    let mut q: Vec<Vec<f64>> = Vec::new();
    let mut kept: Vec<usize> = Vec::new();
    let mut r = vec![vec![0.0; p]; p];
    for j in 0..p {
        let mut v: Vec<f64> = (0..n).map(|i| x[i][j]).collect();
        let norm0 = v.iter().map(|a| a * a).sum::<f64>().sqrt();
        for (k, qk) in q.iter().enumerate() {
            let proj = dot(qk, &v);
            r[k][j] = proj;
            for i in 0..n {
                v[i] -= proj * qk[i];
            }
        }
        let norm = v.iter().map(|a| a * a).sum::<f64>().sqrt();
        if norm0 > 0.0 && norm > 1e-7 * norm0 {
            r[q.len()][j] = norm;
            q.push(v.iter().map(|a| a / norm).collect());
            kept.push(j);
        }
    }
    let qty: Vec<f64> = q.iter().map(|qk| dot(qk, y)).collect();
    let k = kept.len();
    let mut c = vec![0.0; k];
    for a in (0..k).rev() {
        let mut s = qty[a];
        for b in a + 1..k {
            s -= r[a][kept[b]] * c[b];
        }
        c[a] = s / r[a][kept[a]];
    }
    let mut coef = vec![0.0; p];
    for (a, &j) in kept.iter().enumerate() {
        coef[j] = c[a];
    }
    let residuals = (0..n).map(|i| y[i] - dot(&x[i], &coef)).collect();
    LeastSquares {
        coef,
        residuals,
        full_rank: k == p,
    }
}

fn crossprod(x: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let p = x.first().map(|r| r.len()).unwrap_or(0);
    let mut out = vec![vec![0.0; p]; p];
    for row in x {
        for a in 0..p {
            for b in 0..p {
                out[a][b] += row[a] * row[b];
            }
        }
    }
    out
}

/// Gaussian elimination with partial pivoting; `None` when singular.
fn solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = a
        .iter()
        .zip(b)
        .map(|(row, &v)| row.iter().copied().chain([v]).collect())
        .collect();
    let scale = a
        .iter()
        .flatten()
        .fold(0.0_f64, |acc, v| acc.max(v.abs()))
        .max(1.0);
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))?;
        if m[pivot][col].abs() <= f64::EPSILON * scale * n as f64 {
            return None;
        }
        m.swap(col, pivot);
        for row in col + 1..n {
            let f = m[row][col] / m[col][col];
            if f != 0.0 {
                for k in col..=n {
                    m[row][k] -= f * m[col][k];
                }
            }
        }
    }
    let mut out = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| m[row][k] * out[k]).sum();
        out[row] = (m[row][n] - s) / m[row][row];
    }
    Some(out)
}

fn invert(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut cols = Vec::with_capacity(n);
    for j in 0..n {
        let e: Vec<f64> = (0..n).map(|i| if i == j { 1.0 } else { 0.0 }).collect();
        cols.push(solve(a, &e)?);
    }
    Some(
        (0..n)
            .map(|i| (0..n).map(|j| cols[j][i]).collect())
            .collect(),
    )
}

/// Lower-triangular Cholesky factor; `None` unless positive definite.
fn cholesky(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let s: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                let d = a[i][i] - s;
                if d <= 0.0 || !d.is_finite() {
                    return None;
                }
                l[i][j] = d.sqrt();
            } else {
                l[i][j] = (a[i][j] - s) / l[j][j];
            }
        }
    }
    Some(l)
}

/// Cyclic Jacobi eigen-decomposition of a small symmetric matrix: values in
/// decreasing order with matching column eigenvectors (`vectors[row][k]`).
fn symmetric_eigen(a: &[Vec<f64>]) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = a.len();
    let mut m = a.to_vec();
    let mut v: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    for _ in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| m[i][j] * m[i][j])
            .sum();
        if off < 1e-22 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if m[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (mkp, mkq) = (m[k][p], m[k][q]);
                    m[k][p] = c * mkp - s * mkq;
                    m[k][q] = s * mkp + c * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p][k], m[q][k]);
                    m[p][k] = c * mpk - s * mqk;
                    m[q][k] = s * mpk + c * mqk;
                }
                for row in v.iter_mut() {
                    let (vkp, vkq) = (row[p], row[q]);
                    row[p] = c * vkp - s * vkq;
                    row[q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| m[b][b].total_cmp(&m[a][a]));
    let values = order.iter().map(|&k| m[k][k]).collect();
    let vectors = (0..n)
        .map(|i| order.iter().map(|&k| v[i][k]).collect())
        .collect();
    (values, vectors)
}

/// Standard normal (Box-Muller).
fn normal(rng: &mut ChaCha12Rng) -> f64 {
    let mut u1 = rng.random::<f64>();
    while u1 <= f64::MIN_POSITIVE {
        u1 = rng.random::<f64>();
    }
    let u2 = rng.random::<f64>();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Gamma(shape, 1) by Marsaglia-Tsang (with the shape < 1 boost).
fn gamma(shape: f64, rng: &mut ChaCha12Rng) -> f64 {
    if shape < 1.0 {
        let u = rng.random::<f64>().max(f64::MIN_POSITIVE);
        return gamma(shape + 1.0, rng) * u.powf(1.0 / shape);
    }
    let d = shape - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let z = normal(rng);
        let v = (1.0 + c * z).powi(3);
        if v <= 0.0 {
            continue;
        }
        let u = rng.random::<f64>().max(f64::MIN_POSITIVE);
        if u.ln() < 0.5 * z * z + d - d * v + d * v.ln() {
            return d * v;
        }
    }
}

/// Chi-square with `df` degrees of freedom.
fn chisq(df: f64, rng: &mut ChaCha12Rng) -> f64 {
    2.0 * gamma(df / 2.0, rng)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;

    /// Three correlated columns with a few holes.
    fn matrix() -> ColumnMatrix {
        let n = 40;
        let a: Vec<f64> = (0..n)
            .map(|i| 10.0 + i as f64 * 0.5 + ((i * 7) % 5) as f64)
            .collect();
        let b: Vec<f64> = a
            .iter()
            .enumerate()
            .map(|(i, v)| 2.0 * v + ((i * 3) % 7) as f64)
            .collect();
        let c: Vec<f64> = a
            .iter()
            .enumerate()
            .map(|(i, v)| 30.0 - v * 0.3 + ((i * 11) % 4) as f64)
            .collect();
        let mut cols = vec![a, b, c];
        for &(j, i) in &[(0, 3), (0, 17), (1, 8), (1, 30), (2, 0), (2, 22), (2, 39)] {
            cols[j][i] = f64::NAN;
        }
        ColumnMatrix {
            names: vec!["a".into(), "b".into(), "c".into()],
            cols,
        }
    }

    #[test]
    fn every_method_fills_holes_with_observed_values_only() {
        let m = matrix();
        for method in [
            ImputeMethod::Pmm,
            ImputeMethod::MidasTouch,
            ImputeMethod::Rf,
        ] {
            let out = impute(&m, method, 20260914, MICE_MAXIT).expect("impute");
            assert_eq!(out.imputed_cells, 7, "{method:?}");
            assert!(out.excluded.is_empty());
            for (j, col) in out.matrix.cols.iter().enumerate() {
                let observed: Vec<f64> = m.cols[j]
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .collect();
                for (i, v) in col.iter().enumerate() {
                    assert!(v.is_finite(), "{method:?} left a hole at ({i},{j})");
                    if m.cols[j][i].is_finite() {
                        assert_eq!(*v, m.cols[j][i], "observed cells are untouched");
                    } else {
                        // All three methods impute donor values.
                        assert!(
                            observed.contains(v),
                            "{method:?} imputed a non-donor value {v}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn imputation_replays_with_its_seed() {
        let m = matrix();
        for method in [
            ImputeMethod::Pmm,
            ImputeMethod::MidasTouch,
            ImputeMethod::Rf,
        ] {
            let a = impute(&m, method, 7, MICE_MAXIT).expect("a");
            let b = impute(&m, method, 7, MICE_MAXIT).expect("b");
            assert_eq!(a, b, "{method:?} is deterministic for a seed");
        }
        let seeds: Vec<Vec<f64>> = (0..8)
            .map(|s| {
                impute(&m, ImputeMethod::Pmm, s, MICE_MAXIT)
                    .expect("pmm")
                    .matrix
                    .cols[2]
                    .clone()
            })
            .collect();
        assert!(
            seeds.windows(2).any(|w| w[0] != w[1]),
            "different seeds give different draws"
        );
    }

    #[test]
    fn constant_and_collinear_columns_are_left_alone() {
        let mut m = matrix();
        let n = m.n_rows();
        let mut constant = vec![5.0; n];
        constant[4] = f64::NAN;
        // mice drops the collinear column with fewer observed values, so the
        // twin gets more holes than column `a`.
        let mut twin: Vec<f64> = m.cols[0].iter().map(|v| v * 3.0 + 1.0).collect();
        for i in [10, 11, 12] {
            twin[i] = f64::NAN;
        }
        m.cols.push(constant);
        m.cols.push(twin);
        m.names.push("k".into());
        m.names.push("twin".into());
        let out = impute(&m, ImputeMethod::Pmm, 1, MICE_MAXIT).expect("impute");
        assert_eq!(out.excluded, vec!["k".to_string(), "twin".to_string()]);
        assert!(out.matrix.cols[3][4].is_nan());
        assert!(out.matrix.cols[4][10].is_nan());
        assert_eq!(out.imputed_cells, 7);
    }

    #[test]
    fn a_column_without_observations_is_an_error() {
        let mut m = matrix();
        m.cols[1] = vec![f64::NAN; m.n_rows()];
        let err = impute(&m, ImputeMethod::Pmm, 1, MICE_MAXIT).expect_err("no observations");
        assert!(err.to_string().contains("no observed values"), "{err}");
    }

    #[test]
    fn helpers_match_textbook_results() {
        // Least squares on y = 1 + 2x.
        let x: Vec<Vec<f64>> = (0..5).map(|i| vec![1.0, i as f64]).collect();
        let y: Vec<f64> = (0..5).map(|i| 1.0 + 2.0 * i as f64).collect();
        let fit = least_squares(&x, &y);
        assert!(fit.full_rank);
        assert!((fit.coef[0] - 1.0).abs() < 1e-12 && (fit.coef[1] - 2.0).abs() < 1e-12);
        // An aliased column gets a zero coefficient.
        let x2: Vec<Vec<f64>> = (0..5)
            .map(|i| vec![1.0, i as f64, 2.0 * i as f64])
            .collect();
        let fit2 = least_squares(&x2, &y);
        assert!(!fit2.full_rank);
        assert_eq!(fit2.coef[2], 0.0);
        // Eigenvalues of [[2,1],[1,2]] are 3 and 1.
        let (vals, _) = symmetric_eigen(&[vec![2.0, 1.0], vec![1.0, 2.0]]);
        assert!((vals[0] - 3.0).abs() < 1e-12 && (vals[1] - 1.0).abs() < 1e-12);
        // Chi-square mean is its degrees of freedom.
        let mut rng = ChaCha12Rng::seed_from_u64(3);
        let mean_chisq = (0..20_000).map(|_| chisq(7.0, &mut rng)).sum::<f64>() / 20_000.0;
        assert!((mean_chisq - 7.0).abs() < 0.15, "{mean_chisq}");
    }
}
