//! Golden procedure 9 (class T): clustering against the R oracle —
//! `stats::kmeans` (Hartigan-Wong, seeded `sample.int` nstart draws),
//! `cluster::pam` (original build + swap), `stats::hclust` ward.D2, and
//! `cluster::diana` — on the INAA fixture, plus the WSS and silhouette
//! diagnostic series. Label permutations are canonicalized by matching
//! centers/medoids (cluster numbering is arbitrary in R); merges, heights,
//! and order are compared directly.

use archaeodash_analysis::{cluster_diagnostics, hclust_ward_d2, kmeans, pam};
use archaeodash_parity::{
    assert_within_serialization, base_chem, golden_json, load_inaa, numeric_frame,
};

#[test]
fn golden_09_kmeans_matches_r_hartigan_wong() {
    let golden = golden_json("09_clustering.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);

    let seed = 20260914;
    let fit = kmeans(&matrix, 5, 100, 25, seed).expect("kmeans");

    let g_tot = golden["kmeans"]["tot_withinss"].as_f64().expect("tot");
    assert!(
        (fit.tot_withinss - g_tot).abs() <= 5.1e-5,
        "tot.withinss: mine {} vs golden {g_tot}",
        fit.tot_withinss
    );

    // Centers: golden is 5x8 row-major (centers x variables), rounded to 4dp.
    let g_centers = golden["kmeans"]["centers"].as_array().expect("centers");
    assert_eq!(g_centers.len(), 5);
    // Align my cluster numbering to R's via center proximity, then compare
    // labels element-wise and centers directly.
    let p = fit.centers[0].len();
    let mut perm = vec![0usize; 5];
    let mut used = vec![false; 5];
    for (gi, gcenter) in g_centers.iter().enumerate() {
        let mut best = (f64::INFINITY, 0usize);
        for (cj, my) in fit.centers.iter().enumerate() {
            if used[cj] {
                continue;
            }
            let d: f64 = (0..p)
                .map(|j| {
                    let dv = gcenter[j].as_f64().expect("f64") - my[j];
                    dv * dv
                })
                .sum();
            if d < best.0 {
                best = (d, cj);
            }
        }
        used[best.1] = true;
        perm[best.1] = gi;
    }
    let g_cluster = golden["kmeans"]["cluster"].as_array().expect("cluster");
    assert_eq!(g_cluster.len(), fit.cluster.len());
    let mismatches = g_cluster
        .iter()
        .zip(fit.cluster.iter())
        .filter(|(g, m)| g.as_i64().expect("int") != (perm[(**m as usize) - 1] as i64) + 1)
        .count();
    assert_eq!(mismatches, 0, "kmeans label mismatches after alignment");
    for (gi, gcenter) in g_centers.iter().enumerate() {
        let my = &fit.centers[perm.iter().position(|&p| p == gi).expect("perm")];
        for (j, gv) in gcenter.as_array().expect("row").iter().enumerate() {
            assert_within_serialization(&format!("kmeans center[{gi}][{j}]"), my[j], gv.as_f64());
        }
    }

    // WSS series: k = 1..10 with seed + k, k = 1 is the grand-mean totss.
    let g_wss = golden["wss"].as_array().expect("wss");
    assert_eq!(g_wss.len(), 10);
    let mut mean = vec![0.0f64; matrix.cols[0].len()];
    let n = matrix.n_rows();
    for row in 0..n {
        for (j, col) in matrix.cols.iter().enumerate() {
            mean[j] += col[row];
        }
    }
    for v in mean.iter_mut() {
        *v /= n as f64;
    }
    let mut totss = 0.0f64;
    for row in 0..n {
        for (j, col) in matrix.cols.iter().enumerate() {
            let dv = col[row] - mean[j];
            totss += dv * dv;
        }
    }
    assert_within_serialization("wss[1] (totss)", totss, g_wss[0].as_f64());
    for k in 2..=10usize {
        let fit = kmeans(&matrix, k, 100, 25, seed + k as i32).expect("kmeans wss");
        assert_within_serialization(
            &format!("wss[{k}]"),
            fit.tot_withinss,
            g_wss[k - 1].as_f64(),
        );
    }
}

#[test]
fn golden_09_silhouette_series_matches_r() {
    let golden = golden_json("09_clustering.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);
    let g_sil = golden["silhouette"].as_array().expect("silhouette");
    assert_eq!(g_sil.len(), 9);

    let n = matrix.n_rows();
    let p = matrix.cols.len();
    let rows: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..p).map(|j| matrix.cols[j][i]).collect())
        .collect();
    let mut dist = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let mut s = 0.0f64;
            for c in 0..p {
                let dv = rows[i][c] - rows[j][c];
                s += dv * dv;
            }
            let v = s.sqrt();
            dist[i][j] = v;
            dist[j][i] = v;
        }
    }
    for k in 2..=10usize {
        let fit = kmeans(&matrix, k, 100, 25, 20260914 + k as i32).expect("kmeans");
        let sil = archaeodash_analysis::cluster::silhouette_mean(&dist, &fit.cluster, k);
        assert_within_serialization(&format!("silhouette[{k}]"), sil, g_sil[k - 2].as_f64());
    }
}

#[test]
fn golden_09_pam_matches_r() {
    let golden = golden_json("09_clustering.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);

    let result = pam(&matrix, 5).expect("pam");
    let g_med = golden["pam"]["medoids"].as_array().expect("medoids");
    let r_medoids: Vec<usize> = g_med
        .iter()
        .map(|v| v.as_i64().expect("int") as usize)
        .collect();
    assert_eq!(result.medoids, r_medoids, "medoid set and numbering");

    let g_obj = golden["pam"]["objective"].as_array().expect("objective");
    assert_within_serialization(
        "pam objective[1]",
        result.build_objective,
        g_obj[0].as_f64(),
    );
    assert_within_serialization("pam objective[2]", result.swap_objective, g_obj[1].as_f64());

    let g_cluster = golden["pam"]["cluster"].as_array().expect("cluster");
    assert_eq!(g_cluster.len(), result.clustering.len());
    let mismatches = g_cluster
        .iter()
        .zip(result.clustering.iter())
        .filter(|(g, m)| g.as_i64().expect("int") != i64::from(**m))
        .count();
    assert_eq!(mismatches, 0, "pam label mismatches");
}

#[test]
fn golden_09_hclust_matches_r() {
    let golden = golden_json("09_clustering.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);

    let result = hclust_ward_d2(&matrix).expect("hclust");
    let g_merge = golden["hclust"]["merge"].as_array().expect("merge");
    assert_eq!(g_merge.len(), result.merge.len());
    for (g, r) in g_merge.iter().zip(result.merge.iter()) {
        let row = r;
        assert_eq!(
            i64::from(row[0]),
            g[0].as_i64().expect("int"),
            "merge row left"
        );
        assert_eq!(
            i64::from(row[1]),
            g[1].as_i64().expect("int"),
            "merge row right"
        );
    }
    let g_height = golden["hclust"]["height"].as_array().expect("height");
    assert_eq!(g_height.len(), result.height.len());
    for (i, g) in g_height.iter().enumerate() {
        assert_within_serialization(&format!("hclust height[{i}]"), result.height[i], g.as_f64());
    }
    let g_order = golden["hclust"]["order"].as_array().expect("order");
    let r_order: Vec<usize> = g_order
        .iter()
        .map(|v| v.as_i64().expect("int") as usize)
        .collect();
    assert_eq!(result.order, r_order, "hclust dendrogram order");
}

#[test]
fn golden_09_diana_matches_r() {
    let golden = golden_json("09_clustering.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);

    let result = archaeodash_analysis::cluster::diana(&matrix).expect("diana");
    let g_merge = golden["diana"]["merge"].as_array().expect("merge");
    assert_eq!(g_merge.len(), result.merge.len());
    for (g, r) in g_merge.iter().zip(result.merge.iter()) {
        let row = r;
        assert_eq!(
            i64::from(row[0]),
            g[0].as_i64().expect("int"),
            "diana merge row left"
        );
        assert_eq!(
            i64::from(row[1]),
            g[1].as_i64().expect("int"),
            "diana merge row right"
        );
    }
    let g_height = golden["diana"]["height"].as_array().expect("height");
    assert_eq!(g_height.len(), result.height.len());
    for (i, g) in g_height.iter().enumerate() {
        assert_within_serialization(&format!("diana height[{i}]"), result.height[i], g.as_f64());
    }
    let g_order = golden["diana"]["order"].as_array().expect("order");
    let r_order: Vec<usize> = g_order
        .iter()
        .map(|v| v.as_i64().expect("int") as usize)
        .collect();
    assert_eq!(result.order, r_order, "diana leaf order");
}
