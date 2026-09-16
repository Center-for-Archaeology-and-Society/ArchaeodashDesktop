//! Golden #14 — measured-data export round trip (class E).
//!
//! The oracle exports the loaded INAA fixture through `rio::export` to CSV
//! (`saveexportTab.R` chemical-data path) and re-imports it. Our writer must
//! reproduce the exported bytes exactly and the re-imported cells must match
//! the source frame cell-for-cell.

use archaeodash_data_io::{data_loader, read_text_table, write_text_csv};
use archaeodash_parity::{fixture, golden_json};
use sha2::{Digest, Sha256};

#[test]
fn golden_14_measured_data_export_round_trip() {
    // Manifest keeps the registry entry (id, parity class, artifact hash).
    let manifest = golden_json("manifest.json");
    let proc = manifest["procedures"]
        .as_array()
        .expect("procedures array")
        .iter()
        .find(|p| p["id"] == 14)
        .expect("procedure 14 in manifest")
        .clone();
    assert_eq!(proc["parity_class"], "E");

    // Per-procedure artifact holds the captured expectations.
    let golden = golden_json("14_measured_data_export_round_trip.json");
    let expected_sha = golden["export_sha256"].as_str().expect("export_sha256");
    let source_rows = golden["source_rows"].as_u64().expect("source_rows");
    let round_trip_rows = golden["round_trip_rows"].as_u64().expect("round_trip_rows");
    let golden_columns: Vec<String> = golden["source_columns"]
        .as_array()
        .expect("source_columns")
        .iter()
        .map(|c| c.as_str().expect("column name").to_string())
        .collect();

    // Load the fixture exactly like the oracle `dataLoader` call.
    let frame = data_loader(&fixture("INAA_test.csv")).expect("fixture loads");
    assert_eq!(frame.n_rows() as u64, source_rows, "source row count");
    assert_eq!(frame.columns, golden_columns, "source column order");

    // Export through the new writer and compare bytes with the golden file.
    let out_dir = tempfile::tempdir().expect("tempdir");
    let out_path = out_dir.path().join("14_measured_data_export.csv");
    write_text_csv(&out_path, &frame).expect("export writes");

    let golden_bytes = std::fs::read(fixture("golden/14_measured_data_export.csv"))
        .expect("golden export csv exists");
    let written_bytes = std::fs::read(&out_path).expect("written export csv");
    let mut hasher = Sha256::new();
    hasher.update(&written_bytes);
    let digest = format!("{:x}", hasher.finalize());
    assert_eq!(
        digest, expected_sha,
        "export bytes must match the rio::export oracle byte-for-byte"
    );
    assert_eq!(written_bytes, golden_bytes, "literal byte equality");

    // Round trip: re-import and confirm cell text equality like the oracle.
    let reread = read_text_table(&out_path).expect("re-import");
    assert_eq!(reread.n_rows() as u64, round_trip_rows, "round-trip rows");
    assert_eq!(reread.columns, golden_columns, "round-trip columns");
    assert_eq!(reread.rows, frame.rows, "cell_text_equal");
}
