//! Golden procedure 8 (class T): LDA against the `MASS:::lda.default`
//! moment-method oracle (default priors = level proportions, `tol = 1e-4`)
//! and the capture-script score convention, on the INAA fixture. Sign
//! canonicalization matches the PCA golden test.

use archaeodash_analysis::lda;
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

#[test]
fn golden_08_lda_matches_mass_moment() {
    let golden = golden_json("08_lda.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);
    let groups: Vec<String> = frame
        .column("CORE")
        .expect("CORE group column")
        .into_iter()
        .map(|label| label.expect("complete CORE column").to_string())
        .collect();

    let result = lda(&matrix, &groups, 3).expect("lda");
    assert!(result.warnings.is_empty());

    // Priors in sorted-level order (class T: tolerance comparison).
    let r_prior = golden_f64s(&golden["prior"]);
    assert_eq!(result.prior.len(), r_prior.len());
    for (g, expected) in r_prior.iter().enumerate() {
        assert_within_serialization(&format!("prior[{g}]"), result.prior[g], Some(*expected));
    }

    // Group means, level-major rows.
    let means_rows = golden["means"].as_array().expect("golden means");
    assert_eq!(result.means.len(), means_rows.len());
    for (g, row) in means_rows.iter().enumerate() {
        let expected = golden_f64s(row);
        assert_eq!(result.means[g].len(), expected.len());
        for (j, value) in expected.iter().enumerate() {
            assert_within_serialization(
                &format!("means[{g}][{j}]"),
                result.means[g][j],
                Some(*value),
            );
        }
    }

    // Scaling: variable-major rows, canonicalized per LD column.
    let scaling_rows = golden["scaling"].as_array().expect("golden scaling");
    let rank = result.svd.len();
    assert_eq!(rank, scaling_rows[0].as_array().unwrap().len());
    let mut golden_scaling_cols: Vec<Vec<f64>> = (0..rank)
        .map(|c| {
            scaling_rows
                .iter()
                .map(|row| row.as_array().unwrap()[c].as_f64().expect("f64"))
                .collect()
        })
        .collect();
    let flips = archaeodash_analysis::component_sign_flips(&golden_scaling_cols);
    archaeodash_analysis::apply_sign_flips(&mut golden_scaling_cols, &flips);
    for (c, golden_col) in golden_scaling_cols.iter().enumerate() {
        for (v, expected) in golden_col.iter().enumerate() {
            assert_within_serialization(
                &format!("scaling[{v}][{c}]"),
                result.scaling[v][c],
                Some(*expected),
            );
        }
    }

    // Singular values are sign-free.
    let r_svd = golden_f64s(&golden["svd"]);
    assert_eq!(result.svd.len(), r_svd.len());
    for (k, expected) in r_svd.iter().enumerate() {
        assert_within_serialization(&format!("svd[{k}]"), result.svd[k], Some(*expected));
    }

    // Scores: 307 row objects keyed LD1..LDk.
    let score_rows = golden["scores"].as_array().expect("golden scores");
    assert_eq!(result.scores.len(), score_rows.len(), "score row count");
    assert_eq!(result.score_names.len(), rank);
    let mut golden_score_cols: Vec<Vec<f64>> = (0..rank)
        .map(|c| {
            let name = format!("LD{}", c + 1);
            score_rows
                .iter()
                .map(|row| row.get(&name).expect("LD key").as_f64().expect("f64"))
                .collect()
        })
        .collect();
    archaeodash_analysis::apply_sign_flips(&mut golden_score_cols, &flips);
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

#[test]
fn golden_08_lda_rejects_under_three_groups() {
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);
    // Collapse the five CORE levels to two: the legacy validate_lda_groups
    // gate (min_groups = 3) must fire with its exact message shape.
    let groups: Vec<String> = frame
        .column("CORE")
        .expect("CORE group column")
        .into_iter()
        .map(|label| match label.expect("complete CORE column") {
            "D1" => "D1".to_string(),
            _ => "D2".to_string(),
        })
        .collect();
    let err = lda(&matrix, &groups, 3).expect_err("two groups rejected");
    assert!(err
        .to_string()
        .contains("LDA requires at least 3 groups. Current selection has 2."));
}
