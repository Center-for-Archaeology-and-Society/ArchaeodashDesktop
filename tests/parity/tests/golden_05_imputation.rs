//! Golden procedure 5 (class D): mice imputation parity. The R oracle
//! (`scripts/capture_imputation_distribution.R`) masks 8% of the eight
//! golden elements of the INAA fixture and records 100 seeded mice runs per
//! legacy method. The Rust port runs the same masked matrix with 100 of its
//! own seeds; because the random streams differ (ChaCha12 vs R's
//! Mersenne-Twister and ranger's generator), the two are compared as
//! distributions, against the R-vs-R band (seeds 1-50 vs 51-100):
//!
//! - accuracy: mean |imputed - true| in column-sd units;
//! - per-cell centre: correlation of each masked cell's mean draw;
//! - spread: mean per-cell standard deviation of the draws;
//! - pooled shape: Kolmogorov-Smirnov distance of all standardized draws.
#![allow(clippy::expect_used)] // test code; a malformed golden is fatal

use archaeodash_analysis::{impute, ColumnMatrix, ImputeMethod, MICE_MAXIT};
use archaeodash_parity::golden_json;

struct Golden {
    matrix: ColumnMatrix,
    cells: Vec<(usize, usize)>,
    truth: Vec<f64>,
    /// Column sd of the true (complete) values, for standardizing.
    col_sd: Vec<f64>,
    col_mean: Vec<f64>,
}

fn golden() -> (Golden, serde_json::Value) {
    let g = golden_json("05_imputation_distribution.json");
    let names: Vec<String> = g["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .map(|v| v.as_str().expect("name").to_string())
        .collect();
    let rows = g["data"].as_array().expect("data");
    let mut cols = vec![Vec::with_capacity(rows.len()); names.len()];
    for row in rows {
        for (j, v) in row.as_array().expect("row").iter().enumerate() {
            cols[j].push(v.as_f64().unwrap_or(f64::NAN));
        }
    }
    let cells: Vec<(usize, usize)> = g["cells"]
        .as_array()
        .expect("cells")
        .iter()
        .map(|c| {
            let c = c.as_array().expect("cell");
            (
                c[0].as_u64().expect("row") as usize,
                c[1].as_u64().expect("col") as usize,
            )
        })
        .collect();
    let truth: Vec<f64> = g["truth"]
        .as_array()
        .expect("truth")
        .iter()
        .map(|v| v.as_f64().expect("t"))
        .collect();
    // Complete columns: observed values plus the masked truths.
    let mut complete = cols.clone();
    for (k, &(i, j)) in cells.iter().enumerate() {
        complete[j][i] = truth[k];
    }
    let col_mean: Vec<f64> = complete
        .iter()
        .map(|c| c.iter().sum::<f64>() / c.len() as f64)
        .collect();
    let col_sd: Vec<f64> = complete
        .iter()
        .zip(&col_mean)
        .map(|(c, m)| {
            (c.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (c.len() - 1) as f64).sqrt()
        })
        .collect();
    (
        Golden {
            matrix: ColumnMatrix { names, cols },
            cells,
            truth,
            col_sd,
            col_mean,
        },
        g,
    )
}

/// Draws per masked cell: `draws[cell][run]`.
fn r_draws(g: &serde_json::Value, method: &str) -> Vec<Vec<f64>> {
    g["draws"][method]
        .as_array()
        .expect("method draws")
        .iter()
        .map(|cell| {
            cell.as_array()
                .expect("cell draws")
                .iter()
                .map(|v| v.as_f64().expect("draw"))
                .collect()
        })
        .collect()
}

fn rust_draws(golden: &Golden, method: ImputeMethod, seeds: std::ops::Range<u64>) -> Vec<Vec<f64>> {
    let mut draws = vec![Vec::new(); golden.cells.len()];
    for seed in seeds {
        let out = impute(&golden.matrix, method, seed, MICE_MAXIT).expect("impute");
        assert!(
            out.excluded.is_empty(),
            "no golden column is constant or collinear"
        );
        for (k, &(i, j)) in golden.cells.iter().enumerate() {
            draws[k].push(out.matrix.cols[j][i]);
        }
    }
    draws
}

struct Metrics {
    error: f64,
    centre_corr: f64,
    spread: f64,
    ks: f64,
}

fn standardized(golden: &Golden, k: usize, v: f64) -> f64 {
    let j = golden.cells[k].1;
    (v - golden.col_mean[j]) / golden.col_sd[j]
}

fn summarize(golden: &Golden, draws: &[Vec<f64>]) -> (f64, Vec<f64>, f64, Vec<f64>) {
    let mut err = 0.0;
    let mut count = 0.0;
    let mut centres = Vec::new();
    let mut spread = 0.0;
    let mut pooled = Vec::new();
    for (k, cell) in draws.iter().enumerate() {
        let z: Vec<f64> = cell.iter().map(|&v| standardized(golden, k, v)).collect();
        let t = standardized(golden, k, golden.truth[k]);
        err += z.iter().map(|v| (v - t).abs()).sum::<f64>();
        count += z.len() as f64;
        let m = z.iter().sum::<f64>() / z.len() as f64;
        centres.push(m);
        spread += (z.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (z.len() - 1) as f64).sqrt();
        pooled.extend(z);
    }
    pooled.sort_by(f64::total_cmp);
    (err / count, centres, spread / draws.len() as f64, pooled)
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

/// Two-sample Kolmogorov-Smirnov statistic of sorted samples.
fn ks(a: &[f64], b: &[f64]) -> f64 {
    let (mut i, mut j, mut d) = (0, 0, 0.0_f64);
    while i < a.len() && j < b.len() {
        let x = a[i].min(b[j]);
        while i < a.len() && a[i] <= x {
            i += 1;
        }
        while j < b.len() && b[j] <= x {
            j += 1;
        }
        d = d.max((i as f64 / a.len() as f64 - j as f64 / b.len() as f64).abs());
    }
    d
}

fn compare(golden: &Golden, a: &[Vec<f64>], b: &[Vec<f64>]) -> (Metrics, Metrics) {
    let (ea, ca, sa, pa) = summarize(golden, a);
    let (eb, cb, sb, pb) = summarize(golden, b);
    let shared = Metrics {
        error: eb / ea,
        centre_corr: correlation(&ca, &cb),
        spread: sb / sa,
        ks: ks(&pa, &pb),
    };
    let absolute = Metrics {
        error: ea,
        centre_corr: f64::NAN,
        spread: sa,
        ks: f64::NAN,
    };
    (shared, absolute)
}

fn check(method: ImputeMethod, key: &str) {
    let (golden, g) = golden();
    let r = r_draws(&g, key);
    let (r_first, r_second): (Vec<Vec<f64>>, Vec<Vec<f64>>) = r
        .iter()
        .map(|c| (c[..50].to_vec(), c[50..].to_vec()))
        .unzip();
    let ours = rust_draws(&golden, method, 1..101);
    let (band, _) = compare(&golden, &r_first, &r_second);
    let (vs, abs_r) = compare(&golden, &r, &ours);
    eprintln!(
        "{key}: R error {:.3}; R-vs-R error ratio {:.3}, centre corr {:.3}, spread ratio {:.3}, KS {:.3}; \
         Rust-vs-R error ratio {:.3}, centre corr {:.3}, spread ratio {:.3}, KS {:.3}",
        abs_r.error, band.error, band.centre_corr, band.spread, band.ks, vs.error, vs.centre_corr, vs.spread, vs.ks
    );
    assert!(
        (vs.error - 1.0).abs() <= 0.15,
        "{key}: accuracy differs from R: ratio {:.3}",
        vs.error
    );
    assert!(
        vs.spread > 0.8 && vs.spread < 1.25,
        "{key}: draw spread differs from R: ratio {:.3}",
        vs.spread
    );
    assert!(
        vs.centre_corr >= band.centre_corr - 0.1,
        "{key}: per-cell centres disagree: {:.3} vs R band {:.3}",
        vs.centre_corr,
        band.centre_corr
    );
    assert!(
        vs.ks <= (band.ks * 2.0).max(0.06),
        "{key}: pooled shape differs: KS {:.3} vs R band {:.3}",
        vs.ks,
        band.ks
    );
}

#[test]
fn pmm_matches_mice_distribution() {
    check(ImputeMethod::Pmm, "pmm");
}

#[test]
fn midastouch_matches_mice_distribution() {
    check(ImputeMethod::MidasTouch, "midastouch");
}

#[test]
fn rf_matches_mice_distribution() {
    check(ImputeMethod::Rf, "rf");
}
