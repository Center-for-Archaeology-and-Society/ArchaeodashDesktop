//! Golden procedure 1 (class E): CSV import, numeric inference, group
//! partitions. Oracle entry points: DataLoader.R, columnTypeHints.R.

use archaeodash_data_io::{
    data_loader, default_chem_columns, default_id_column, group_partitions,
    guess_numeric_columns_fast,
};
use archaeodash_parity::{fixture, golden_json};

#[test]
fn golden_01_csv_import_and_numeric_inference() {
    let golden = golden_json("01_csv_import_and_numeric_inference.json");
    let frame = data_loader(&fixture("INAA_test.csv")).expect("fixture");

    // Column order after clean_names(case="none") + rowid prepend.
    let expected_columns: Vec<String> = golden["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(frame.columns, expected_columns, "cleaned column order");

    assert_eq!(
        frame.n_rows(),
        golden["row_count"].as_u64().unwrap() as usize
    );

    assert_eq!(
        default_id_column(&frame.columns).as_deref(),
        golden["id_column"].as_str()
    );

    let chem = default_chem_columns(&frame.columns);
    let expected_chem: Vec<String> = golden["default_chem_columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(chem, expected_chem, "default chem selection");

    let numeric_like = guess_numeric_columns_fast(&frame, &["rowid"], 1500, 0.95);
    let expected_numeric: Vec<String> = golden["numeric_like_columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(numeric_like, expected_numeric, "1500-row / 95 percent rule");

    let partitions = group_partitions(&frame, "CORE").expect("no blank groups");
    let expected_partitions = golden["group_partitions"].as_array().unwrap();
    assert_eq!(partitions.len(), expected_partitions.len(), "group count");
    for ((name, count), expected) in partitions.iter().zip(expected_partitions) {
        assert_eq!(name, expected["CORE"].as_str().unwrap(), "partition order");
        assert_eq!(
            *count,
            expected["row_count"].as_u64().unwrap() as usize,
            "partition {name}"
        );
    }
}
