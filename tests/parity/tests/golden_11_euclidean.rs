//! Golden procedure 11 (class E): Euclidean nearest matches against the
//! legacy `calcEDistance` oracle on the same `membership_df` frame as
//! procedure 10 (first four chem columns, `CORE` grouping, `anid` IDs,
//! `limit = 5`, `withinGroup = FALSE`). Distances are compared within the
//! jsonlite 4-decimal serialization tolerance; keys and group labels must
//! match exactly, including row order.

use archaeodash_analysis::calc_e_distance;
use archaeodash_data_io::{data_loader, default_chem_columns, default_id_column};
use archaeodash_parity::{assert_within_serialization, fixture, golden_json, numeric_frame};

#[test]
fn golden_11_euclidean_matches_r() {
    let golden = golden_json("11_euclidean_nearest_matches.json");
    let frame = data_loader(&fixture("INAA_test.csv")).expect("fixture loads");
    let id_col = default_id_column(&frame.columns).expect("anid column");
    let group_col = "CORE";
    let membership_chem: Vec<String> = default_chem_columns(&frame.columns)
        .into_iter()
        .take(4)
        .collect();
    let matrix = numeric_frame(&frame, &membership_chem);
    let rowids: Vec<String> = frame
        .column("rowid")
        .expect("rowid column")
        .into_iter()
        .map(|cell| cell.unwrap_or_default().to_string())
        .collect();
    let ids: Vec<String> = frame
        .column(&id_col)
        .expect("id column")
        .into_iter()
        .map(|cell| cell.unwrap_or_default().to_string())
        .collect();
    let groups: Vec<String> = frame
        .column(group_col)
        .expect("group column")
        .into_iter()
        .map(|cell| cell.unwrap_or_default().to_string())
        .collect();

    // projection = eligible groups from procedure 10.
    let eligible = archaeodash_analysis::get_eligible(&groups, membership_chem.len());
    let matches =
        calc_e_distance(&rowids, &ids, &groups, &matrix, &eligible, 5, false).expect("matches");

    let rows = golden.as_array().expect("golden rows");
    assert_eq!(rows.len(), matches.len(), "match count");
    for (i, (gr, m)) in rows.iter().zip(matches.iter()).enumerate() {
        assert_eq!(
            gr["rowid"].as_str().expect("rowid"),
            m.rowid,
            "row {i} rowid"
        );
        assert_eq!(gr["anid"].as_str().expect("anid"), m.id, "row {i} anid");
        assert_eq!(
            gr["match"].as_str().expect("match"),
            m.match_id,
            "row {i} match"
        );
        assert_eq!(gr["CORE"].as_str().expect("CORE"), m.group, "row {i} CORE");
        assert_eq!(
            gr["CORE_match"].as_str().expect("CORE_match"),
            m.match_group,
            "row {i} CORE_match"
        );
        assert_within_serialization(
            &format!("row {i} distance"),
            m.distance,
            Some(gr["distance"].as_f64().expect("distance")),
        );
    }
}
