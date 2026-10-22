//! Excel (`.xlsx`) source reader (Section 7.1), via Calamine.
//!
//! Produces the same all-text [`TextFrame`] as the CSV/TSV reader, so name
//! cleaning, ID/element defaults, and R-parity numeric coercion apply
//! unchanged. Cell conversion follows `readxl` + `as.character`:
//! - numbers use the R `as.character` double formatting (`r_format_double`),
//!   so `15587.9` stays `15587.9` and `104.0` becomes `104`;
//! - booleans become `TRUE` / `FALSE`;
//! - dates become `YYYY-MM-DD` (with ` HH:MM:SS` when there is a time part);
//! - empty cells, empty strings, and formula errors (`#N/A`, `#DIV/0!`, …)
//!   are missing.
//!
//! The first row of the sheet's used range is the header; blank header cells
//! are named `...N` like readxl. Rows with no values at all are dropped.

use std::path::Path;

use calamine::{open_workbook, Data, Reader, Xlsx};

use crate::loader::TextFrame;
use crate::rnum::r_format_double;
use crate::ImportError;

fn open(path: &Path) -> Result<Xlsx<std::io::BufReader<std::fs::File>>, ImportError> {
    open_workbook(path).map_err(|e| ImportError::Parse(format!("cannot read Excel workbook: {e}")))
}

/// Worksheet names in workbook order.
pub fn xlsx_sheet_names(path: &Path) -> Result<Vec<String>, ImportError> {
    Ok(open(path)?.sheet_names().to_vec())
}

fn cell_text(cell: &Data) -> Option<String> {
    match cell {
        Data::Empty | Data::Error(_) => None,
        Data::String(s) => (!s.is_empty()).then(|| s.clone()),
        Data::Int(i) => Some(i.to_string()),
        Data::Float(f) => Some(r_format_double(*f)),
        Data::Bool(b) => Some(if *b { "TRUE" } else { "FALSE" }.to_string()),
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(value) => {
                let time = value.format("%H:%M:%S").to_string();
                Some(if time == "00:00:00" {
                    value.format("%Y-%m-%d").to_string()
                } else {
                    value.format("%Y-%m-%d %H:%M:%S").to_string()
                })
            }
            None => Some(r_format_double(dt.as_f64())),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => (!s.is_empty()).then(|| s.clone()),
    }
}

/// Reads one worksheet as text. `sheet = None` picks the first sheet that
/// has any cells; a named sheet must exist.
///
/// Returns the frame and the name of the sheet that was read.
pub fn read_xlsx_table(
    path: &Path,
    sheet: Option<&str>,
) -> Result<(TextFrame, String), ImportError> {
    let mut workbook = open(path)?;
    let names = workbook.sheet_names().to_vec();
    if names.is_empty() {
        return Err(ImportError::Parse(
            "the Excel workbook has no worksheets".into(),
        ));
    }
    let (name, range) = match sheet {
        Some(wanted) => {
            if !names.iter().any(|n| n == wanted) {
                return Err(ImportError::Parse(format!(
                    "worksheet {wanted:?} not found; available: {}",
                    names.join(", ")
                )));
            }
            let range = workbook.worksheet_range(wanted).map_err(|e| {
                ImportError::Parse(format!("cannot read worksheet {wanted:?}: {e}"))
            })?;
            (wanted.to_string(), range)
        }
        None => {
            let mut found = None;
            for name in &names {
                let range = workbook.worksheet_range(name).map_err(|e| {
                    ImportError::Parse(format!("cannot read worksheet {name:?}: {e}"))
                })?;
                if !range.is_empty() {
                    found = Some((name.clone(), range));
                    break;
                }
            }
            found.ok_or_else(|| {
                ImportError::Parse("every worksheet in the workbook is empty".into())
            })?
        }
    };

    let mut rows = range.rows();
    let header = rows
        .next()
        .ok_or_else(|| ImportError::Parse(format!("worksheet {name:?} is empty")))?;
    let columns: Vec<String> = header
        .iter()
        .enumerate()
        .map(|(i, cell)| cell_text(cell).unwrap_or_else(|| format!("...{}", i + 1)))
        .collect();
    let mut out = Vec::new();
    for row in rows {
        let values: Vec<Option<String>> = (0..columns.len())
            .map(|i| row.get(i).and_then(cell_text))
            .collect();
        if values.iter().any(Option::is_some) {
            out.push(values);
        }
    }
    Ok((TextFrame { columns, rows: out }, name))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

    fn workbook(path: &Path) {
        let mut book = Workbook::new();
        let blank = book.add_worksheet();
        blank.set_name("Notes").expect("name");
        let data = book.add_worksheet();
        data.set_name("INAA").expect("name");
        // Header with a blank cell (column 4).
        data.write_string(0, 0, "ANID").expect("w");
        data.write_string(0, 1, "Site").expect("w");
        data.write_string(0, 2, "Fe").expect("w");
        data.write_string(0, 4, "Date").expect("w");
        data.write_string(1, 0, "AID1").expect("w");
        data.write_string(1, 1, "Baca").expect("w");
        data.write_number(1, 2, 15587.9).expect("w");
        data.write_boolean(1, 3, true).expect("w");
        let date = ExcelDateTime::from_ymd(2024, 3, 9).expect("date");
        let fmt = Format::new().set_num_format("yyyy-mm-dd");
        data.write_datetime_with_format(1, 4, &date, &fmt)
            .expect("w");
        // Row 2 left completely empty, row 3 has an integer-valued float.
        data.write_string(3, 0, "AID2").expect("w");
        data.write_number(3, 2, 104.0).expect("w");
        data.write_formula(3, 1, "=1/0").expect("w");
        let other = book.add_worksheet();
        other.set_name("Other").expect("name");
        other.write_string(0, 0, "x").expect("w");
        other.write_number(1, 0, 1.5).expect("w");
        book.save(path).expect("save");
    }

    #[test]
    fn reads_first_nonempty_sheet_as_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("book.xlsx");
        workbook(&path);
        assert_eq!(
            xlsx_sheet_names(&path).expect("names"),
            vec!["Notes", "INAA", "Other"]
        );
        let (frame, sheet) = read_xlsx_table(&path, None).expect("read");
        assert_eq!(sheet, "INAA");
        assert_eq!(frame.columns, vec!["ANID", "Site", "Fe", "...4", "Date"]);
        // The blank row is dropped.
        assert_eq!(frame.rows.len(), 2);
        assert_eq!(
            frame.rows[0],
            vec![
                Some("AID1".into()),
                Some("Baca".into()),
                Some("15587.9".into()),
                Some("TRUE".into()),
                Some("2024-03-09".into()),
            ]
        );
        // Integer-valued floats print without ".0"; a formula without a
        // cached result reads as missing.
        assert_eq!(frame.rows[1][0].as_deref(), Some("AID2"));
        assert_eq!(frame.rows[1][2].as_deref(), Some("104"));
    }

    #[test]
    fn reads_a_named_sheet_and_rejects_unknown_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("book.xlsx");
        workbook(&path);
        let (frame, sheet) = read_xlsx_table(&path, Some("Other")).expect("read");
        assert_eq!(sheet, "Other");
        assert_eq!(frame.columns, vec!["x"]);
        assert_eq!(frame.rows, vec![vec![Some("1.5".to_string())]]);
        let err = read_xlsx_table(&path, Some("Missing")).expect_err("unknown sheet");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn rejects_files_that_are_not_workbooks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("fake.xlsx");
        std::fs::write(&path, b"ANID,Fe\nA,1\n").expect("write");
        let err = read_xlsx_table(&path, None).expect_err("not a workbook");
        assert!(err.to_string().contains("Excel"), "{err}");
    }
}
