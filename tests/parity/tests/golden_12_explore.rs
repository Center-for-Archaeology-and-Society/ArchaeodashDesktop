//! Golden procedure 12 (class E): Explore views against the R 4.6.1 oracle
//! on the INAA fixture - `profile_missing` band thresholds, `hist.default`
//! breakpoints/counts for the first base-chem column (`breaks = 30`), and the
//! `comp.profile` `pivot_longer` row-major long table.

use archaeodash_analysis::{compositional_profile, histogram, missing_profile};
use archaeodash_parity::{
    assert_exact, assert_within_serialization, base_chem, golden_json, load_inaa, numeric_frame,
};

#[test]
fn golden_12_explore_views_match_r_oracle() {
    let golden = golden_json("12_explore_views.json");
    let frame = load_inaa();
    let columns = base_chem(&frame);
    let matrix = numeric_frame(&frame, &columns);

    // Missing profile: feature order (descending missing count, stable ties),
    // counts, percentages, and the cut() band labels.
    let golden_mp = golden["missing_profile"].as_array().expect("missing rows");
    let rows = missing_profile(&matrix);
    assert_eq!(rows.len(), golden_mp.len(), "missing-profile row count");
    for (i, (mine, g)) in rows.iter().zip(golden_mp).enumerate() {
        assert_eq!(
            mine.feature,
            g["feature"].as_str().expect("feature"),
            "feature[{i}]"
        );
        assert_exact(
            &format!("num_missing[{i}]"),
            mine.num_missing as f64,
            Some(g["num_missing"].as_f64().expect("count")),
        );
        assert_within_serialization(
            &format!("pct_missing[{i}]"),
            mine.pct_missing,
            g["pct_missing"].as_f64(),
        );
        assert_eq!(
            mine.band.label(),
            g["Band"].as_str().expect("band"),
            "band[{i}]"
        );
    }

    // Histogram: hist(features[[1]], breaks = 30, plot = FALSE) - pretty
    // breakpoints with the fuzzy-edge right-closed bin count.
    let golden_breaks: Vec<f64> = golden["histogram"]["breaks"]
        .as_array()
        .expect("breaks")
        .iter()
        .map(|v| v.as_f64().expect("break f64"))
        .collect();
    let golden_counts: Vec<u64> = golden["histogram"]["counts"]
        .as_array()
        .expect("counts")
        .iter()
        .map(|v| v.as_u64().expect("count u64"))
        .collect();
    let (breaks, counts) = histogram(&matrix.cols[0], 30).expect("histogram");
    assert_eq!(breaks.len(), golden_breaks.len(), "break count");
    assert_eq!(counts.len(), golden_counts.len(), "bin count");
    for (i, (mine, g)) in breaks.iter().zip(golden_breaks.iter()).enumerate() {
        assert_exact(&format!("breaks[{i}]"), *mine, Some(*g));
    }
    assert_eq!(counts, golden_counts, "histogram counts");

    // Compositional profile: pivot_longer row-major, rowid then elements in
    // column order; values serialized at jsonlite's 4-decimal default.
    let golden_cp = golden["compositional_profile"]
        .as_array()
        .expect("profile rows");
    let rows = compositional_profile(&matrix, None).expect("profile");
    assert_eq!(rows.len(), golden_cp.len(), "profile row count");
    let golden_values = archaeodash_parity::golden_column(golden_cp, "value");
    for (i, (mine, g)) in rows.iter().zip(golden_cp).enumerate() {
        assert_exact(
            &format!("rowid[{i}]"),
            mine.rowid as f64,
            Some(g["rowid"].as_f64().expect("rowid")),
        );
        assert_eq!(
            mine.element,
            g["element"].as_str().expect("element"),
            "element[{i}]"
        );
        assert_within_serialization(&format!("value[{i}]"), mine.value, golden_values[i]);
    }
}
