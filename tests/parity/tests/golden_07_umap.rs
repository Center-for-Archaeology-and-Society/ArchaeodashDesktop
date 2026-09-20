//! Golden procedure 7 (class D): UMAP naive-method parity against the R
//! `umap` 0.2.10.0 oracle on the INAA fixture (three seeded runs). The
//! deterministic pipeline stages (k-nn graph, `smooth.knn.dist`, fuzzy-set
//! weights, Laplacian spectrum, `a`/`b` fit) are pinned at oracle precision;
//! the final layouts are compared distributionally (10-NN overlap and
//! pairwise-distance correlation) because the SGD random stream and the
//! eigensolver differ between R (Mersenne-Twister + RSpectra) and this port
//! (seeded ChaCha12 + faer). Thresholds sit well below the measured R-vs-R
//! seed-to-seed band (overlap 0.85-0.86, correlation 0.89-0.97).

use archaeodash_analysis::umap::{
    find_ab_params, fuzzy_simplicial_set, knn_brute_force, laplacian_smallest_eigenvalues,
    smooth_knn_dist, umap,
};
use archaeodash_parity::{base_chem, golden_json, load_inaa, numeric_frame};

fn golden_layouts(golden: &serde_json::Value) -> Vec<Vec<[f64; 2]>> {
    golden
        .as_array()
        .expect("golden runs array")
        .iter()
        .map(|run| {
            run["layout"]
                .as_array()
                .expect("golden layout")
                .iter()
                .map(|row| {
                    [
                        row["V1"].as_f64().expect("V1"),
                        row["V2"].as_f64().expect("V2"),
                    ]
                })
                .collect()
        })
        .collect()
}

fn pairwise(a: &[[f64; 2]]) -> Vec<f64> {
    let n = a.len();
    let mut distances = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = a[i][0] - a[j][0];
            let dy = a[i][1] - a[j][1];
            distances.push((dx * dx + dy * dy).sqrt());
        }
    }
    distances
}

fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for (xi, yi) in x.iter().zip(y.iter()) {
        sxy += (xi - mx) * (yi - my);
        sxx += (xi - mx) * (xi - mx);
        syy += (yi - my) * (yi - my);
    }
    sxy / (sxx * syy).sqrt()
}

/// Fraction of each point's 10 nearest neighbors (self excluded) shared
/// between two layouts.
fn knn_overlap(a: &[[f64; 2]], b: &[[f64; 2]], k: usize) -> f64 {
    let n = a.len();
    let mut total = 0usize;
    for i in 0..n {
        let mut da: Vec<(f64, usize)> = (0..n)
            .filter(|&j| j != i)
            .map(|j| {
                let dx = a[i][0] - a[j][0];
                let dy = a[i][1] - a[j][1];
                (dx * dx + dy * dy, j)
            })
            .collect();
        let mut db: Vec<(f64, usize)> = (0..n)
            .filter(|&j| j != i)
            .map(|j| {
                let dx = b[i][0] - b[j][0];
                let dy = b[i][1] - b[j][1];
                (dx * dx + dy * dy, j)
            })
            .collect();
        da.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        db.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        let na: std::collections::HashSet<usize> = da.iter().take(k).map(|&(_, j)| j).collect();
        let nb: std::collections::HashSet<usize> = db.iter().take(k).map(|&(_, j)| j).collect();
        total += na.intersection(&nb).count();
    }
    total as f64 / (n * k) as f64
}

fn fixture_matrix() -> archaeodash_analysis::ColumnMatrix {
    let frame = load_inaa();
    let columns = base_chem(&frame);
    numeric_frame(&frame, &columns)
}

#[test]
fn golden_07_umap_pinned_pipeline_stages() {
    let matrix = fixture_matrix();
    assert_eq!(matrix.cols.len(), 8);
    let v = matrix.cols[0].len();
    assert_eq!(v, 307);

    // Brute-force k-nn graph vs the oracle probe (1-based R indices).
    let data: Vec<Vec<f64>> = (0..v)
        .map(|i| matrix.cols.iter().map(|col| col[i]).collect())
        .collect();
    let (indexes, distances) = knn_brute_force(&data, 15);
    let expected_idx = [
        0usize, 105, 19, 208, 79, 162, 66, 207, 2, 185, 258, 153, 183, 70, 34,
    ];
    assert_eq!(&indexes[0], &expected_idx);
    let expected_dist = [
        0.0,
        3.072831863281817,
        3.1768935282756927,
        3.8481958474069433,
        4.4626668439398411,
        4.5094718881483278,
        4.8066683014329170,
        6.2532830697162582,
        6.6373678397388831,
        7.0249214102365549,
        7.4424390867510652,
        7.7726579893881826,
        7.8368640520299939,
        7.9615621808788273,
        9.5189589667147914,
    ];
    for (j, expected) in expected_dist.iter().enumerate() {
        assert!(
            (distances[0][j] - expected).abs() < 1e-9,
            "knn distance[{j}] = {} vs {expected}",
            distances[0][j]
        );
    }

    let (sigmas, rhos) = smooth_knn_dist(&distances, 15, 1.0, 1.0);
    let expected_sigma = [
        1.3953704833984375,
        1.2579345703125,
        0.53404617309570312,
        3.230010986328125,
        3.509063720703125,
    ];
    let expected_rho = [
        3.072831863281817,
        3.3880105976811818,
        6.1722335884183765,
        4.7887469947784851,
        1.7776415386685864,
    ];
    for i in 0..5 {
        assert!(
            (sigmas[i] - expected_sigma[i]).abs() < 1e-9,
            "sigma[{i}] = {} vs {}",
            sigmas[i],
            expected_sigma[i]
        );
        assert!(
            (rhos[i] - expected_rho[i]).abs() < 1e-9,
            "rho[{i}] = {} vs {}",
            rhos[i],
            expected_rho[i]
        );
    }

    let edges = fuzzy_simplicial_set(&indexes, &distances, 1.0, 1.0, 1.0);
    assert_eq!(edges.len(), 5598, "fuzzy-graph edge count");
    let max_weight = edges.iter().map(|&(_, _, w)| w).fold(0.0f64, f64::max);
    assert!((max_weight - 1.0).abs() < 1e-12, "max weight {max_weight}");

    // Normalized-Laplacian spectrum vs the R dense-eigen probe (RSpectra
    // agrees to well within 1e-6 on these separated eigenvalues).
    let eigenvalues = laplacian_smallest_eigenvalues(&edges, v, 3).expect("connected graph");
    assert!(eigenvalues[0].abs() < 1e-10, "lambda0 = {}", eigenvalues[0]);
    assert!(
        (eigenvalues[1] - 0.00357196).abs() < 1e-6,
        "lambda1 = {}",
        eigenvalues[1]
    );
    assert!(
        (eigenvalues[2] - 0.00873702).abs() < 1e-6,
        "lambda2 = {}",
        eigenvalues[2]
    );

    // Curve-fit parameters vs the oracle's full-precision grid search.
    let (a, b) = find_ab_params(1.0, 0.1);
    assert!((a - 1.5769436126945664).abs() < 1e-6, "a = {a}");
    assert!((b - 0.8950607181519281).abs() < 1e-6, "b = {b}");
}

#[test]
fn umap_is_seed_deterministic() {
    let matrix = fixture_matrix();
    let first = umap(&matrix, 20260914).expect("umap");
    let second = umap(&matrix, 20260914).expect("umap");
    assert_eq!(first, second, "same seed must reproduce bit-identically");
}

#[test]
fn golden_07_umap_distributional_parity() {
    let golden = golden_json("07_umap.json");
    let runs = golden_layouts(&golden);
    assert_eq!(runs.len(), 3);
    let matrix = fixture_matrix();

    for run in golden.as_array().expect("golden runs array") {
        let seed = run["seed"].as_u64().expect("seed");
        let golden_layout = golden_layouts(&serde_json::json!([run])).remove(0);
        let result = umap(&matrix, seed).expect("umap");
        assert_eq!(result.layout.len(), golden_layout.len());
        let layout: Vec<[f64; 2]> = result.layout.iter().map(|row| [row[0], row[1]]).collect();

        let mine = pairwise(&layout);
        let theirs = pairwise(&golden_layout);
        let corr = pearson(&mine, &theirs);
        let overlap = knn_overlap(&layout, &golden_layout, 10);

        // Calibrated against R-vs-R seed variation (corr 0.89-0.97,
        // overlap 0.85-0.86) with margin for the differing RNG streams.
        println!("seed {seed}: pdist corr {corr:.4}, knn10 overlap {overlap:.4}");
        assert!(
            corr >= 0.75,
            "seed {seed}: pairwise-distance correlation {corr:.4} below 0.75"
        );
        assert!(
            overlap >= 0.70,
            "seed {seed}: 10-NN overlap {overlap:.4} below 0.70"
        );

        let cfg = &run["config"];
        assert_eq!(cfg["n_neighbors"].as_i64(), Some(15));
        assert_eq!(cfg["n_epochs"].as_i64(), Some(200));
        assert!(
            (result.config.a - cfg["a"].as_f64().unwrap()).abs() < 5.1e-5,
            "a vs golden"
        );
        assert!(
            (result.config.b - cfg["b"].as_f64().unwrap()).abs() < 5.1e-5,
            "b vs golden"
        );
    }
}
