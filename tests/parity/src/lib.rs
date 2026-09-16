//! Golden parity harness helpers shared by the procedure tests.
#![allow(clippy::expect_used)] // test-support code; a malformed golden is fatal

use archaeodash_data_io::TextFrame;
use serde_json::Value;
use std::path::PathBuf;

/// Repository-root-relative fixture paths resolve from the crate directory.
pub fn fixture(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // tests/
    path.pop(); // repo root
    path.join("fixtures").join(name)
}

pub fn golden_json(name: &str) -> Value {
    let path = fixture(&format!("golden/{name}"));
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("golden file: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("golden JSON: {e}"))
}

/// Loads the canonical INAA fixture through the Rust data loader.
pub fn load_inaa() -> TextFrame {
    archaeodash_data_io::data_loader(&fixture("INAA_test.csv")).expect("fixture loads")
}

/// Extracts one column of a golden row-object array as f64s (null -> NaN).
#[allow(clippy::expect_used)] // test-support code; a malformed golden is fatal
pub fn golden_column(rows: &[Value], name: &str) -> Vec<Option<f64>> {
    rows.iter()
        .map(|row| match row.get(name).expect("golden column") {
            Value::Null => None,
            Value::Number(n) => Some(n.as_f64().expect("f64")),
            other => panic!("unexpected golden value {other:?}"),
        })
        .collect()
}

/// Class-E exact assertion with NaN <-> null equivalence. jsonlite wrote the
/// goldens at 4 decimal digits; class-E outputs are rounded to 3 decimals, so
/// parsed golden values are exact at that precision.
#[allow(clippy::expect_used)] // test-support code; a malformed golden is fatal
pub fn assert_exact(name: &str, mine: f64, golden: Option<f64>) {
    match golden {
        None => assert!(mine.is_nan(), "{name}: expected null, got {mine}"),
        Some(g) => assert!(
            (g - mine).abs() <= 1e-12,
            "{name}: mine {mine} vs golden {g}"
        ),
    }
}

/// Class-E comparison for raw-value columns the oracle serialized at jsonlite's
/// 4-decimal default: half-of-last-digit tolerance.
#[allow(clippy::expect_used)] // test-support code; a malformed golden is fatal
pub fn assert_within_serialization(name: &str, mine: f64, golden: Option<f64>) {
    match golden {
        None => assert!(mine.is_nan(), "{name}: expected null, got {mine}"),
        Some(g) => assert!(
            (g - mine).abs() <= 5.1e-5,
            "{name}: mine {mine} vs golden {g}"
        ),
    }
}
