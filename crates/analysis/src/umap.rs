//! UMAP ordination: a faithful port of the R `umap` package (0.2.10.0)
//! `method = "naive"` pipeline (golden parity procedure 7, class D;
//! IMPLEMENTATION.md Sections 8.5 and 17.1 item 3).
//!
//! Ported R sources: `umap.naive`, `knn.info` (brute-force branch),
//! `smooth.knn.dist`, `naive.fuzzy.simplicial.set`, `concomp.coo`,
//! `laplacian.coo`, `spectral.eigenvectors`, `make.spectral.embedding`,
//! `make.initial.embedding`, `make.epochs.per.sample`, and the package's C
//! `optimize_embedding`/`optimize_epoch`/`clip4`, plus `find.ab.params`.
//!
//! Determinism note (the Section 17.1 spike decision): the oracle's only
//! random draws are the spectral-init jitter `rnorm(V * d, 0, 0.001)` and the
//! SGD negative samples `runif(n, 0, V)`. Those streams are deliberately not
//! replicated bit-for-bit (R Mersenne-Twister + inversion normals vs a seeded
//! ChaCha12 stream + Box-Muller), so layouts are compared distributionally
//! (10-NN overlap and pairwise-distance correlation against the three seeded
//! golden layouts), while every deterministic stage (k-nn graph,
//! `smooth.knn.dist` binary search, fuzzy-set weights, Laplacian spectrum,
//! `a`/`b` curve fit) is pinned against the oracle at full precision in the
//! parity test. The eigensolver runs under `Parallelism::Serial` so same-seed
//! runs are bit-identical across repeated calls.
//!
//! Scale ceiling: the naive method's brute-force neighbor search is O(V^2)
//! and the spectral init is a dense eigendecomposition; R switches to
//! `knn.from.data.reps` at 2048+ rows, which is not ported. Inputs of 2048+
//! rows are rejected with `umap_input_too_large` instead of silently
//! degrading (Section 12 interactive budget).

use std::collections::BTreeMap;

use archaeodash_domain::DomainError;
use faer::{Mat, Side};
use rand::RngExt;
use rand::SeedableRng;
use rand_chacha::ChaCha12Rng;

use crate::ColumnMatrix;

/// Resolved legacy `umap.defaults` configuration (golden capture 7 config).
#[derive(Debug, Clone, PartialEq)]
pub struct UmapConfig {
    /// `n_neighbors`: brute-force neighbor count per row (self included).
    pub n_neighbors: usize,
    /// `n_components`: embedding dimension.
    pub n_components: usize,
    /// `n_epochs`: SGD epochs.
    pub n_epochs: usize,
    /// `metric`: only `"euclidean"` (the legacy `mdEuclidean` C metric).
    pub metric: String,
    /// `init`: only `"spectral"` (with the legacy random fallback).
    pub init: String,
    /// `min_dist`.
    pub min_dist: f64,
    /// `spread`.
    pub spread: f64,
    /// `set_op_mix_ratio`.
    pub set_op_mix_ratio: f64,
    /// `local_connectivity`.
    pub local_connectivity: f64,
    /// `bandwidth`.
    pub bandwidth: f64,
    /// `alpha`: initial SGD learning rate.
    pub alpha: f64,
    /// `gamma`: negative-sample weight.
    pub gamma: f64,
    /// `negative_sample_rate`.
    pub negative_sample_rate: f64,
    /// Fitted `a` curve parameter (`find.ab.params(spread, min_dist)`).
    pub a: f64,
    /// Fitted `b` curve parameter.
    pub b: f64,
}

impl Default for UmapConfig {
    fn default() -> Self {
        let (a, b) = find_ab_params(1.0, 0.1);
        Self {
            n_neighbors: 15,
            n_components: 2,
            n_epochs: 200,
            metric: "euclidean".to_string(),
            init: "spectral".to_string(),
            min_dist: 0.1,
            spread: 1.0,
            set_op_mix_ratio: 1.0,
            local_connectivity: 1.0,
            bandwidth: 1.0,
            alpha: 1.0,
            gamma: 1.0,
            negative_sample_rate: 5.0,
            a,
            b,
        }
    }
}

/// UMAP parity result: the embedding row-major (`layout[i][c]` = row `i`,
/// dimension `c`), the resolved config echo, and non-fatal legacy warnings
/// (spectral-init fallback).
#[derive(Debug, Clone, PartialEq)]
pub struct Umap {
    /// Embedding, row-major: one `[V1, V2]` pair per input row, centered.
    pub layout: Vec<Vec<f64>>,
    /// Resolved configuration (fitted `a`/`b` included).
    pub config: UmapConfig,
    /// Non-fatal legacy warnings (spectral-init fallback).
    pub warnings: Vec<String>,
}

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

/// Euclidean distance between two rows, summed in column order (the legacy
/// `mdEuclidean` C routine).
fn euclidean(a: &[f64], b: &[f64]) -> f64 {
    let mut sum = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        let d = x - y;
        sum += d * d;
    }
    sum.sqrt()
}

/// Brute-force k-nn graph, the legacy `knn.info` branch for `nrow(d) < 2048`:
/// a full symmetric distance matrix with the diagonal treated as `-1`, each
/// row stably ordered by distance (ties by column index, like R's `order`),
/// the first `k` entries kept, and the self distance overwritten with `0`.
pub fn knn_brute_force(data: &[Vec<f64>], k: usize) -> (Vec<Vec<usize>>, Vec<Vec<f64>>) {
    let v = data.len();
    let mut dist = vec![vec![0.0f64; v]; v];
    for i in 0..v {
        for j in (i + 1)..v {
            let d = euclidean(&data[i], &data[j]);
            dist[i][j] = d;
            dist[j][i] = d;
        }
    }
    let mut indexes = vec![vec![0usize; k]; v];
    let mut distances = vec![vec![0.0f64; k]; v];
    for i in 0..v {
        let mut order: Vec<usize> = (0..v).collect();
        order.sort_by(|&x, &y| {
            let dx = if x == i { -1.0 } else { dist[i][x] };
            let dy = if y == i { -1.0 } else { dist[i][y] };
            dx.partial_cmp(&dy).unwrap_or(std::cmp::Ordering::Equal)
        });
        for (j, &idx) in order.iter().enumerate().take(k) {
            indexes[i][j] = idx;
            distances[i][j] = if idx == i { 0.0 } else { dist[i][idx] };
        }
    }
    (indexes, distances)
}

/// Legacy `smooth.knn.dist`: per-row binary search for the bandwidth-scaled
/// soft-k kernel width `sigma` and the nearest-neighbor offset `rho`.
/// Returns `(sigma, rho)` rows.
pub fn smooth_knn_dist(
    distances: &[Vec<f64>],
    n_neighbors: usize,
    local_connectivity: f64,
    bandwidth: f64,
) -> (Vec<f64>, Vec<f64>) {
    let v = distances.len();
    let target = (n_neighbors as f64).log2() * bandwidth;
    let total: f64 = distances.iter().map(|row| row.iter().sum::<f64>()).sum();
    let k_dist_mean = total / (v * n_neighbors) as f64;
    let local_int = local_connectivity.floor() as usize;
    let interpolation = local_connectivity - local_int as f64;
    let mut sigmas = vec![0.0f64; v];
    let mut rhos = vec![0.0f64; v];
    for (i, row) in distances.iter().enumerate() {
        let nonzero: Vec<f64> = row.iter().copied().filter(|d| *d != 0.0).collect();
        let num_nonzero = nonzero.len() as f64;
        if num_nonzero > local_connectivity {
            if local_int > 0 {
                rhos[i] = nonzero[local_int - 1]
                    + interpolation * (nonzero[local_int] - nonzero[local_int - 1]);
            } else {
                rhos[i] = interpolation * nonzero[0];
            }
        } else if num_nonzero > 0.0 && num_nonzero < local_connectivity {
            rhos[i] = nonzero.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        }
        let mut lo = 0.0f64;
        let mut hi = f64::INFINITY;
        let mut mid = 1.0f64;
        // R: for (n in seq(2, iterations)) with iterations = 64 -> 63 passes.
        for _ in 0..63 {
            let val: f64 = row[1..]
                .iter()
                .map(|d| (-((d - rhos[i]).max(0.0)) / mid).exp())
                .sum();
            if (val - target).abs() < 1e-5 {
                break;
            }
            if val > target {
                hi = mid;
                mid = (lo + hi) / 2.0;
            } else {
                lo = mid;
                mid = if hi.is_finite() {
                    (lo + hi) / 2.0
                } else {
                    mid * 2.0
                };
            }
        }
        sigmas[i] = mid;
        let row_mean = row.iter().sum::<f64>() / row.len() as f64;
        let floor = if rhos[i] > 0.0 {
            0.001 * row_mean
        } else {
            0.001 * k_dist_mean
        };
        sigmas[i] = sigmas[i].max(floor);
    }
    (sigmas, rhos)
}

/// Legacy `naive.fuzzy.simplicial.set` with the `set_op_mix_ratio` algebra:
/// `mix * (a + b - a*b) + (1 - mix) * a*b` over the directed-weight union
/// (a missing side contributes zero, like the `merge(all = TRUE)` zero fill),
/// reduced by dropping zero and non-finite weights. Returns edges sorted by
/// `(from, to)` (0-based), matching R's `merge` key ordering.
pub fn fuzzy_simplicial_set(
    indexes: &[Vec<usize>],
    distances: &[Vec<f64>],
    set_op_mix_ratio: f64,
    local_connectivity: f64,
    bandwidth: f64,
) -> Vec<(usize, usize, f64)> {
    let v = indexes.len();
    let k = indexes.first().map_or(0, |row| row.len());
    let (sigmas, rhos) = smooth_knn_dist(distances, k, local_connectivity, bandwidth);
    let mut coo: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for i in 0..v {
        for j in 0..k {
            let idx = indexes[i][j];
            let d = distances[i][j];
            let val = if idx == i {
                0.0
            } else if d - rhos[i] <= 0.0 {
                1.0
            } else {
                (-(d - rhos[i]) / (sigmas[i] * bandwidth)).exp()
            };
            coo.insert((i, idx), val);
        }
    }
    // Union of the directed weights with their transpose (R's
    // `merge(all = TRUE)` zero fill), then the set-op algebra.
    let mut union: BTreeMap<(usize, usize), (f64, f64)> = BTreeMap::new();
    for (&(f, t), &av) in &coo {
        union.entry((f, t)).or_insert((0.0, 0.0)).0 = av;
    }
    for (&(f, t), &av) in &coo {
        union.entry((t, f)).or_insert((0.0, 0.0)).1 = av;
    }
    let mut edges: Vec<(usize, usize, f64)> = Vec::with_capacity(union.len());
    for (&(f, t), &(av, bv)) in &union {
        let prod = av * bv;
        let value = set_op_mix_ratio * (av + bv - prod) + (1.0 - set_op_mix_ratio) * prod;
        if value != 0.0 && value.is_finite() {
            edges.push((f, t, value));
        }
    }
    edges
}

/// Legacy `concomp.coo`: connected components over the reduced edge list,
/// first-touch labeled frontier walk (including the legacy re-labeling quirk
/// for already-visited frontier members). Returns `(n_components, labels)`.
pub fn connected_components(edges: &[(usize, usize, f64)], v: usize) -> (usize, Vec<usize>) {
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); v];
    for &(f, t, _) in edges {
        neighbors[f].push(t);
    }
    let mut visited = vec![false; v];
    let mut components = vec![usize::MAX; v];
    let mut count = 0usize;
    let mut tovisit: Vec<usize> = vec![0];
    while !tovisit.is_empty() {
        let unvisited: Vec<usize> = tovisit.iter().copied().filter(|&t| !visited[t]).collect();
        if !unvisited.is_empty() {
            for &t in &tovisit {
                visited[t] = true;
                components[t] = count;
            }
            let mut next: Vec<usize> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for &t in &tovisit {
                for &n in &neighbors[t] {
                    if seen.insert(n) {
                        next.push(n);
                    }
                }
            }
            tovisit = next;
        }
        if unvisited.is_empty() || tovisit.is_empty() {
            count += 1;
            tovisit = (0..v).filter(|&i| !visited[i]).take(1).collect();
        }
    }
    (count, components)
}

/// Legacy `laplacian.coo` dense form `I - D^{-1/2} W D^{-1/2}`; `None`
/// mirrors the legacy "singular degrees" error (a row with no outgoing edge).
fn laplacian_dense(edges: &[(usize, usize, f64)], v: usize) -> Option<Mat<f64>> {
    let mut deg = vec![0.0f64; v];
    for &(f, _, w) in edges {
        deg[f] += w;
    }
    let mut seen = vec![false; v];
    for &(f, _, _) in edges {
        seen[f] = true;
    }
    if seen.iter().filter(|&&s| s).count() != v {
        return None;
    }
    let mut l = Mat::zeros(v, v);
    for i in 0..v {
        l[(i, i)] = 1.0;
    }
    for &(f, t, w) in edges {
        if f != t {
            l[(f, t)] = -w / (deg[f] * deg[t]).sqrt();
        }
    }
    Some(l)
}

/// The `k` smallest-|eigenvalue| eigenpairs of a symmetric matrix, sorted by
/// ascending magnitude. (RSpectra's `eigs(which = "SM")`, which the oracle
/// uses, returns the same selection in DESCENDING modulus order; callers
/// that mirror R's column slicing must reorder accordingly.)
/// Runs under `Parallelism::Serial` for run-to-run bit determinism.
fn smallest_eigenpairs(l: &Mat<f64>, k: usize) -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
    let previous = faer::get_global_parallelism();
    faer::set_global_parallelism(faer::Par::Seq);
    let eigen = faer::linalg::solvers::SelfAdjointEigen::new(l.as_ref(), Side::Lower);
    faer::set_global_parallelism(previous);
    let eigen = eigen.ok()?;
    let values: Vec<f64> = eigen.S().column_vector().iter().copied().collect();
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&x, &y| values[x].abs().total_cmp(&values[y].abs()));
    let picked: Vec<usize> = order.into_iter().take(k).collect();
    let picked_values: Vec<f64> = picked.iter().map(|&i| values[i]).collect();
    let v = l.nrows();
    let mut vectors = vec![vec![0.0f64; picked.len()]; v];
    for (c, &eig_idx) in picked.iter().enumerate() {
        for (i, row) in vectors.iter_mut().enumerate() {
            row[c] = *eigen.U().get(i, eig_idx);
        }
    }
    Some((picked_values, vectors))
}

/// Test-visible wrapper: the `k` smallest-|eigenvalue| eigenvalues of the
/// normalized Laplacian, or `None` on the legacy failure paths.
pub fn laplacian_smallest_eigenvalues(
    edges: &[(usize, usize, f64)],
    v: usize,
    k: usize,
) -> Option<Vec<f64>> {
    let l = laplacian_dense(edges, v)?;
    smallest_eigenpairs(&l, k).map(|(values, _)| values)
}

/// R's `quantile(x, p)` default type 7.
fn quantile_type7(mut values: Vec<f64>, p: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    let h = (n as f64 - 1.0) * p + 1.0;
    let lo = h.floor();
    let frac = h - lo;
    let lo = lo as usize;
    if lo >= n {
        values[n - 1]
    } else if frac == 0.0 {
        values[lo - 1]
    } else {
        values[lo - 1] + frac * (values[lo] - values[lo - 1])
    }
}

fn center_columns(layout: &mut [Vec<f64>]) {
    if layout.is_empty() {
        return;
    }
    let v = layout.len();
    let d = layout[0].len();
    for c in 0..d {
        let mean = layout.iter().map(|row| row[c]).sum::<f64>() / v as f64;
        for row in layout.iter_mut() {
            row[c] -= mean;
        }
    }
}

/// Legacy `make.random.embedding(d, V)`: uniform `[-10, 10)`, column-major.
fn random_embedding(d: usize, v: usize, rng: &mut ChaCha12Rng) -> Vec<Vec<f64>> {
    let mut layout = vec![vec![0.0f64; d]; v];
    for c in 0..d {
        for row in layout.iter_mut().take(v) {
            row[c] = rng.random_range(-10.0..10.0);
        }
    }
    layout
}

/// Box-Muller standard normals from the seeded stream (stands in for R's
/// inversion-method `rnorm`; distributionally identical, class-D parity).
fn normal_noise(n: usize, rng: &mut ChaCha12Rng) -> Vec<f64> {
    let mut values = Vec::with_capacity(n);
    for _ in 0..n.div_ceil(2) {
        let mut u1 = rng.random::<f64>();
        if u1 <= 0.0 {
            u1 = f64::MIN_POSITIVE;
        }
        let u2 = rng.random::<f64>();
        let r = (-2.0 * u1.ln()).sqrt();
        values.push(r * (std::f64::consts::TAU * u2).cos());
        values.push(r * (std::f64::consts::TAU * u2).sin());
    }
    values.truncate(n);
    values
}

/// Legacy `one.embedding`: spectral init (or the random fallback with the
/// legacy warning), column-centered, scaled by
/// `10 / (quantile(col1, 0.99) - quantile(col1, 0.01))`. The legacy code
/// would silently scale by `Inf` on a degenerate column-1 range; here a
/// non-finite expansion keeps the centered embedding and records the warning.
fn one_embedding(
    v: usize,
    d: usize,
    edges: &[(usize, usize, f64)],
    rng: &mut ChaCha12Rng,
    warnings: &mut Vec<String>,
) -> Vec<Vec<f64>> {
    let mut embedding = match laplacian_dense(edges, v).and_then(|l| smallest_eigenpairs(&l, d + 1))
    {
        Some((_, vectors)) => {
            // R keeps `[, seq_len(d)]` of RSpectra `eigs(k = d + 1, which =
            // "SM")` output. RSpectra returns those d+1 smallest-modulus
            // eigenpairs in DESCENDING modulus order (probed: 0.00874,
            // 0.00357, ~0), so the kept d columns are the d largest-modulus
            // of the selection with the trivial near-null eigenvector
            // dropped first. `smallest_eigenpairs` returns ascending order,
            // so map column c to the (d - c)-th ascending column.
            let mut kept = vec![vec![0.0f64; d]; v];
            for (i, row) in vectors.iter().enumerate() {
                for (c, slot) in kept[i].iter_mut().enumerate() {
                    *slot = row[d - c];
                }
            }
            kept
        }
        None => {
            warnings.push(
                "failed creating initial embedding; using random embedding instead".to_string(),
            );
            random_embedding(d, v, rng)
        }
    };
    center_columns(&mut embedding);
    let col0: Vec<f64> = embedding.iter().map(|row| row[0]).collect();
    let q01 = quantile_type7(col0.clone(), 0.01);
    let q99 = quantile_type7(col0.clone(), 0.99);
    let expansion = 10.0 / (q99 - q01);
    if expansion.is_finite() {
        for row in embedding.iter_mut() {
            for value in row.iter_mut() {
                *value *= expansion;
            }
        }
    }
    embedding
}

/// Legacy `make.spectral.embedding` (via `make.initial.embedding` with
/// `init = "spectral"`): per-component scaled spectral placement on the
/// 20-spaced offset grid for disconnected graphs, then the `rnorm(V * d, 0,
/// 0.001)` jitter in R's column-major fill order.
fn initial_embedding(
    v: usize,
    d: usize,
    edges: &[(usize, usize, f64)],
    rng: &mut ChaCha12Rng,
) -> (Vec<Vec<f64>>, Vec<String>) {
    let mut warnings = Vec::new();
    let (n_components, components) = connected_components(edges, v);
    let mut layout;
    if n_components <= 1 {
        layout = one_embedding(v, d, edges, rng, &mut warnings);
    } else {
        // Components sorted by size descending, stable by ascending label
        // (R's `sort(table(components), decreasing = TRUE)`).
        let mut counts: Vec<(usize, usize)> = (0..n_components).map(|label| (label, 0)).collect();
        for &c in &components {
            counts[c].1 += 1;
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let m = (n_components as f64).sqrt().ceil() as usize;
        let seq: Vec<usize> = (0..=m).collect();
        // expand.grid varies the first dimension fastest.
        let mut grid: Vec<Vec<usize>> = vec![vec![0usize; d]];
        for dim in 0..d {
            let mut next = Vec::with_capacity(grid.len() * seq.len());
            for &offset in &seq {
                for row in &grid {
                    let mut row = row.clone();
                    row[dim] = offset;
                    next.push(row);
                }
            }
            grid = next;
        }
        grid.sort_by(|a, b| {
            let sa: usize = a.iter().sum();
            let sb: usize = b.iter().sum();
            sa.cmp(&sb).then(a.iter().max().cmp(&b.iter().max()))
        });
        if grid.len() < n_components {
            return (random_embedding(d, v, rng), warnings);
        }
        layout = vec![vec![0.0f64; d]; v];
        for (ci, &(label, _)) in counts.iter().enumerate() {
            let members: Vec<usize> = (0..v).filter(|&i| components[i] == label).collect();
            let reindex: std::collections::HashMap<usize, usize> = members
                .iter()
                .enumerate()
                .map(|(new, &old)| (old, new))
                .collect();
            let sub: Vec<(usize, usize, f64)> = edges
                .iter()
                .filter(|&&(f, t, _)| reindex.contains_key(&f) && reindex.contains_key(&t))
                .map(|&(f, t, w)| (reindex[&f], reindex[&t], w))
                .collect();
            let mut emb = one_embedding(members.len(), d, &sub, rng, &mut warnings);
            center_columns(&mut emb);
            let col0: Vec<f64> = emb.iter().map(|row| row[0]).collect();
            let q01 = quantile_type7(col0.clone(), 0.01);
            let q99 = quantile_type7(col0.clone(), 0.99);
            let expansion = 10.0 / (q99 - q01);
            if expansion.is_finite() {
                for row in emb.iter_mut() {
                    for value in row.iter_mut() {
                        *value *= expansion;
                    }
                }
            }
            for (local_i, &global_i) in members.iter().enumerate() {
                for c in 0..d {
                    layout[global_i][c] = emb[local_i][c] + 20.0 * grid[ci][c] as f64;
                }
            }
        }
    }
    let noise = normal_noise(v * d, rng);
    for c in 0..d {
        for (i, row) in layout.iter_mut().enumerate() {
            row[c] += noise[c * v + i] * 0.001;
        }
    }
    (layout, warnings)
}

/// Legacy `make.epochs.per.sample`: `epochs / (epochs * w / max(w))` for
/// positive samples, `-1` otherwise.
pub fn epochs_per_sample(weights: &[f64], n_epochs: usize) -> Vec<f64> {
    let max_w = weights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    weights
        .iter()
        .map(|&w| {
            let n_samples = n_epochs as f64 * (w / max_w);
            if n_samples > 0.0 {
                n_epochs as f64 / n_samples
            } else {
                -1.0
            }
        })
        .collect()
}

/// Legacy C `clip4`: multiply by `inner`, clamp to `[-4, 4]`, multiply by
/// `outer`.
fn clip4(x: f64, inner: f64, outer: f64) -> f64 {
    // Clamping semantics match the legacy C `clip4` (NaN passes through
    // unchanged, as with the if/else chain).
    (x * inner).clamp(-4.0, 4.0) * outer
}

/// Legacy C `optimize_embedding`: SGD over the epoch-sampled edge list with
/// the exact R draw order (one `runif(n_neg, 0, V)` call per adjusted edge
/// per epoch, from the seeded stream).
fn optimize_embedding(
    layout: &mut [Vec<f64>],
    pairs: &[(usize, usize)],
    eps: &[f64],
    config: &UmapConfig,
    rng: &mut ChaCha12Rng,
) {
    let n = eps.len();
    if n == 0 {
        return;
    }
    let v = layout.len() as f64;
    let a = config.a;
    let b = config.b;
    let bm1 = b - 1.0;
    let m2ab = -2.0 * a * b;
    let p2gb = 2.0 * config.gamma * b;
    // R: fix.observations = min(from, 1-based) > 1; move_other = !(that > 0).
    let move_other = !pairs.iter().map(|&(f, _)| f).min().is_some_and(|f| f > 0);
    let mut eons: Vec<f64> = eps.to_vec();
    let epns: Vec<f64> = eps
        .iter()
        .map(|e| e / config.negative_sample_rate)
        .collect();
    let mut eon2s = epns.clone();
    let mut adjust = vec![false; n];
    let mut nns = vec![0.0f64; n];
    for epoch in 0..config.n_epochs {
        let alpha = config.alpha * (1.0 - epoch as f64 / config.n_epochs as f64);
        let np1 = (epoch + 1) as f64;
        for i in 0..n {
            adjust[i] = eons[i] <= np1;
            if adjust[i] {
                nns[i] = ((1.0 + epoch as f64 - eon2s[i]) / epns[i]).floor();
            }
        }
        for i in 0..n {
            if !adjust[i] {
                continue;
            }
            let (j, k) = pairs[i];
            // Primary link attraction.
            let d = layout[j].len();
            let mut codiff = vec![0.0f64; d];
            for (c, slot) in codiff.iter_mut().enumerate() {
                *slot = layout[j][c] - layout[k][c];
            }
            let codist2: f64 = codiff.iter().map(|x| x * x).sum();
            let gradcoeff = m2ab * codist2.powf(bm1) / (a * codist2.powf(b) + 1.0);
            for (c, diff) in codiff.iter().enumerate() {
                let gradd = clip4(*diff, gradcoeff, alpha);
                layout[j][c] += gradd;
                if move_other {
                    layout[k][c] -= gradd;
                }
            }
            // Negative samples: `runif(n_neg, 0, V)` per adjusted edge.
            let n_neg = nns[i].max(0.0) as usize;
            for _ in 0..n_neg {
                let k2 = rng.random_range(0.0..v).floor() as usize;
                let mut codiff = vec![0.0f64; layout[j].len()];
                for (c, slot) in codiff.iter_mut().enumerate() {
                    *slot = layout[j][c] - layout[k2][c];
                }
                let codist2: f64 = codiff.iter().map(|x| x * x).sum();
                let gradcoeff = p2gb / ((0.001 + codist2) * (a * codist2.powf(b) + 1.0));
                for (c, diff) in codiff.iter().enumerate() {
                    let gradd = clip4(*diff, gradcoeff, alpha);
                    layout[j][c] += gradd;
                }
            }
        }
        for i in 0..n {
            if adjust[i] {
                eons[i] += eps[i];
                eon2s[i] += nns[i] * epns[i];
            }
        }
    }
}

/// Legacy `find.ab.params`: recursive 10x10 grid search fitting the
/// `1 / (1 + a * x^(2b))` curve to the min_dist/spread piecewise target.
pub fn find_ab_params(spread: f64, min_dist: f64) -> (f64, f64) {
    let n = 300;
    let xv: Vec<f64> = (0..n)
        .map(|i| spread * 3.0 * i as f64 / (n as f64 - 1.0))
        .collect();
    let yv: Vec<f64> = xv
        .iter()
        .map(|&x| {
            if x < min_dist {
                1.0
            } else if x > min_dist {
                ((min_dist - x) / spread).exp()
            } else {
                0.0
            }
        })
        .collect();
    fn recursive(xv: &[f64], yv: &[f64], alim: (f64, f64), blim: (f64, f64)) -> (f64, f64) {
        let grid = |lo: f64, hi: f64| -> Vec<f64> {
            let by = (hi - lo) / 9.0;
            (0..10).map(|i| lo + i as f64 * by).collect()
        };
        let avals = grid(alim.0, alim.1);
        let bvals = grid(blim.0, blim.1);
        let mut best = (avals[0], bvals[0]);
        let mut best_err = f64::INFINITY;
        // expand.grid varies the first factor fastest: b outer, a inner.
        for &b in &bvals {
            for &a in &avals {
                let err: f64 = xv
                    .iter()
                    .zip(yv.iter())
                    .map(|(&x, &y)| {
                        let diff = 1.0 / (1.0 + a * x.powf(2.0 * b)) - y;
                        diff * diff
                    })
                    .sum();
                if err < best_err {
                    best_err = err;
                    best = (a, b);
                }
            }
        }
        let mid = ((alim.0 + alim.1) / 2.0, (blim.0 + blim.1) / 2.0);
        if (best.0 - mid.0).abs() + (best.1 - mid.1).abs() > 1e-8 {
            let da = avals[1] - avals[0];
            let db = bvals[1] - bvals[0];
            recursive(
                xv,
                yv,
                (best.0 - 1.5 * da, best.0 + 1.5 * da),
                (best.1 - 1.5 * db, best.1 + 1.5 * db),
            )
        } else {
            best
        }
    }
    recursive(&xv, &yv, (0.0, 20.0), (0.0, 20.0))
}

/// Runs UMAP (legacy `umap::umap(features, method = "naive")` with default
/// configuration and the caller's seed) over a numeric column matrix.
/// `seed` replaces the legacy `set.seed(...)` global stream with a seeded
/// ChaCha12 stream (class-D parity; see the module docs).
///
/// # Errors
/// Rejects empty or non-finite inputs, `n_neighbors` outside `2..n`, and
/// inputs at or above the 2048-row brute-force ceiling.
/// Default deterministic seed replacing the legacy unseeded `set.seed`
/// stream; the class-D golden fixtures pin seeds 20260914-20260916.
pub const DEFAULT_SEED: u64 = 20260914;

pub fn umap(m: &ColumnMatrix, seed: u64) -> Result<Umap, DomainError> {
    let config = UmapConfig::default();
    let v = m.cols.first().map_or(0, |col| col.len());
    let p = m.cols.len();
    if v == 0 || p == 0 {
        return Err(validation(
            "ordination_empty",
            "UMAP requires at least one row and one column",
        ));
    }
    for (j, name) in m.names.iter().enumerate() {
        if m.cols[j].iter().any(|value| !value.is_finite()) {
            return Err(validation(
                "ordination_missing_values",
                format!("UMAP requires complete cases: column {name:?} has NA values"),
            ));
        }
    }
    let k = config.n_neighbors;
    if k < 2 {
        return Err(validation(
            "umap_neighbors",
            "number of neighbors must be greater than 1",
        ));
    }
    if k >= v {
        return Err(validation(
            "umap_neighbors",
            "number of neighbors must be smaller than number of items",
        ));
    }
    if v >= 2048 {
        return Err(validation(
            "umap_input_too_large",
            "UMAP brute-force neighbor search covers fewer than 2048 rows",
        ));
    }
    let data: Vec<Vec<f64>> = (0..v)
        .map(|i| m.cols.iter().map(|col| col[i]).collect())
        .collect();
    let (indexes, distances) = knn_brute_force(&data, k);
    let graph = fuzzy_simplicial_set(
        &indexes,
        &distances,
        config.set_op_mix_ratio,
        config.local_connectivity,
        config.bandwidth,
    );
    let mut rng = ChaCha12Rng::seed_from_u64(seed);
    let (mut layout, warnings) = initial_embedding(v, config.n_components, &graph, &mut rng);
    if config.n_epochs > 0 {
        let gmax = graph
            .iter()
            .map(|&(_, _, w)| w)
            .fold(f64::NEG_INFINITY, f64::max);
        let kept: Vec<(usize, usize, f64)> = graph
            .iter()
            .copied()
            .filter(|&(_, _, w)| w >= gmax / config.n_epochs as f64)
            .collect();
        let weights: Vec<f64> = kept.iter().map(|&(_, _, w)| w).collect();
        let eps = epochs_per_sample(&weights, config.n_epochs);
        let pairs: Vec<(usize, usize)> = kept.iter().map(|&(f, t, _)| (f, t)).collect();
        optimize_embedding(&mut layout, &pairs, &eps, &config, &mut rng);
    }
    center_columns(&mut layout);
    Ok(Umap {
        layout,
        config,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ab_params_match_r_oracle() {
        let (a, b) = find_ab_params(1.0, 0.1);
        assert!((a - 1.576_943_612_694_566_4).abs() < 1e-6, "a = {a}");
        assert!((b - 0.895_060_718_151_928_1).abs() < 1e-6, "b = {b}");
    }

    #[test]
    fn epochs_per_sample_matches_r_formula() {
        let eps = epochs_per_sample(&[1.0, 0.5, 0.004, 0.0], 200);
        assert!((eps[0] - 1.0).abs() < 1e-12);
        assert!((eps[1] - 2.0).abs() < 1e-12);
        assert!((eps[2] - 250.0).abs() < 1e-12);
        assert!(eps[3] < 0.0);
    }

    #[test]
    fn quantile_type7_matches_r() {
        // R: quantile(1:10, c(0.01, 0.5, 0.99)) = 1.09, 5.5, 9.91.
        let values: Vec<f64> = (1..=10).map(f64::from).collect();
        assert!((quantile_type7(values.clone(), 0.01) - 1.09).abs() < 1e-12);
        assert!((quantile_type7(values.clone(), 0.5) - 5.5).abs() < 1e-12);
        assert!((quantile_type7(values, 0.99) - 9.91).abs() < 1e-12);
    }

    #[test]
    fn fuzzy_set_union_algebra() {
        // Two rows, symmetric weights: union = a + b - a*b at mix ratio 1.
        let indexes = vec![vec![0, 1], vec![1, 0]];
        let distances = vec![vec![0.0, 1.0], vec![0.0, 1.0]];
        let edges = fuzzy_simplicial_set(&indexes, &distances, 1.0, 1.0, 1.0);
        // rho = 0 for both rows (single nonzero distance with
        // local_connectivity 1 leaves rho at 0), so the binary search grows
        // sigma until sum(exp(-d/sigma)) converges to log2(2) = 1; the union
        // weight is a + b - a*b for the symmetric pair, self edges dropped.
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0], (0, 1, edges[0].2));
        assert_eq!(edges[1], (1, 0, edges[1].2));
        assert!(
            (edges[0].2 - 0.999_999_999_941_792_8).abs() < 1e-15,
            "edge weight {}",
            edges[0].2
        );
        assert!((edges[0].2 - edges[1].2).abs() < 1e-18);
    }

    #[test]
    fn debug_spectral_init_probe() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures/INAA_test.csv");
        let frame = archaeodash_data_io::data_loader(&fixture).expect("load");
        let names = ["as", "la", "lu", "nd", "sm", "u", "yb", "ce"];
        let cols: Vec<Vec<f64>> = names
            .iter()
            .map(|n| archaeodash_data_io::loader::numeric_column(&frame, n).expect("col"))
            .collect();
        let v = cols[0].len();
        let data: Vec<Vec<f64>> = (0..v)
            .map(|i| cols.iter().map(|col| col[i]).collect())
            .collect();
        let (indexes, distances) = knn_brute_force(&data, 15);
        let edges = fuzzy_simplicial_set(&indexes, &distances, 1.0, 1.0, 1.0);
        let mut warnings = Vec::new();
        let n_components = connected_components(&edges, v).0;
        println!("n_components = {n_components}");
        let layout = if n_components <= 1 {
            // spectral only, pre-noise: replicate one_embedding without RNG
            one_embedding_no_noise(v, 2, &edges, &mut warnings)
        } else {
            panic!("multi-component");
        };
        let col0: Vec<f64> = layout.iter().map(|r| r[0]).collect();
        let q01 = quantile_type7(col0.clone(), 0.01);
        let q99 = quantile_type7(col0.clone(), 0.99);
        println!("q01 = {q01:.10}  q99 = {q99:.10}");
        for r in layout.iter().take(3) {
            println!("{:.7} {:.7}", r[0], r[1]);
        }
        let _ = warnings;
    }

    fn one_embedding_no_noise(
        v: usize,
        d: usize,
        edges: &[(usize, usize, f64)],
        warnings: &mut Vec<String>,
    ) -> Vec<Vec<f64>> {
        let l = laplacian_dense(edges, v).expect("laplacian");
        let (_, vectors) = smallest_eigenpairs(&l, d + 1).expect("eigen");
        let mut embedding = vectors;
        center_columns(&mut embedding);
        let col0: Vec<f64> = embedding.iter().map(|row| row[0]).collect();
        let q01 = quantile_type7(col0.clone(), 0.01);
        let q99 = quantile_type7(col0, 0.99);
        let expansion = 10.0 / (q99 - q01);
        println!("expansion = {expansion:.10}");
        for row in embedding.iter_mut() {
            for value in row.iter_mut() {
                *value *= expansion;
            }
        }
        let _ = warnings;
        embedding
    }

    #[test]
    fn validation_rejects_bad_neighbors() {
        let matrix = ColumnMatrix {
            names: vec!["a".into(), "b".into()],
            cols: vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]],
        };
        let err = umap(&matrix, 1).expect_err("k >= n");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "umap_neighbors"
        ));
    }
}
