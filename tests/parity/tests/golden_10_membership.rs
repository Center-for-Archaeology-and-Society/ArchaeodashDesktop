//! Golden procedure 10 (class T): group membership probabilities against the
//! legacy R oracle — `getEligible` plus `group.mem.probs` for both the
//! Hotelling T2 and Mahalanobis paths on the INAA fixture with the first four
//! chem columns (`as`, `la`, `lu`, `nd`) and the `CORE` grouping. The oracle
//! feeds `membership_df = bind_cols(loaded[, c("rowid", id, group)],
//! numeric_frame(loaded, membership_chem))` (capture_baselines.R).

use archaeodash_analysis::{get_eligible, group_mem_probs, MembershipMethod, MembershipTable};
use archaeodash_data_io::{data_loader, default_chem_columns, default_id_column};
use archaeodash_parity::{assert_within_serialization, fixture, golden_json, numeric_frame};

/// Golden cells are jsonlite 4-decimal serialisations of values already
/// rounded to `round(5) * 100` (Hotellings) or raw distances (Mahalanobis);
/// NaN/Inf serialize as null.
fn compare_membership(key: &str, table: &MembershipTable) {
    let golden = golden_json("10_membership_probabilities.json");
    let rows = golden[key].as_array().expect("golden rows");
    assert_eq!(rows.len(), table.rows.len(), "{key} row count");
    for (i, (gr, mr)) in rows.iter().zip(table.rows.iter()).enumerate() {
        assert_eq!(gr["ID"].as_str().expect("ID"), mr.id, "{key} row {i} ID");
        assert_eq!(
            gr["Group"].as_str().expect("Group"),
            table.group,
            "{key} row {i} Group"
        );
        assert_eq!(
            gr["GroupVal"].as_str().expect("GroupVal"),
            mr.group_val,
            "{key} row {i} GroupVal"
        );
        match gr["BestGroup"].as_str() {
            Some(g) => assert_eq!(Some(g), mr.best_group.as_deref(), "{key} row {i} BestGroup"),
            None => assert!(mr.best_group.is_none(), "{key} row {i} BestGroup"),
        }
        let g_val = gr["BestValue"].as_f64();
        let m_val = mr.best_value.filter(|v| v.is_finite());
        match (g_val, m_val) {
            (None, None) => {}
            (Some(g), Some(m)) => {
                assert_within_serialization(&format!("{key} row {i} BestValue"), m, Some(g))
            }
            (g, m) => panic!("{key} row {i} BestValue null mismatch: golden {g:?}, mine {m:?}"),
        }
        let in_group = gr["InGroup"].as_bool();
        assert_eq!(in_group, mr.in_group, "{key} row {i} InGroup");
        for (j, group_name) in table.eligible.iter().enumerate() {
            let g = gr[group_name].as_f64();
            let m = mr.probs[j];
            let m = if m.is_finite() { Some(m) } else { None };
            match (g, m) {
                (None, None) => {}
                (Some(g), Some(m)) => assert_within_serialization(
                    &format!("{key} row {i} column {group_name}"),
                    m,
                    Some(g),
                ),
                (g, m) => panic!(
                    "{key} row {i} column {group_name} null mismatch: golden {g:?}, mine {m:?}"
                ),
            }
        }
    }
}

#[test]
fn golden_10_membership_matches_r() {
    let golden = golden_json("10_membership_probabilities.json");
    let frame = data_loader(&fixture("INAA_test.csv")).expect("fixture loads");
    let id_col = default_id_column(&frame.columns).expect("anid column");
    let group_col = "CORE";
    let membership_chem: Vec<String> = default_chem_columns(&frame.columns)
        .into_iter()
        .take(4)
        .collect();
    let chem = numeric_frame(&frame, &membership_chem);
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

    // eligibility: dplyr count order (sorted keys) with n > max(nc, ng) + 1.
    let eligible = get_eligible(&groups, membership_chem.len());
    let golden_eligible: Vec<String> = golden["eligibility"]
        .as_array()
        .expect("eligibility")
        .iter()
        .map(|v| v.as_str().expect("group string").to_string())
        .collect();
    assert_eq!(eligible, golden_eligible, "eligible groups");

    let hotelling = group_mem_probs(
        &ids,
        &groups,
        group_col,
        &chem,
        &membership_chem,
        &eligible,
        MembershipMethod::Hotellings,
    )
    .expect("hotelling table");
    let mahalanobis = group_mem_probs(
        &ids,
        &groups,
        group_col,
        &chem,
        &membership_chem,
        &eligible,
        MembershipMethod::Mahalanobis,
    )
    .expect("mahalanobis table");
    compare_membership("hotelling", &hotelling);
    compare_membership("mahalanobis", &mahalanobis);
}
