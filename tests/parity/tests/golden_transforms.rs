//! Golden procedures 2-4 (class E): zScore, log transforms, ratio application.
//! Oracle entry points: zScore.R, datainputTab.R transform/ratio blocks.

use archaeodash_analysis::{
    apply_ratios, log_transform, z_score, ColumnMatrix, LogBase, RatioSpec,
};
use archaeodash_data_io::loader::numeric_column;
use archaeodash_parity::{
    assert_exact, assert_within_serialization, golden_column, golden_json, load_inaa,
};
use serde_json::Value;

/// The oracle's base_chem: first 8 default chem columns.
fn base_chem(frame: &archaeodash_data_io::TextFrame) -> Vec<String> {
    let chem = archaeodash_data_io::default_chem_columns(&frame.columns);
    chem.into_iter().take(8).collect()
}

/// Builds the numeric frame exactly like `numeric_frame(loaded, columns)`:
/// as.numeric(as.character(x)) per column, NA for unparseable.
fn numeric_frame(frame: &archaeodash_data_io::TextFrame, columns: &[String]) -> ColumnMatrix {
    let cols = columns
        .iter()
        .map(|c| numeric_column(frame, c).expect("column exists"))
        .collect();
    ColumnMatrix {
        names: columns.to_vec(),
        cols,
    }
}

fn golden_rows(golden: &serde_json::Value) -> Vec<Value> {
    golden.as_array().expect("golden row array").to_vec()
}

#[test]
fn golden_02_zscore() {
    let golden = golden_json("02_zscore.json");
    let rows = golden_rows(&golden);
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);
    let z = z_score(&matrix).expect("zscore");

    assert_eq!(z.n_rows(), rows.len(), "row count");
    for (j, name) in columns.iter().enumerate() {
        let expected = golden_column(&rows, name);
        for i in 0..z.n_rows() {
            assert_exact(&format!("zscore[{name}][{i}]"), z.cols[j][i], expected[i]);
        }
    }
}

#[test]
fn golden_03_log_transforms() {
    let golden = golden_json("03_log_transforms.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);

    for (key, base) in [("log10", LogBase::Base10), ("log", LogBase::Natural)] {
        let result = log_transform(&matrix, base).expect("log transform");
        let block = &golden[key];
        assert_eq!(
            result.non_finite_to_zero,
            block["non_finite_to_zero"].as_u64().unwrap(),
            "{key} non-finite count"
        );
        let rows = golden_rows(&block["values"]);
        assert_eq!(result.matrix.n_rows(), rows.len());
        for (j, name) in columns.iter().enumerate() {
            let expected = golden_column(&rows, name);
            for i in 0..result.matrix.n_rows() {
                assert_exact(
                    &format!("{key}[{name}][{i}]"),
                    result.matrix.cols[j][i],
                    expected[i],
                );
            }
        }
    }
}

#[test]
fn golden_04_ratios() {
    let golden = golden_json("04_ratio_construction_and_application.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let mut matrix = numeric_frame(&frame, &columns);

    // Oracle modifications: ratio_input$la[[1]] <- 0; ratio_input$nd[[2]] <- NA.
    let la = matrix.names.iter().position(|n| n == "la").unwrap();
    let nd = matrix.names.iter().position(|n| n == "nd").unwrap();
    matrix.cols[la][0] = 0.0;
    matrix.cols[nd][1] = f64::NAN;

    let specs: Vec<RatioSpec> = golden["specs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| RatioSpec {
            output_name: s["ratio"].as_str().unwrap().to_string(),
            numerator: s["numerator"].as_str().unwrap().to_string(),
            denominator: s["denominator"].as_str().unwrap().to_string(),
        })
        .collect();

    let out = apply_ratios(&matrix, &specs).expect("ratios apply");
    let rows = golden_rows(&golden["values"]);
    assert_eq!(out.n_rows(), rows.len());
    assert_eq!(out.names.len(), columns.len() + specs.len());

    for (j, name) in out.names.iter().enumerate() {
        let expected = golden_column(&rows, name);
        for i in 0..out.n_rows() {
            if j < columns.len() {
                // Input columns: raw values, serialized at 4 decimals.
                assert_within_serialization(
                    &format!("ratio_input[{name}][{i}]"),
                    out.cols[j][i],
                    expected[i],
                );
            } else {
                // Ratio outputs: null denominator rule plus 4-decimal serialization.
                assert_within_serialization(
                    &format!("ratio[{name}][{i}]"),
                    out.cols[j][i],
                    expected[i],
                );
            }
        }
    }
}
