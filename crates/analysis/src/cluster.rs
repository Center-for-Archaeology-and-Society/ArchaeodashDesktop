//! Clustering with exact R `stats`/`cluster` semantics (golden parity
//! procedure 9, class T; IMPLEMENTATION.md Section 8): `kmeans`
//! (Hartigan-Wong AS-136 with R's seeded `sample.int` nstart draws), `pam`
//! (Kaufman-Rousseeuw build + original full-scan swap + `cstat` numbering),
//! `hclust` `ward.D2` (NN-chain agglomeration with `hcass2` merge/order
//! coding), `diana` (DIANA splitting with banner + merge derivation), and the
//! mean silhouette width (`cluster::silhouette` via `sildist`).
//!
//! Distances are plain Euclidean (`stats::dist`, no NA handling), and the
//! `RMt19937` stream reproduces R's `set.seed`/`sample.int` bit-for-bit so
//! the golden fixture's nstart draws are replayable.
//!
//! The ports keep the Fortran/C index loops (`kmns.f`, `hclust.f`, `twins.c`,
//! `pam.c`) line-faithful on purpose, so clippy's iterator idioms are off.
#![allow(clippy::needless_range_loop, clippy::manual_swap)]

use archaeodash_domain::DomainError;

use crate::CancellationToken;
use crate::ColumnMatrix;

// ---------------------------------------------------------------------------
// R-compatible RNG
// ---------------------------------------------------------------------------

/// R 4.x Mersenne-Twister with `set.seed` scrambling (`RNG.c`): 50 LCG
/// scrambling steps, then the 624-word MT state filled from the same LCG
/// (`i_seed[0]` is `mti`). `unif` applies R's `fixup` on the
/// `1/(2^32 - 1)` scaling; `unif_index` is the `"Rejection"` sample kind.
pub struct RMt19937 {
    mt: [u32; 624],
    mti: usize,
}

const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;
const I2_32M1: f64 = 2.328_306_437_080_797e-10; // 1/(2^32 - 1)
const D2_32: f64 = 2.328_306_436_538_696_3e-10; // 1/2^32 (MT_genrand scaling)

impl RMt19937 {
    /// Seeds exactly like R `set.seed(seed)` for the default RNG kind:
    /// 50 LCG scrambling steps, then 625 LCG words into `i_seed` where
    /// `i_seed[0]` is the `mti` slot (overwritten with 624 by FixupSeeds)
    /// and `mt[i] = i_seed[i + 1]`.
    pub fn new(seed: i32) -> Self {
        let mut seed = seed as u32;
        for _ in 0..50 {
            seed = seed.wrapping_mul(69069).wrapping_add(1);
        }
        let mut i_seed = [0u32; 625];
        for s in i_seed.iter_mut() {
            seed = seed.wrapping_mul(69069).wrapping_add(1);
            *s = seed;
        }
        let mut mt = [0u32; 624];
        mt.copy_from_slice(&i_seed[1..]);
        Self { mt, mti: 624 }
    }

    fn genrand_int32(&mut self) -> u32 {
        if self.mti >= 624 {
            for kk in 0..227 {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] =
                    self.mt[kk + 397] ^ (y >> 1) ^ (if y & 1 == 1 { MATRIX_A } else { 0 });
            }
            for kk in 227..623 {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] =
                    self.mt[kk + 397 - 624] ^ (y >> 1) ^ (if y & 1 == 1 { MATRIX_A } else { 0 });
            }
            let y = (self.mt[623] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
            self.mt[623] = self.mt[396] ^ (y >> 1) ^ (if y & 1 == 1 { MATRIX_A } else { 0 });
            self.mti = 0;
        }
        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// R `runif` on `[0, 1)`: `fixup(MT_genrand())` where MT_genrand scales
    /// the tempered word by `1/2^32` (`d2_32`, not `i2_32m1`).
    pub fn unif(&mut self) -> f64 {
        let u = self.genrand_int32() as f64 * D2_32;
        if u <= 0.0 {
            0.5 * I2_32M1
        } else if (1.0 - u) <= 0.0 {
            1.0 - 0.5 * I2_32M1
        } else {
            u
        }
    }

    fn rbits(&mut self, bits: u32) -> f64 {
        let mut v: i64 = 0;
        let mut nb: u32 = 0;
        while nb <= bits {
            let v1 = (self.unif() * 65536.0) as i64;
            v = 65536 * v + v1;
            nb += 16;
        }
        (v & ((1i64 << bits) - 1)) as f64
    }

    /// R `R_unif_index` (`Sample_kind = "Rejection"`).
    pub fn unif_index(&mut self, dn: f64) -> f64 {
        if dn <= 0.0 {
            return 0.0;
        }
        let bits = dn.log2().ceil() as u32;
        loop {
            let dv = self.rbits(bits);
            if dn > dv {
                return dv;
            }
        }
    }
}

/// R `sample.int(n, size, replace = FALSE)` (non-hashing path), returning
/// 1-based draws.
pub fn r_sample_int(rng: &mut RMt19937, n: usize, size: usize) -> Vec<usize> {
    let mut x: Vec<usize> = (0..n).collect();
    let mut remaining = n;
    let mut out = Vec::with_capacity(size);
    for _ in 0..size {
        let j = rng.unif_index(remaining as f64) as usize;
        out.push(x[j] + 1);
        remaining -= 1;
        x[j] = x[remaining];
    }
    out
}

// ---------------------------------------------------------------------------
// k-means (Hartigan-Wong, AS-136 via kmns.f)
// ---------------------------------------------------------------------------

/// `stats::kmeans` result pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Kmeans {
    /// 1-based cluster labels per row (`cluster`).
    pub cluster: Vec<i32>,
    /// Cluster means, `k x p` row-major (`centers`).
    pub centers: Vec<Vec<f64>>,
    /// Per-cluster within-cluster sum of squares (`withinss`).
    pub withinss: Vec<f64>,
    /// `tot.withinss`.
    pub tot_withinss: f64,
    /// Cluster sizes (`size`).
    pub size: Vec<i32>,
    /// Iterations used (`iter`).
    pub iter: usize,
}

const BIG: f64 = 1.0e30;

/// AS-136 state shared by the optimal-transfer and quick-transfer stages.
struct Kmns<'a> {
    x: &'a [Vec<f64>],
    n: usize,
    p: usize,
    k: usize,
    c: Vec<Vec<f64>>,
    ic1: Vec<usize>,
    ic2: Vec<usize>,
    nc: Vec<usize>,
    an1: Vec<f64>,
    an2: Vec<f64>,
    ncp: Vec<i64>,
    d: Vec<f64>,
    itran: Vec<bool>,
    live: Vec<i64>,
    indx: usize,
    imaxqtr: i64,
}

impl<'a> Kmns<'a> {
    #[allow(dead_code)]
    fn dist2(&self, i: usize, l: usize) -> f64 {
        let mut s = 0.0f64;
        for j in 0..self.p {
            let dv = self.x[i][j] - self.c[l][j];
            s += dv * dv;
        }
        s
    }

    /// Fortran OPTRA: one pass over the data, optimal transfers only.
    fn optra(&mut self) {
        let n = self.n;
        let k = self.k;
        let p = self.p;
        for l in 0..k {
            if self.itran[l] {
                self.live[l] = n as i64 + 1;
            }
        }
        for i in 0..n {
            self.indx += 1;
            let l1 = self.ic1[i];
            let mut l2 = self.ic2[i];
            let ll = l2;
            // Fortran `IF (NC(L1) .EQ. 1) GO TO 90`: skip the transfer scan
            // but still run the `INDX == M` check below.
            if self.nc[l1] != 1 {
                if self.ncp[l1] != 0 {
                    let mut de = 0.0f64;
                    for j in 0..p {
                        let df = self.x[i][j] - self.c[l1][j];
                        de += df * df;
                    }
                    self.d[i] = de * self.an1[l1];
                }
                let mut da = 0.0f64;
                for j in 0..p {
                    let db = self.x[i][j] - self.c[l2][j];
                    da += db * db;
                }
                let mut r2 = da * self.an2[l2];
                for l in 0..k {
                    let i1 = i as i64 + 1;
                    if (i1 >= self.live[l1] && i1 >= self.live[l]) || l == l1 || l == ll {
                        continue;
                    }
                    let rr = r2 / self.an2[l];
                    let mut dc = 0.0f64;
                    let mut early = false;
                    for j in 0..p {
                        let dd = self.x[i][j] - self.c[l][j];
                        dc += dd * dd;
                        if dc >= rr {
                            early = true;
                            break;
                        }
                    }
                    if early {
                        continue;
                    }
                    r2 = dc * self.an2[l];
                    l2 = l;
                }
                if r2 < self.d[i] {
                    self.indx = 0;
                    // Fortran: LIVE(L1) = M + I (1-based I); the decrement by
                    // M at the stage tail leaves the step of the last update.
                    let step = n as i64 + i as i64 + 1;
                    self.live[l1] = step;
                    self.live[l2] = step;
                    self.ncp[l1] = i as i64 + 1;
                    self.ncp[l2] = i as i64 + 1;
                    let al1 = self.nc[l1] as f64;
                    let alw = al1 - 1.0;
                    let al2 = self.nc[l2] as f64;
                    let alt = al2 + 1.0;
                    for j in 0..p {
                        self.c[l1][j] = (self.c[l1][j] * al1 - self.x[i][j]) / alw;
                        self.c[l2][j] = (self.c[l2][j] * al2 + self.x[i][j]) / alt;
                    }
                    self.nc[l1] -= 1;
                    self.nc[l2] += 1;
                    self.an2[l1] = alw / al1;
                    self.an1[l1] = if alw <= 1.0 { BIG } else { alw / (alw - 1.0) };
                    self.an1[l2] = alt / al2;
                    self.an2[l2] = alt / (alt + 1.0);
                    self.ic1[i] = l2;
                    self.ic2[i] = l1;
                } else {
                    self.ic2[i] = l2;
                }
            }
            if self.indx == n {
                return;
            }
        }
        for l in 0..k {
            self.itran[l] = false;
            self.live[l] -= n as i64;
        }
    }

    /// Fortran QTRAN: quick transfers until no change for a full pass.
    fn qtran(&mut self) -> bool {
        // Returns false when the iMaxQtr step limit was exceeded (ifault = 4).
        let n = self.n;
        let _k = self.k;
        let p = self.p;
        let mut icoun = 0usize;
        let mut istep: i64 = 0;
        loop {
            for i in 0..n {
                icoun += 1;
                istep += 1;
                if istep >= self.imaxqtr {
                    self.imaxqtr = -1;
                    return false;
                }
                let l1 = self.ic1[i];
                let l2 = self.ic2[i];
                // Fortran `IF (NC(L1) .EQ. 1) GO TO 60` and the early exit in
                // the scan below both jump to the `ICOUN == M` check that
                // closes each iteration; only the transfer block is skipped.
                if self.nc[l1] != 1 {
                    if istep <= self.ncp[l1] {
                        let mut da = 0.0f64;
                        for j in 0..p {
                            let db = self.x[i][j] - self.c[l1][j];
                            da += db * db;
                        }
                        self.d[i] = da * self.an1[l1];
                    }
                    if istep < self.ncp[l1] || istep < self.ncp[l2] {
                        let r2 = self.d[i] / self.an2[l2];
                        let mut dd = 0.0f64;
                        let mut early = false;
                        for j in 0..p {
                            let de = self.x[i][j] - self.c[l2][j];
                            dd += de * de;
                            if dd >= r2 {
                                early = true;
                                break;
                            }
                        }
                        if !early {
                            icoun = 0;
                            self.indx = 0;
                            self.itran[l1] = true;
                            self.itran[l2] = true;
                            self.ncp[l1] = istep + n as i64;
                            self.ncp[l2] = istep + n as i64;
                            let al1 = self.nc[l1] as f64;
                            let alw = al1 - 1.0;
                            let al2 = self.nc[l2] as f64;
                            let alt = al2 + 1.0;
                            for j in 0..p {
                                self.c[l1][j] = (self.c[l1][j] * al1 - self.x[i][j]) / alw;
                                self.c[l2][j] = (self.c[l2][j] * al2 + self.x[i][j]) / alt;
                            }
                            self.nc[l1] -= 1;
                            self.nc[l2] += 1;
                            self.an2[l1] = alw / al1;
                            self.an1[l1] = if alw <= 1.0 { BIG } else { alw / (alw - 1.0) };
                            self.an1[l2] = alt / al2;
                            self.an2[l2] = alt / (alt + 1.0);
                            self.ic1[i] = l2;
                            self.ic2[i] = l1;
                        }
                    }
                }
                if icoun == n {
                    return true;
                }
            }
        }
    }

    /// Fortran tail: final centers and per-cluster WSS from `IC1`/`NC`.
    fn finish(&self) -> Kmeans {
        let n = self.n;
        let k = self.k;
        let p = self.p;
        let mut c: Vec<Vec<f64>> = vec![vec![0.0; p]; k];
        for i in 0..n {
            let ii = self.ic1[i];
            for j in 0..p {
                c[ii][j] += self.x[i][j];
            }
        }
        for j in 0..p {
            for l in 0..k {
                c[l][j] /= self.nc[l] as f64;
            }
        }
        let mut wss = vec![0.0f64; k];
        for i in 0..n {
            let ii = self.ic1[i];
            for j in 0..p {
                let da = self.x[i][j] - c[ii][j];
                wss[ii] += da * da;
            }
        }
        Kmeans {
            cluster: self.ic1.iter().map(|&l| l as i32 + 1).collect(),
            centers: c,
            withinss: wss.clone(),
            tot_withinss: wss.iter().sum(),
            size: self.nc.iter().map(|&v| v as i32).collect(),
            iter: 0,
        }
    }
}

/// Runs one Hartigan-Wong start from the given initial centers.
fn kmns_start(
    x: &[Vec<f64>],
    n: usize,
    p: usize,
    k: usize,
    centers: &[Vec<f64>],
    iter_max: usize,
    cancel: &CancellationToken,
) -> Result<Kmeans, DomainError> {
    cancel.check()?;
    let mut s = Kmns {
        x,
        n,
        p,
        k,
        c: centers.to_vec(),
        ic1: vec![0usize; n],
        ic2: vec![0usize; n],
        nc: vec![0usize; k],
        an1: vec![0.0f64; k],
        an2: vec![0.0f64; k],
        ncp: vec![0i64; k],
        d: vec![0.0f64; n],
        itran: vec![true; k],
        live: vec![0i64; k],
        indx: 0,
        imaxqtr: (50usize.saturating_mul(n)).min(i32::MAX as usize) as i64,
    };

    // Initial two-closest-centre assignment (Fortran DO 50 block; the early
    // exit `IF (DB .GE. DT(2)) GO TO 50` skips the promote step).
    for i in 0..n {
        if i % 64 == 0 {
            cancel.check()?;
        }
        s.ic1[i] = 0;
        s.ic2[i] = 1;
        let mut dt = [0.0f64; 2];
        for (il, cell) in dt.iter_mut().enumerate() {
            let mut v = 0.0f64;
            for j in 0..p {
                let da = x[i][j] - centers[il][j];
                v += da * da;
            }
            *cell = v;
        }
        if dt[0] > dt[1] {
            s.ic1[i] = 1;
            s.ic2[i] = 0;
            dt.swap(0, 1);
        }
        for l in 2..k {
            let mut db = 0.0f64;
            let mut early = false;
            for j in 0..p {
                let dc = x[i][j] - centers[l][j];
                db += dc * dc;
                if db >= dt[1] {
                    early = true;
                    break;
                }
            }
            if early {
                continue;
            }
            if db >= dt[0] {
                dt[1] = db;
                s.ic2[i] = l;
            } else {
                dt[1] = dt[0];
                s.ic2[i] = s.ic1[i];
                dt[0] = db;
                s.ic1[i] = l;
            }
        }
    }

    // Centre update; an empty cluster is a hard error (R ifault = 1).
    for l in 0..k {
        s.nc[l] = 0;
    }
    for i in 0..n {
        s.nc[s.ic1[i]] += 1;
    }
    for l in 0..k {
        s.c[l] = vec![0.0; p];
    }
    for i in 0..n {
        let l = s.ic1[i];
        for j in 0..p {
            s.c[l][j] += x[i][j];
        }
    }
    for (l, count) in s.nc.clone().iter().enumerate() {
        if *count == 0 {
            return Err(DomainError::validation(
                "kmeans_empty_cluster",
                "kmeans empty cluster: try a better set of initial centers",
            ));
        }
        let aa = *count as f64;
        s.c[l] = s.c[l].iter().map(|v| v / aa).collect();
        s.an2[l] = aa / (aa + 1.0);
        s.an1[l] = if aa <= 1.0 { BIG } else { aa / (aa - 1.0) };
        s.itran[l] = true;
        s.ncp[l] = -1;
    }

    let mut iter_used = 0usize;
    while iter_used < iter_max {
        cancel.check()?;
        iter_used += 1;
        s.optra();
        cancel.check()?;
        if s.indx == n {
            break;
        }
        if !s.qtran() {
            cancel.check()?;
            break;
        }
        if k == 2 {
            break;
        }
        for l in 0..k {
            s.ncp[l] = 0;
        }
    }
    let mut result = s.finish();
    result.iter = iter_used;
    Ok(result)
}

/// Deduplicates rows in first-occurrence order (R `unique` on a data frame).
fn unique_rows(x: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let mut seen: Vec<Vec<f64>> = Vec::new();
    'outer: for row in x {
        for seen_row in &seen {
            if row.iter().zip(seen_row.iter()).all(|(a, b)| a == b) {
                continue 'outer;
            }
        }
        seen.push(row.to_vec());
    }
    seen
}

/// Runs `stats::kmeans(x, centers = k, iter.max, nstart)` with the
/// Hartigan-Wong algorithm: with `nstart >= 2`, initial centres are drawn
/// with one seeded `sample.int` over the unique rows per start (R samples
/// `cn <- unique(x)` whenever `nstart >= 2`), keeping the best
/// `tot.withinss`. `k == 1` short-circuits to the grand-mean total sum of
/// squares (R switches to the Lloyd/MacQueen path, whose single-cluster
/// result is the grand mean).
pub fn kmeans(
    x: &ColumnMatrix,
    centers: usize,
    iter_max: usize,
    nstart: usize,
    seed: i32,
) -> Result<Kmeans, DomainError> {
    kmeans_cancellable(
        x,
        centers,
        iter_max,
        nstart,
        seed,
        &CancellationToken::default(),
    )
}

pub fn kmeans_cancellable(
    x: &ColumnMatrix,
    centers: usize,
    iter_max: usize,
    nstart: usize,
    seed: i32,
    cancel: &CancellationToken,
) -> Result<Kmeans, DomainError> {
    cancel.check()?;
    let n = x.n_rows();
    let p = x.cols.len();
    if n < 2 || p == 0 {
        return Err(DomainError::validation(
            "kmeans_input",
            "kmeans requires at least two rows and one column",
        ));
    }
    if centers < 1 || centers >= n {
        return Err(DomainError::validation(
            "kmeans_centers",
            "number of cluster centres must lie between 1 and nrow(x)",
        ));
    }
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..p).map(|j| x.cols[j][i]).collect())
        .collect();

    if centers == 1 {
        let mut mean = vec![0.0f64; p];
        for row in &rows {
            for (j, v) in row.iter().enumerate() {
                mean[j] += v;
            }
        }
        for v in mean.iter_mut() {
            *v /= n as f64;
        }
        let mut tot = 0.0f64;
        for row in &rows {
            for j in 0..p {
                let dv = row[j] - mean[j];
                tot += dv * dv;
            }
        }
        return Ok(Kmeans {
            cluster: vec![1; n],
            centers: vec![mean],
            withinss: vec![tot],
            tot_withinss: tot,
            size: vec![n as i32],
            iter: 0,
        });
    }

    let cn = unique_rows(&rows);
    let mm = cn.len();
    if centers > mm {
        return Err(DomainError::validation(
            "kmeans_centers",
            "more cluster centers than distinct data points",
        ));
    }

    let mut rng = RMt19937::new(seed);
    let mut best: Option<Kmeans> = None;
    for _ in 0..nstart {
        cancel.check()?;
        let idx = r_sample_int(&mut rng, mm, centers);
        let init: Vec<Vec<f64>> = idx.iter().map(|&i| cn[i - 1].clone()).collect();
        let fit = kmns_start(&rows, n, p, centers, &init, iter_max, cancel)?;
        if best
            .as_ref()
            .is_none_or(|b| fit.tot_withinss < b.tot_withinss)
        {
            best = Some(fit);
        }
    }
    best.ok_or_else(|| DomainError::validation("kmeans_centers", "no k-means start succeeded"))
}

// ---------------------------------------------------------------------------
// PAM (cluster::pam, original build + swap, pamonce = FALSE)
// ---------------------------------------------------------------------------

/// Distance supported by the Phase 6 clustering procedures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistanceMetric {
    Euclidean,
    Manhattan,
    Minkowski { p: f64 },
    Maximum,
}

/// Agglomerative linkage supported by HCA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkageMethod {
    Average,
    Complete,
    WardD,
    WardD2,
}

fn validate_metric(metric: DistanceMetric) -> Result<(), DomainError> {
    if let DistanceMetric::Minkowski { p } = metric {
        if !p.is_finite() || p <= 0.0 {
            return Err(DomainError::validation(
                "cluster_metric",
                "Minkowski p must be finite and positive",
            ));
        }
    }
    Ok(())
}

fn metric_distance(a: &[f64], b: &[f64], metric: DistanceMetric) -> f64 {
    match metric {
        DistanceMetric::Euclidean => a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f64>()
            .sqrt(),
        DistanceMetric::Manhattan => a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum(),
        DistanceMetric::Minkowski { p } => a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs().powf(p))
            .sum::<f64>()
            .powf(1.0 / p),
        DistanceMetric::Maximum => a
            .iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max),
    }
}

fn metric_matrix(
    x: &ColumnMatrix,
    metric: DistanceMetric,
    cancel: &CancellationToken,
) -> Result<Vec<Vec<f64>>, DomainError> {
    validate_metric(metric)?;
    let n = x.n_rows();
    let p = x.cols.len();
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..p).map(|j| x.cols[j][i]).collect())
        .collect();
    let mut d = vec![vec![0.0; n]; n];
    for i in 0..n {
        if i % 16 == 0 {
            cancel.check()?;
        }
        for j in i + 1..n {
            let v = metric_distance(&rows[i], &rows[j], metric);
            d[i][j] = v;
            d[j][i] = v;
        }
    }
    Ok(d)
}

/// `cluster::pam` result pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Pam {
    /// 1-based medoid object indices in cluster-number order (`medoids`).
    pub medoids: Vec<usize>,
    /// Average distance to the closest medoid after BUILD (`objective[1]`).
    pub build_objective: f64,
    /// Average distance after SWAP (`objective[2]`).
    pub swap_objective: f64,
    /// 1-based cluster labels per row in `cstat` first-appearance order
    /// (`clustering`).
    pub clustering: Vec<i32>,
}

/// Runs `cluster::pam(x, k, metric = "euclidean")` with `pamonce = FALSE`:
/// BUILD picks medoids greedily (Fortran keeps the *last* argmax via
/// `AMMAX <= BETER(I)`), SWAP applies the best `T_{i,h}` improvement while it
/// clears `-16 * eps * |objective|`, and `cstat` numbers clusters by
/// first appearance in row order.
pub fn pam(x: &ColumnMatrix, k: usize) -> Result<Pam, DomainError> {
    pam_with_metric(x, k, DistanceMetric::Euclidean)
}

/// PAM using the requested R `cluster::daisy` metric.
pub fn pam_with_metric(
    x: &ColumnMatrix,
    k: usize,
    metric: DistanceMetric,
) -> Result<Pam, DomainError> {
    pam_with_metric_cancellable(x, k, metric, &CancellationToken::default())
}

pub fn pam_with_metric_cancellable(
    x: &ColumnMatrix,
    k: usize,
    metric: DistanceMetric,
    cancel: &CancellationToken,
) -> Result<Pam, DomainError> {
    cancel.check()?;
    if !matches!(
        metric,
        DistanceMetric::Euclidean | DistanceMetric::Manhattan
    ) {
        return Err(DomainError::validation(
            "pam_metric",
            "PAM supports Euclidean and Manhattan distances",
        ));
    }
    let n = x.n_rows();
    let p = x.cols.len();
    if n < 2 || k < 1 || k >= n {
        return Err(DomainError::validation(
            "pam_input",
            "pam requires 1 < k < nrow(x)",
        ));
    }
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..p).map(|j| x.cols[j][i]).collect())
        .collect();
    let dist = if metric == DistanceMetric::Euclidean {
        dense_euclidean(&rows, n, p)
    } else {
        metric_matrix(x, metric, cancel)?
    };

    // BUILD (bswap, med_given = FALSE): `s` is the sentinel
    // 1.1 * max(dys) + 1 used to seed dysma; for the first medoid every
    // `beter[i]` sums the positive parts of `s - d(i, j)` over all j.
    let mut max_dys = 0.0f64;
    for i in 1..=n {
        for j in (i + 1)..=n {
            let d = dist[i - 1][j - 1];
            if d > max_dys {
                max_dys = d;
            }
        }
    }
    let s = max_dys * 1.1 + 1.0;
    let mut nrepr = vec![false; n + 1]; // 1-based
    let mut dysma = vec![s; n + 1];
    let mut medoids: Vec<usize> = Vec::with_capacity(k);
    for _ in 0..k {
        cancel.check()?;
        let mut nmax = 1usize;
        let mut ammax = 0.0f64;
        for i in 1..=n {
            if i % 16 == 0 {
                cancel.check()?;
            }
            if !nrepr[i] {
                let mut beter = 0.0f64;
                for j in 1..=n {
                    let cmd = dysma[j] - dist[i - 1][j - 1];
                    if cmd > 0.0 {
                        beter += cmd;
                    }
                }
                // C `if (ammax <= beter[i])`: the last maximum wins.
                if ammax <= beter {
                    ammax = beter;
                    nmax = i;
                }
            }
        }
        nrepr[nmax] = true;
        medoids.push(nmax);
        for j in 1..=n {
            let dj = dist[nmax - 1][j - 1];
            if dysma[j] > dj {
                dysma[j] = dj;
            }
        }
    }
    let mut sky = dysma[1..=n].iter().sum::<f64>();
    let build_objective = sky / n as f64;

    // SWAP (bswap L60/L60b, pamonce = 0): each pass recomputes dysma/dysmb
    // from scratch, scans all (h, i) swap candidates for the smallest T_{i,h}
    // (`dzsky > dz`, first found wins ties), and accepts it only when the
    // improvement clears -16 * eps * |sky|; `sky` accumulates the applied dz.
    let mut dysmb = vec![0.0f64; n + 1];
    loop {
        cancel.check()?;
        for j in 1..=n {
            dysma[j] = s;
            dysmb[j] = s;
            for i in 1..=n {
                if nrepr[i] {
                    let dij = dist[i - 1][j - 1];
                    if dysma[j] > dij {
                        dysmb[j] = dysma[j];
                        dysma[j] = dij;
                    } else if dysmb[j] > dij {
                        dysmb[j] = dij;
                    }
                }
            }
        }
        let mut dzsky = 1.0f64;
        let mut hbest = 0usize;
        let mut nbest = 0usize;
        for h in 1..=n {
            if h % 16 == 0 {
                cancel.check()?;
            }
            if nrepr[h] {
                continue;
            }
            for i in 1..=n {
                if !nrepr[i] {
                    continue;
                }
                let mut dz = 0.0f64;
                for j in 1..=n {
                    let hj = dist[h - 1][j - 1];
                    let ij = dist[i - 1][j - 1];
                    if ij == dysma[j] {
                        let small = if dysmb[j] > hj { hj } else { dysmb[j] };
                        dz += -dysma[j] + small;
                    } else if hj < dysma[j] {
                        dz += -dysma[j] + hj;
                    }
                }
                if dzsky > dz {
                    dzsky = dz;
                    hbest = h;
                    nbest = i;
                }
            }
        }
        if dzsky >= -16.0 * f64::EPSILON * sky.abs() {
            break;
        }
        nrepr[hbest] = true;
        nrepr[nbest] = false;
        sky += dzsky;
    }
    medoids = (1..=n).filter(|&i| nrepr[i]).collect();
    let swap_objective = sky / n as f64;

    // cstat: nearest-medoid assignment with ties going to the smallest
    // medoid object index, then cluster numbers by first appearance.
    let mut sorted_medoids = medoids.clone();
    sorted_medoids.sort_unstable();
    let mut nsend = vec![0usize; n + 1];
    for j in 1..=n {
        if !nrepr[j] {
            let mut dsmal = f64::INFINITY;
            let mut ksmal = 0usize;
            for &m in &sorted_medoids {
                let dmj = dist[m - 1][j - 1];
                if dsmal > dmj {
                    dsmal = dmj;
                    ksmal = m;
                }
            }
            nsend[j] = ksmal;
        } else {
            nsend[j] = j;
        }
    }
    let mut ncluv = vec![0i32; n + 1];
    let mut jk = 1i32;
    let mut nplac = nsend[1];
    for j in 1..=n {
        if nsend[j] == nplac {
            ncluv[j] = 1;
        }
    }
    for ja in 2..=n {
        nplac = nsend[ja];
        if ncluv[nplac] == 0 {
            jk += 1;
            for j in 1..=n {
                if nsend[j] == nplac {
                    ncluv[j] = jk;
                }
            }
            if jk == k as i32 {
                break;
            }
        }
    }
    let mut med = vec![0usize; k + 1];
    for kk in 1..=k {
        let mut m = 0usize;
        for j in 1..=n {
            if ncluv[j] == kk as i32 {
                m = nsend[j];
            }
        }
        med[kk] = m;
    }

    Ok(Pam {
        medoids: med[1..].to_vec(),
        build_objective,
        swap_objective,
        clustering: ncluv[1..].to_vec(),
    })
}

// ---------------------------------------------------------------------------
// Hierarchical clustering
// ---------------------------------------------------------------------------

/// `stats::hclust(method = "ward.D2")` result pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Hclust {
    /// R merge matrix rows, 1-based: negative `-v` is leaf `v`, positive `v`
    /// is the stage-`v` cluster.
    pub merge: Vec<[i32; 2]>,
    /// Agglomeration heights (square root of the Ward-D2 squared update).
    pub height: Vec<f64>,
    /// Dendrogram leaf order (`hcass2`).
    pub order: Vec<usize>,
}

/// Port of `stats/src/hclust.f` (`IOPT = 8`, Ward D2 on squared Euclidean
/// distances, NN-chain with live-list redetermination) plus `hcass2` for the
/// R merge coding and dendrogram order.
#[allow(clippy::too_many_lines)]
pub fn hclust_ward_d2(x: &ColumnMatrix) -> Result<Hclust, DomainError> {
    hclust_ward_d2_cancellable(x, &CancellationToken::default())
}

pub fn hclust_ward_d2_cancellable(
    x: &ColumnMatrix,
    cancel: &CancellationToken,
) -> Result<Hclust, DomainError> {
    cancel.check()?;
    let n = x.n_rows();
    let p = x.cols.len();
    if n < 2 {
        return Err(DomainError::validation(
            "hclust_input",
            "hclust requires at least two rows",
        ));
    }
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..p).map(|j| x.cols[j][i]).collect())
        .collect();
    // Squared Euclidean distances in the Fortran IOFFST compact layout used
    // by R's `dist()` (row-major upper triangle): entry 0 unused, d(I, J) for
    // 1-based I < J at J + (I-1)*N - I*(I+1)/2.
    let len = n * (n - 1) / 2 + 1;
    let mut diss = vec![0.0f64; len];
    let mut idx = 0usize;
    for i in 1..n {
        for j in (i + 1)..=n {
            let mut s = 0.0f64;
            for c in 0..p {
                let dv = rows[i - 1][c] - rows[j - 1][c];
                s += dv * dv;
            }
            idx += 1;
            diss[idx] = s;
        }
    }
    let ioffst = |i: usize, j: usize| -> usize {
        let (lo, hi) = if i < j { (i, j) } else { (j, i) };
        hi + (lo - 1) * n - (lo * (lo + 1)) / 2
    };

    let mut memb = vec![1.0f64; n + 1];
    let mut flag = vec![true; n + 1];
    let mut nn = vec![0usize; n + 1];
    let mut disnn = vec![0.0f64; n + 1];
    let inf = f64::INFINITY;

    // Initial nearest neighbours to the right of each object.
    for i in 1..n {
        let mut dmin = inf;
        let mut jm = 0usize;
        for j in (i + 1)..=n {
            let ind = ioffst(i, j);
            if dmin > diss[ind] {
                dmin = diss[ind];
                jm = j;
            }
        }
        nn[i] = jm;
        disnn[i] = dmin;
    }

    let mut ia = vec![0i64; n];
    let mut ib = vec![0i64; n];
    let mut crit = vec![0.0f64; n];
    let mut ncl = n as i64;
    let mut aggloms = 0usize;

    loop {
        cancel.check()?;
        // Least dissimilarity among the live NN pairs.
        let mut dmin = inf;
        let mut im = 0usize;
        let mut jm = 0usize;
        for i in 1..n {
            if flag[i] && disnn[i] < dmin {
                dmin = disnn[i];
                im = i;
                jm = nn[i];
            }
        }
        ncl -= 1;
        let (i2, j2) = (im.min(jm), im.max(jm));
        ia[aggloms] = i2 as i64;
        ib[aggloms] = j2 as i64;
        aggloms += 1;
        crit[aggloms - 1] = dmin.sqrt(); // ward.D2 reports sqrt of squared
        flag[j2] = false;

        // Lance-Williams update of dissimilarities from the new cluster.
        let mut dmin_new = inf;
        let mut jj = 0usize;
        for k in 1..=n {
            if flag[k] && k != i2 {
                let ind1 = ioffst(i2, k);
                let ind2 = ioffst(j2, k);
                let d12 = diss[ioffst(i2, j2)];
                diss[ind1] = ((memb[i2] + memb[k]) * diss[ind1]
                    + (memb[j2] + memb[k]) * diss[ind2]
                    - memb[k] * d12)
                    / (memb[i2] + memb[j2] + memb[k]);
                if i2 < k {
                    if diss[ind1] < dmin_new {
                        dmin_new = diss[ind1];
                        jj = k;
                    }
                } else if diss[ind1] < disnn[k] {
                    disnn[k] = diss[ind1];
                    nn[k] = i2;
                }
            }
        }
        memb[i2] += memb[j2];
        disnn[i2] = dmin_new;
        nn[i2] = jj;

        // Redetermine NNs of live clusters whose NN was merged away.
        for i in 1..n {
            if flag[i] && (nn[i] == i2 || nn[i] == j2) {
                let mut dmin2 = inf;
                let mut jm2 = 0usize;
                for j in (i + 1)..=n {
                    if flag[j] {
                        let ind = ioffst(i, j);
                        if diss[ind] < dmin2 {
                            dmin2 = diss[ind];
                            jm2 = j;
                        }
                    }
                }
                nn[i] = jm2;
                disnn[i] = dmin2;
            }
        }
        if ncl <= 1 {
            break;
        }
    }

    // hcass2: recode agglomerations as (negative leaf | positive stage) pairs.
    // `ia`/`ib` keep the raw lowest-constituent coding and are never modified;
    // `k` must be read from them (hcass2.f uses MIN(IA(I),IB(I)) on the
    // originals), while only the `iia`/`iib` copies receive the -stage refs.
    let mut iia: Vec<i64> = ia[..n - 1].to_vec();
    let mut iib: Vec<i64> = ib[..n - 1].to_vec();
    for i in 0..n - 2 {
        let k = ia[i].min(ib[i]);
        for j in (i + 1)..(n - 1) {
            if ia[j] == k {
                iia[j] = -(i as i64) - 1;
            }
            if ib[j] == k {
                iib[j] = -(i as i64) - 1;
            }
        }
    }
    for i in 0..n - 1 {
        iia[i] = -iia[i];
        iib[i] = -iib[i];
        if iia[i] > 0 && iib[i] < 0 {
            std::mem::swap(&mut iia[i], &mut iib[i]);
        }
        if iia[i] > 0 && iib[i] > 0 {
            let (k1, k2) = (iia[i].min(iib[i]), iia[i].max(iib[i]));
            iia[i] = k1;
            iib[i] = k2;
        }
    }
    // hcass2 order: start from the last merge, replacing stage references by
    // their two children until only leaves remain.
    let mut iorder: Vec<i64> = Vec::with_capacity(n);
    iorder.push(iia[n - 2]);
    iorder.push(iib[n - 2]);
    let mut loc = 2usize;
    for i in (0..n - 2).rev() {
        let stage = i as i64 + 1;
        if let Some(pos) = iorder.iter().position(|&v| v == stage) {
            if pos + 1 == loc {
                loc += 1;
                iorder.push(iib[i]);
            } else {
                loc += 1;
                iorder.insert(pos + 1, iib[i]);
            }
            iorder[pos] = iia[i];
        }
    }
    let order: Vec<usize> = iorder.iter().map(|&v| (-v) as usize).collect();

    Ok(Hclust {
        merge: (0..n - 1).map(|i| [iia[i] as i32, iib[i] as i32]).collect(),
        height: crit[..n - 1].to_vec(),
        order,
    })
}

/// Hierarchical clustering with selectable metric and R linkage update.
/// The legacy Euclidean Ward.D2 entry point above remains the line-faithful
/// R `hclust.f` implementation used by existing fixtures.
pub fn hclust(
    x: &ColumnMatrix,
    metric: DistanceMetric,
    linkage: LinkageMethod,
) -> Result<Hclust, DomainError> {
    hclust_cancellable(x, metric, linkage, &CancellationToken::default())
}

pub fn hclust_cancellable(
    x: &ColumnMatrix,
    metric: DistanceMetric,
    linkage: LinkageMethod,
    cancel: &CancellationToken,
) -> Result<Hclust, DomainError> {
    cancel.check()?;
    if metric == DistanceMetric::Euclidean && linkage == LinkageMethod::WardD2 {
        return hclust_ward_d2_cancellable(x, cancel);
    }
    hclust_nn_chain_cancellable(x, metric, linkage, cancel)
}

/// R's `hclust.f` nearest-neighbour chain and `hcass2` reconstruction, with
/// the selected dissimilarity and Lance-Williams update.
#[allow(clippy::too_many_lines)]
fn hclust_nn_chain_cancellable(
    x: &ColumnMatrix,
    metric: DistanceMetric,
    linkage: LinkageMethod,
    cancel: &CancellationToken,
) -> Result<Hclust, DomainError> {
    validate_metric(metric)?;
    let n = x.n_rows();
    if n < 2 {
        return Err(DomainError::validation(
            "hclust_input",
            "hclust requires at least two rows",
        ));
    }
    let p = x.cols.len();
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..p).map(|j| x.cols[j][i]).collect())
        .collect();
    // hclust.f uses R dist()'s compact upper-triangle IOFFST layout.
    let mut diss = vec![0.0f64; n * (n - 1) / 2 + 1];
    let mut idx = 0usize;
    for i in 1..n {
        cancel.check()?;
        for j in i + 1..=n {
            let d = metric_distance(&rows[i - 1], &rows[j - 1], metric);
            idx += 1;
            diss[idx] = if linkage == LinkageMethod::WardD2 {
                d * d
            } else {
                d
            };
        }
    }
    let ioffst = |i: usize, j: usize| -> usize {
        let (lo, hi) = if i < j { (i, j) } else { (j, i) };
        hi + (lo - 1) * n - (lo * (lo + 1)) / 2
    };
    let mut memb = vec![1.0f64; n + 1];
    let mut flag = vec![true; n + 1];
    let mut nn = vec![0usize; n + 1];
    let mut disnn = vec![0.0f64; n + 1];
    for i in 1..n {
        let mut dmin = f64::INFINITY;
        let mut jm = 0usize;
        for j in i + 1..=n {
            let d = diss[ioffst(i, j)];
            if dmin > d {
                dmin = d;
                jm = j;
            }
        }
        nn[i] = jm;
        disnn[i] = dmin;
    }
    let mut ia = vec![0i64; n];
    let mut ib = vec![0i64; n];
    let mut crit = vec![0.0f64; n];
    let mut ncl = n as i64;
    let mut aggloms = 0usize;
    loop {
        cancel.check()?;
        let mut dmin = f64::INFINITY;
        let mut im = 0usize;
        let mut jm = 0usize;
        for i in 1..n {
            if flag[i] && disnn[i] < dmin {
                dmin = disnn[i];
                im = i;
                jm = nn[i];
            }
        }
        ncl -= 1;
        let (i2, j2) = (im.min(jm), im.max(jm));
        ia[aggloms] = i2 as i64;
        ib[aggloms] = j2 as i64;
        aggloms += 1;
        crit[aggloms - 1] = if linkage == LinkageMethod::WardD2 {
            dmin.sqrt()
        } else {
            dmin
        };
        flag[j2] = false;
        let mut dmin_new = f64::INFINITY;
        let mut jj = 0usize;
        for k in 1..=n {
            if flag[k] && k != i2 {
                let ind1 = ioffst(i2, k);
                let ind2 = ioffst(j2, k);
                let d12 = diss[ioffst(i2, j2)];
                let da = diss[ind1];
                let db = diss[ind2];
                let ni = memb[i2];
                let nj = memb[j2];
                let nk = memb[k];
                diss[ind1] = match linkage {
                    LinkageMethod::Average => (ni * da + nj * db) / (ni + nj),
                    LinkageMethod::Complete => da.max(db),
                    LinkageMethod::WardD | LinkageMethod::WardD2 => {
                        ((ni + nk) * da + (nj + nk) * db - nk * d12) / (ni + nj + nk)
                    }
                };
                if i2 < k {
                    if diss[ind1] < dmin_new {
                        dmin_new = diss[ind1];
                        jj = k;
                    }
                } else if diss[ind1] < disnn[k] {
                    disnn[k] = diss[ind1];
                    nn[k] = i2;
                }
            }
        }
        memb[i2] += memb[j2];
        disnn[i2] = dmin_new;
        nn[i2] = jj;
        for i in 1..n {
            if flag[i] && (nn[i] == i2 || nn[i] == j2) {
                let mut dmin2 = f64::INFINITY;
                let mut jm2 = 0usize;
                for j in i + 1..=n {
                    if flag[j] {
                        let d = diss[ioffst(i, j)];
                        if d < dmin2 {
                            dmin2 = d;
                            jm2 = j;
                        }
                    }
                }
                nn[i] = jm2;
                disnn[i] = dmin2;
            }
        }
        if ncl <= 1 {
            break;
        }
    }
    // Same hcass2 merge recoding and leaf order used by the exact Ward.D2 port.
    let mut iia: Vec<i64> = ia[..n - 1].to_vec();
    let mut iib: Vec<i64> = ib[..n - 1].to_vec();
    for i in 0..n - 2 {
        let k = ia[i].min(ib[i]);
        for j in i + 1..n - 1 {
            if ia[j] == k {
                iia[j] = -(i as i64) - 1;
            }
            if ib[j] == k {
                iib[j] = -(i as i64) - 1;
            }
        }
    }
    for i in 0..n - 1 {
        iia[i] = -iia[i];
        iib[i] = -iib[i];
        if iia[i] > 0 && iib[i] < 0 {
            std::mem::swap(&mut iia[i], &mut iib[i]);
        }
        if iia[i] > 0 && iib[i] > 0 {
            let (a, b) = (iia[i].min(iib[i]), iia[i].max(iib[i]));
            iia[i] = a;
            iib[i] = b;
        }
    }
    let mut iorder = vec![iia[n - 2], iib[n - 2]];
    let mut loc = 2usize;
    for i in (0..n - 2).rev() {
        let stage = i as i64 + 1;
        if let Some(pos) = iorder.iter().position(|&v| v == stage) {
            if pos + 1 == loc {
                loc += 1;
                iorder.push(iib[i]);
            } else {
                loc += 1;
                iorder.insert(pos + 1, iib[i]);
            }
            iorder[pos] = iia[i];
        }
    }
    Ok(Hclust {
        merge: (0..n - 1).map(|i| [iia[i] as i32, iib[i] as i32]).collect(),
        height: crit[..n - 1].to_vec(),
        order: iorder.iter().map(|&v| (-v) as usize).collect(),
    })
}

/// `cluster::diana(x, metric = "euclidean")` result pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct Diana {
    /// R merge matrix rows (same coding as `hclust`).
    pub merge: Vec<[i32; 2]>,
    /// Banner heights (`ban[2..n]` in positional order).
    pub height: Vec<f64>,
    /// Dendrogram leaf order (`ner`).
    pub order: Vec<usize>,
}

/// Compact lower-triangular column-major Euclidean distance matrix mirroring
/// the `cluster/src/twins.c` `dys` layout from `dysta.c`/`ind_2` (entry 0
/// unused, permanently 0): `d(l, j)` for 1-based `l < j` lives at
/// `(j-1)(j-2)/2 + l`.
struct CompactDist {
    d: Vec<f64>,
}

impl CompactDist {
    fn metric(
        x: &[Vec<f64>],
        n: usize,
        metric: DistanceMetric,
        cancel: &CancellationToken,
    ) -> Result<Self, DomainError> {
        let mut d = vec![0.0f64; n * (n - 1) / 2 + 1];
        let mut idx = 1usize;
        for i in 1..=n {
            if i % 16 == 0 {
                cancel.check()?;
            }
            for j in 1..i {
                d[idx] = metric_distance(&x[i - 1], &x[j - 1], metric);
                idx += 1;
            }
        }
        Ok(Self { d })
    }

    fn at(&self, l: usize, j: usize) -> f64 {
        if l == j {
            return 0.0;
        }
        let (lo, hi) = if l < j { (l, j) } else { (j, l) };
        self.d[(hi - 1) * (hi - 2) / 2 + lo]
    }

    fn max(&self) -> f64 {
        self.d.iter().cloned().fold(0.0f64, f64::max)
    }
}

/// Port of `cluster/src/twins.c` `splyt` (DIANA splitting) with the banner
/// level and merge-structure derivation.
#[allow(clippy::too_many_lines)]
pub fn diana(x: &ColumnMatrix) -> Result<Diana, DomainError> {
    diana_with_metric(x, DistanceMetric::Euclidean)
}

/// DIANA-compatible divisive clustering with Euclidean or Manhattan distance.
pub fn diana_with_metric(x: &ColumnMatrix, metric: DistanceMetric) -> Result<Diana, DomainError> {
    diana_with_metric_cancellable(x, metric, &CancellationToken::default())
}

pub fn diana_with_metric_cancellable(
    x: &ColumnMatrix,
    metric: DistanceMetric,
    cancel: &CancellationToken,
) -> Result<Diana, DomainError> {
    cancel.check()?;
    if !matches!(
        metric,
        DistanceMetric::Euclidean | DistanceMetric::Manhattan
    ) {
        return Err(DomainError::validation(
            "diana_metric",
            "DIANA supports Euclidean and Manhattan distances",
        ));
    }
    let n = x.n_rows();
    let p = x.cols.len();
    if n < 2 {
        return Err(DomainError::validation(
            "diana_input",
            "diana requires at least two rows",
        ));
    }
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..p).map(|j| x.cols[j][i]).collect())
        .collect();
    let dys = CompactDist::metric(&rows, n, metric, cancel)?;
    let at = |l: usize, j: usize| dys.at(l, j);

    let mut kwan = vec![0usize; n + 1];
    let mut ban = vec![0.0f64; n + 1];
    let mut ner: Vec<usize> = (0..=n).collect();
    kwan[1] = n;
    let mut ja = 1usize;
    let mut nclu = 1usize;
    let cs = dys.max();
    let mut merge_rows: Vec<[i32; 2]> = Vec::with_capacity(n - 1);

    loop {
        cancel.check()?;
        let jb = ja + kwan[ja] - 1;
        let mut jma = jb;
        if kwan[ja] == 2 {
            // Special case of a pair of objects.
            kwan[ja] = 1;
            kwan[jb] = 1;
            ban[jb] = at(ner[ja], ner[jb]);
        } else {
            // Find the object with the largest within-cluster dissimilarity
            // and shift it to the end of the cluster segment.
            let mut bygsd = -1.0f64;
            let mut lndsd = 0usize;
            for l in ja..=jb {
                if l % 16 == 0 {
                    cancel.check()?;
                }
                let lner = ner[l];
                let mut sd = 0.0f64;
                for j in ja..=jb {
                    sd += at(lner, ner[j]);
                }
                if bygsd < sd {
                    bygsd = sd;
                    lndsd = l;
                }
            }
            kwan[ja] -= 1;
            kwan[jb] = 1;
            if jb != lndsd {
                let lchan = ner[lndsd];
                for lmma in lndsd..jb {
                    ner[lmma] = ner[lmma + 1];
                }
                ner[jb] = lchan;
            }
            // Divisive loop: move objects maximizing the (own-group mean -
            // split-group mean) difference, bubbling the moved object into
            // place at the split boundary.
            let mut splyn = 0usize;
            jma = jb - 1;
            let mut jmb;
            loop {
                splyn += 1;
                let rest = (jma - ja) as f64;
                let mut jaway = 0usize;
                let mut bdyff = -1.0f64;
                for l in ja..=jma {
                    if l % 16 == 0 {
                        cancel.check()?;
                    }
                    let lner = ner[l];
                    let mut da = 0.0f64;
                    for j in ja..=jma {
                        da += at(lner, ner[j]);
                    }
                    da /= rest;
                    let mut db = 0.0f64;
                    for j in (jma + 1)..=jb {
                        db += at(lner, ner[j]);
                    }
                    db /= splyn as f64;
                    let dyff = da - db;
                    if bdyff < dyff {
                        bdyff = dyff;
                        jaway = l;
                    }
                }
                jmb = jma + 1;
                if bdyff <= 0.0 {
                    break;
                }
                if jma != jaway {
                    let lchan = ner[jaway];
                    for lxx in jaway..jma {
                        ner[lxx] = ner[lxx + 1];
                    }
                    ner[jma] = lchan;
                }
                let mut lxx = jmb;
                while lxx <= jb {
                    let l_1 = lxx - 1;
                    if ner[l_1] < ner[lxx] {
                        break;
                    }
                    let lchan = ner[l_1];
                    ner[l_1] = ner[lxx];
                    ner[lxx] = lchan;
                    lxx += 1;
                }
                kwan[ja] -= 1;
                kwan[jma] = kwan[jmb] + 1;
                kwan[jmb] = 0;
                jma -= 1;
                jmb = jma + 1;
                if jma == ja {
                    break;
                }
            }
            // Switch the two parts when the right part starts with a smaller
            // object index (banner ordering convention).
            if ner[ja] >= ner[jmb] {
                let mut lxxa = ja;
                for lgrb in jmb..=jb {
                    lxxa += 1;
                    let lchan = ner[lgrb];
                    let mut lxg = 0usize;
                    let mut ll = lxxa;
                    while ll <= lgrb {
                        let lxf = lgrb - ll + lxxa;
                        lxg = lxf - 1;
                        ner[lxf] = ner[lxg];
                        ll += 1;
                    }
                    ner[lxg] = lchan;
                }
                let llq = kwan[jmb];
                kwan[jmb] = 0;
                jma = ja + jb - jma - 1;
                jmb = jma + 1;
                kwan[jmb] = kwan[ja];
                kwan[ja] = llq;
            }
            // Banner level: first split uses the data-set diameter.
            if nclu == 1 {
                ban[jmb] = cs;
            } else {
                let mut dm = 0.0f64;
                for k in ja..jb {
                    if k % 16 == 0 {
                        cancel.check()?;
                    }
                    for j in (k + 1)..=jb {
                        let dd = at(ner[k], ner[j]);
                        if dm < dd {
                            dm = dd;
                        }
                    }
                }
                ban[jmb] = dm;
            }
        }
        nclu += 1;
        if nclu >= n {
            break;
        }
        // Advance `ja` to the next splittable cluster segment.
        if jb != n {
            ja += kwan[ja];
            while ja <= n && kwan[ja] <= 1 {
                ja += kwan[ja];
            }
            if ja > n {
                ja = 1;
                while kwan[ja] <= 1 {
                    ja += kwan[ja];
                }
            }
        } else {
            ja = 1;
            while kwan[ja] <= 1 {
                ja += kwan[ja];
            }
        }
        let _ = jma;
    }

    // Merge structure: repeatedly take the smallest remaining banner entry
    // (the C loop's `dmin >= ban[j]` keeps the LAST minimum) and emit the
    // adjacent leaf pair it separates, resolving stage references. Rows are
    // emitted exactly as twins.c does — no sorting of (l1, l2).
    let mut kw: Vec<i64> = kwan.iter().map(|&v| v as i64).collect();
    for _stage in 0..(n - 1) {
        let mut nj = 0usize;
        let mut dmin = cs;
        for j in 2..=n {
            if kw[j] >= 0 && dmin >= ban[j] {
                dmin = ban[j];
                nj = j;
            }
        }
        kw[nj] = -1; // mark consumed, as `kwan[nj] = -1` in twins.c
        let mut l1 = -(ner[nj - 1] as i32);
        let mut l2 = -(ner[nj] as i32);
        for (done, row) in merge_rows.iter().enumerate() {
            let stage_ref = done as i32 + 1;
            if row[0] == l1 || row[1] == l1 {
                l1 = stage_ref;
            }
            if row[0] == l2 || row[1] == l2 {
                l2 = stage_ref;
            }
        }
        merge_rows.push([l1, l2]);
    }

    Ok(Diana {
        merge: merge_rows,
        height: ban[2..].to_vec(),
        order: ner[1..].to_vec(),
    })
}

// ---------------------------------------------------------------------------
// Silhouette
// ---------------------------------------------------------------------------

/// Row-major dense Euclidean distance matrix.
pub fn dense_euclidean(x: &[Vec<f64>], n: usize, p: usize) -> Vec<Vec<f64>> {
    let mut d = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let mut s = 0.0f64;
            for c in 0..p {
                let dv = x[i][c] - x[j][c];
                s += dv * dv;
            }
            let v = s.sqrt();
            d[i][j] = v;
            d[j][i] = v;
        }
    }
    d
}

/// Per-row silhouette widths over a 1-based clustering (the `sil_width`
/// column of `cluster::silhouette`), the row-level companion to
/// [`silhouette_mean`]: `NaN` for the trivial clusterings `k <= 1` or
/// `k >= n`, `0` for singleton clusters or when `a == b`, else
/// `(b - a) / max(a, b)`.
pub fn silhouette_widths(dist: &[Vec<f64>], clustering: &[i32], k: usize) -> Vec<f64> {
    let n = clustering.len();
    if k <= 1 || k >= n {
        return vec![f64::NAN; n];
    }
    let mut counts = vec![0usize; k];
    let mut di_c = vec![vec![0.0f64; k]; n];
    for i in 0..n {
        let ci = (clustering[i] - 1) as usize;
        counts[ci] += 1;
        for j in (i + 1)..n {
            let cj = (clustering[j] - 1) as usize;
            di_c[i][cj] += dist[i][j];
            di_c[j][ci] += dist[i][j];
        }
    }
    (0..n)
        .map(|i| {
            let ci = (clustering[i] - 1) as usize;
            let mut compute_si = true;
            for j in 0..k {
                if j == ci {
                    if counts[j] == 1 {
                        compute_si = false;
                        break;
                    }
                    di_c[i][j] /= (counts[j] - 1) as f64;
                } else {
                    di_c[i][j] /= counts[j] as f64;
                }
            }
            if !compute_si {
                // `cluster::silhouette` assigns singleton observations width 0.
                return 0.0;
            }
            let ai = di_c[i][ci];
            let mut bi = if ci == 0 { di_c[i][1] } else { di_c[i][0] };
            for j in 1..k {
                if j != ci && bi > di_c[i][j] {
                    bi = di_c[i][j];
                }
            }
            if bi == ai {
                0.0
            } else {
                (bi - ai) / bi.max(ai)
            }
        })
        .collect()
}

/// Mean silhouette width over a 1-based clustering (`cluster::sildist`,
/// averaged as in the legacy `silhouette_mean` helper). Returns NaN for the
/// trivial clusterings `k <= 1` or `k >= n`, like `silhouette.default`.
pub fn silhouette_mean(dist: &[Vec<f64>], clustering: &[i32], k: usize) -> f64 {
    if k <= 1 || k >= clustering.len() || clustering.is_empty() {
        return f64::NAN;
    }
    let widths = silhouette_widths(dist, clustering, k);
    let total: f64 = widths.iter().sum();
    total / clustering.len() as f64
}

#[cfg(test)]
mod silhouette_tests {
    use super::{silhouette_mean, silhouette_widths};

    #[test]
    fn trivial_clusterings_have_nan_mean() {
        let dist = vec![
            vec![0.0, 1.0, 4.0],
            vec![1.0, 0.0, 3.0],
            vec![4.0, 3.0, 0.0],
        ];
        assert!(silhouette_mean(&dist, &[1, 1, 1], 1).is_nan());
        assert!(silhouette_mean(&dist, &[1, 2, 3], 3).is_nan());
    }

    #[test]
    fn singleton_cluster_width_is_zero() {
        let dist = vec![
            vec![0.0, 1.0, 4.0],
            vec![1.0, 0.0, 3.0],
            vec![4.0, 3.0, 0.0],
        ];
        let widths = silhouette_widths(&dist, &[1, 1, 2], 2);
        assert_eq!(widths[2], 0.0);
        assert!(widths[0].is_finite());
        assert!(widths[1].is_finite());
        assert!(
            (silhouette_mean(&dist, &[1, 1, 2], 2) - widths.iter().sum::<f64>() / 3.0).abs()
                < 1e-12
        );
    }
}

#[cfg(test)]
mod distance_linkage_tests {
    use super::{diana_with_metric, hclust, pam_with_metric, DistanceMetric, LinkageMethod};
    use crate::ColumnMatrix;

    fn fixture() -> ColumnMatrix {
        ColumnMatrix {
            names: vec!["x".into(), "y".into()],
            cols: vec![vec![0., 1., 5., 5.], vec![0., 0., 0., 2.]],
        }
    }

    #[test]
    fn manhattan_hclust_and_diana_match_r_oracle() {
        let x = fixture();
        let expected = [
            (LinkageMethod::Average, vec![1., 2., 5.5]),
            (LinkageMethod::Complete, vec![1., 2., 7.]),
            (LinkageMethod::WardD, vec![1., 2., 9.5]),
            (LinkageMethod::WardD2, vec![1., 2., 7.7781745930520225]),
        ];
        for (link, heights) in expected {
            let fit = hclust(&x, DistanceMetric::Manhattan, link).unwrap();
            assert_eq!(fit.merge, vec![[-1, -2], [-3, -4], [1, 2]]);
            for (actual, want) in fit.height.iter().zip(heights) {
                assert!(
                    (actual - want).abs() < 1e-10,
                    "{link:?}: {actual} != {want}"
                );
            }
            assert_eq!(fit.order, vec![1, 2, 3, 4]);
        }
        let fit = diana_with_metric(&x, DistanceMetric::Manhattan).unwrap();
        assert_eq!(fit.merge, vec![[-1, -2], [-3, -4], [1, 2]]);
        assert_eq!(fit.height, vec![1., 7., 2.]);
    }

    #[test]
    fn manhattan_pam_matches_r_oracle() {
        let fit = pam_with_metric(&fixture(), 2, DistanceMetric::Manhattan).unwrap();
        assert_eq!(fit.medoids, vec![2, 3]);
        assert_eq!(fit.clustering, vec![1, 1, 2, 2]);
    }

    #[test]
    fn rejects_invalid_minkowski_exponent() {
        assert!(hclust(
            &fixture(),
            DistanceMetric::Minkowski { p: 0.0 },
            LinkageMethod::Average
        )
        .is_err());
    }

    #[test]
    fn equal_distance_ties_follow_r_outputs() {
        let tied = ColumnMatrix {
            names: vec!["x".into(), "y".into()],
            cols: vec![vec![0., 0., 1., -1.], vec![0., 0., 0., 0.]],
        };
        let expected = [
            (LinkageMethod::Average, vec![0., 1., 4. / 3.]),
            (LinkageMethod::Complete, vec![0., 1., 2.]),
            (LinkageMethod::WardD, vec![0., 4. / 3., 5. / 3.]),
            (
                LinkageMethod::WardD2,
                vec![0., 1.1547005383792515, 1.632993161855452],
            ),
        ];
        for (linkage, heights) in expected {
            let fit = hclust(&tied, DistanceMetric::Euclidean, linkage).unwrap();
            assert_eq!(fit.merge, vec![[-1, -2], [-3, 1], [-4, 2]]);
            for (actual, want) in fit.height.iter().zip(heights) {
                assert!(
                    (actual - want).abs() < 1e-10,
                    "{linkage:?}: {actual} != {want}"
                );
            }
            assert_eq!(fit.order, vec![4, 3, 1, 2]);
        }
    }

    #[test]
    fn minkowski_and_maximum_distances_match_r_average_linkage_heights() {
        let x = ColumnMatrix {
            names: vec!["x".into(), "y".into()],
            cols: vec![vec![0., 1., 3., -2.], vec![0., 2., 0., 1.]],
        };
        let cases = [
            (
                DistanceMetric::Minkowski { p: 3. },
                vec![2.0800838230519041, 2.5583363974637834, 3.5110466782514442],
            ),
            (DistanceMetric::Maximum, vec![2., 2.5, 10. / 3.]),
        ];
        for (metric, heights) in cases {
            let fit = hclust(&x, metric, LinkageMethod::Average).unwrap();
            let (merge, order) = match metric {
                DistanceMetric::Minkowski { .. } => {
                    (vec![[-1, -2], [-4, 1], [-3, 2]], vec![3, 4, 1, 2])
                }
                DistanceMetric::Maximum => (vec![[-1, -2], [-3, 1], [-4, 2]], vec![4, 3, 1, 2]),
                _ => unreachable!(),
            };
            assert_eq!(fit.merge, merge);
            assert_eq!(fit.order, order);
            for (actual, want) in fit.height.iter().zip(heights) {
                assert!(
                    (actual - want).abs() < 1e-8,
                    "{metric:?}: {actual} != {want}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics bundle
// ---------------------------------------------------------------------------

/// Full clustering diagnostics replicating the golden capture: k-means at the
/// primary seed (centers 5, iter.max 100, nstart 25), PAM k = 5, ward.D2
/// hclust, DIANA, the WSS series (k = 1..10, `set.seed(seed + k)` per k),
/// and the silhouette series over the same k-means fits.
pub struct ClusterDiagnostics {
    pub kmeans: Kmeans,
    pub pam: Pam,
    pub hclust: Hclust,
    pub diana: Diana,
    pub wss: Vec<f64>,
    pub silhouette: Vec<f64>,
}

/// Computes the diagnostics on a complete (NaN-free) numeric matrix.
pub fn cluster_diagnostics(x: &ColumnMatrix, seed: i32) -> Result<ClusterDiagnostics, DomainError> {
    let n = x.n_rows();
    let p = x.cols.len();
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..p).map(|j| x.cols[j][i]).collect())
        .collect();

    let kmeans_fit = kmeans(x, 5, 100, 25, seed)?;
    let pam = pam(x, 5)?;
    let hclust = hclust_ward_d2(x)?;
    let diana = diana(x)?;

    // WSS series: k = 1 is the grand-mean total sum of squares; k = 2..10
    // re-run k-means with seed + k, mirroring the oracle capture script.
    let mut mean = vec![0.0f64; p];
    for row in &rows {
        for (j, v) in row.iter().enumerate() {
            mean[j] += v;
        }
    }
    for v in mean.iter_mut() {
        *v /= n as f64;
    }
    let mut totss = 0.0f64;
    for row in &rows {
        for j in 0..p {
            let dv = row[j] - mean[j];
            totss += dv * dv;
        }
    }
    let mut wss = vec![totss];
    let mut silhouette = Vec::new();
    let dist = dense_euclidean(&rows, n, p);
    for k in 2..=10usize {
        let fit = kmeans(x, k, 100, 25, seed + k as i32)?;
        wss.push(fit.tot_withinss);
        silhouette.push(silhouette_mean(&dist, &fit.cluster, k));
    }

    Ok(ClusterDiagnostics {
        kmeans: kmeans_fit,
        pam,
        hclust,
        diana,
        wss,
        silhouette,
    })
}
