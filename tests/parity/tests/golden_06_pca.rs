//! Golden procedure 6 (class T): PCA against `stats::prcomp` defaults
//! (`center = TRUE`, `scale. = FALSE`) on the INAA fixture. Component signs
//! are canonicalized on both sides (largest-|entry| positive, first maximum)
//! because LAPACK/faer eigenvector signs are arbitrary.

use archaeodash_analysis::{apply_sign_flips, component_sign_flips, pca};
use archaeodash_parity::{
    assert_within_serialization, base_chem, golden_json, load_inaa, numeric_frame,
};

fn golden_f64s(value: &serde_json::Value) -> Vec<f64> {
    value
        .as_array()
        .expect("golden array")
        .iter()
        .map(|v| v.as_f64().expect("golden f64"))
        .collect()
}

/// Converts a golden row-major matrix (nested arrays) to component-major
/// columns and applies the canonical sign rule.
fn golden_columns(golden: &serde_json::Value) -> Vec<Vec<f64>> {
    let rows = golden.as_array().expect("golden rows");
    let k = rows[0].as_array().expect("golden row").len();
    (0..k)
        .map(|c| {
            rows.iter()
                .map(|r| r.as_array().unwrap()[c].as_f64().expect("f64"))
                .collect()
        })
        .collect()
}

#[test]
fn golden_06_pca_matches_prcomp() {
    let golden = golden_json("06_pca.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);
    let result = pca(&matrix, false).expect("pca");

    assert_eq!(golden["scale"].as_bool(), Some(false));
    assert!(result.scale.is_none());

    // sdev and center are sign-free: compare directly.
    let r_sdev = golden_f64s(&golden["sdev"]);
    assert_eq!(result.sdev.len(), r_sdev.len());
    for (i, g) in r_sdev.iter().enumerate() {
        assert_within_serialization(&format!("sdev[{i}]"), result.sdev[i], Some(*g));
    }
    let r_center = golden_f64s(&golden["center"]);
    assert_eq!(result.center.len(), r_center.len());
    for (j, g) in r_center.iter().enumerate() {
        assert_within_serialization(&format!("center[{j}]"), result.center[j], Some(*g));
    }

    // rotation: golden rows are variables, columns are components
    // (variable-major, like the R matrix as.numeric dump).
    let rotation_rows = golden["rotation"].as_array().expect("golden rotation");
    let p = rotation_rows.len();
    let k = result.sdev.len();
    assert_eq!(result.rotation.len(), p);
    let mut golden_rotation_cols: Vec<Vec<f64>> = (0..k)
        .map(|c| {
            rotation_rows
                .iter()
                .map(|row| row.as_array().unwrap()[c].as_f64().expect("f64"))
                .collect()
        })
        .collect();
    let flips = component_sign_flips(&golden_rotation_cols);
    apply_sign_flips(&mut golden_rotation_cols, &flips);
    for (c, golden_col) in golden_rotation_cols.iter().enumerate() {
        for (v, expected) in golden_col.iter().enumerate() {
            assert_within_serialization(
                &format!("rotation[{v}][{c}]"),
                result.rotation[v][c],
                Some(*expected),
            );
        }
    }

    // scores: 307 row objects keyed PC1..PCk, flipped with the same rule.
    let score_rows = golden["scores"].as_array().expect("golden scores");
    assert_eq!(result.scores.len(), score_rows.len(), "score row count");
    let mut golden_score_cols: Vec<Vec<f64>> = (0..k)
        .map(|c| {
            let name = format!("PC{}", c + 1);
            score_rows
                .iter()
                .map(|row| row.get(&name).expect("PC key").as_f64().expect("f64"))
                .collect()
        })
        .collect();
    apply_sign_flips(&mut golden_score_cols, &flips);
    for (c, golden_col) in golden_score_cols.iter().enumerate() {
        for (i, expected) in golden_col.iter().enumerate() {
            assert_within_serialization(
                &format!("scores[{i}][{c}]"),
                result.scores[i][c],
                Some(*expected),
            );
        }
    }
}
