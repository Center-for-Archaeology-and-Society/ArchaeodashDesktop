//! Source import and the legacy `dataLoader` port (golden #1 semantics).
//!
//! Reads CSV/TSV with the dedicated text reader and XLSX through Calamine
//! (Section 7.1), always into an all-text frame; then applies
//! `clean_names(case = "none")`, drops any incoming `rowid`, and prepends a
//! 1-based `rowid` column exactly like the R oracle.

use std::collections::BTreeMap;
use std::path::Path;

use crate::clean_names::clean_names_case_none;
use crate::rnum::parse_r_numeric;
use crate::ImportError;

/// A fully textual frame: every cell is raw text or absent (empty field).
///
/// Mirrors the oracle's `as.character` frame so numeric inference and later
/// coercion see identical input.
#[derive(Debug, Clone, PartialEq)]
pub struct TextFrame {
    pub columns: Vec<String>,
    /// Row-major cells; `None` marks an empty/absent field.
    pub rows: Vec<Vec<Option<String>>>,
}

impl TextFrame {
    pub fn n_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    /// Column values as `Option<&str>` in row order.
    pub fn column(&self, name: &str) -> Option<Vec<Option<&str>>> {
        let idx = self.column_index(name)?;
        Some(self.rows.iter().map(|r| r[idx].as_deref()).collect())
    }
}

/// Supported source formats (Section 7.1 allowlist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Csv,
    Tsv,
    Xlsx,
}

impl SourceFormat {
    /// Format from a file extension (case-insensitive): `csv`, `tsv`, `xlsx`.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "csv" => Some(Self::Csv),
            "tsv" => Some(Self::Tsv),
            "xlsx" => Some(Self::Xlsx),
            _ => None,
        }
    }

    /// Format from a path's extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        Self::from_extension(path.extension()?.to_str()?)
    }
}

/// A loaded source: the `dataLoader` frame plus, for workbooks, the sheet
/// that was read and every sheet name.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSource {
    pub frame: TextFrame,
    pub sheet: Option<String>,
    pub sheets: Vec<String>,
}

/// `dataLoader` for any supported format. `sheet` selects a worksheet in an
/// XLSX workbook (default: the first non-empty sheet) and is ignored for
/// text formats.
pub fn load_source(
    path: &Path,
    format: SourceFormat,
    sheet: Option<&str>,
) -> Result<LoadedSource, ImportError> {
    let (frame, sheet, sheets) = match format {
        SourceFormat::Csv => (read_delimited_table(path, b',')?, None, Vec::new()),
        SourceFormat::Tsv => (read_delimited_table(path, b'\t')?, None, Vec::new()),
        SourceFormat::Xlsx => {
            let (frame, used) = crate::excel::read_xlsx_table(path, sheet)?;
            (frame, Some(used), crate::excel::xlsx_sheet_names(path)?)
        }
    };
    Ok(LoadedSource {
        frame: finish_data_loader(frame),
        sheet,
        sheets,
    })
}

/// Reads a delimited text file with the fixture/oracle conventions:
/// header row present, flexible raggedness tolerated, empty fields -> `None`.
pub fn read_text_table(path: &Path) -> Result<TextFrame, ImportError> {
    read_delimited_table(path, b',')
}

fn read_delimited_table(path: &Path, delimiter: u8) -> Result<TextFrame, ImportError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .delimiter(delimiter)
        .from_path(path)
        .map_err(|e| ImportError::Parse(format!("cannot read {}: {e}", path.display())))?;
    let columns: Vec<String> = reader
        .headers()
        .map_err(|e| ImportError::Parse(format!("cannot read header: {e}")))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| ImportError::Parse(format!("bad record: {e}")))?;
        let mut row: Vec<Option<String>> = Vec::with_capacity(columns.len());
        for i in 0..columns.len() {
            match record.get(i) {
                // Empty fields and short rows both become absent cells.
                Some(s) if !s.is_empty() => row.push(Some(s.to_string())),
                _ => row.push(None),
            }
        }
        rows.push(row);
    }
    Ok(TextFrame { columns, rows })
}

/// The legacy INAA elemental list (`DataLoader.R::common_inaa_elements`).
pub const COMMON_INAA_ELEMENTS: [&str; 33] = [
    "as", "la", "lu", "nd", "sm", "u", "yb", "ce", "co", "cr", "cs", "eu", "fe", "hf", "ni", "rb",
    "sb", "sc", "sr", "ta", "tb", "th", "zn", "zr", "al", "ba", "ca", "dy", "k", "mn", "na", "ti",
    "v",
];

/// `dataLoader`: clean names, drop incoming `rowid`, prepend 1-based `rowid`.
/// The format comes from the extension (CSV when there is none).
pub fn data_loader(path: &Path) -> Result<TextFrame, ImportError> {
    let format = SourceFormat::from_path(path).unwrap_or(SourceFormat::Csv);
    Ok(load_source(path, format, None)?.frame)
}

fn finish_data_loader(mut frame: TextFrame) -> TextFrame {
    frame.columns = clean_names_case_none(&frame.columns);
    if let Some(pos) = frame.column_index("rowid") {
        frame.columns.remove(pos);
        for row in &mut frame.rows {
            row.remove(pos);
        }
    }
    // rowid_to_column prepends 1-based indices as an integer column; the
    // oracle frame holds them as character.
    frame.columns.insert(0, "rowid".to_string());
    for (i, row) in frame.rows.iter_mut().enumerate() {
        row.insert(0, Some((i + 1).to_string()));
    }
    frame
}

/// `default_chem_columns`: lowercase INAA-list matches in frame order;
/// otherwise every column except `rowid`/`anid`.
pub fn default_chem_columns(columns: &[String]) -> Vec<String> {
    let matched: Vec<String> = columns
        .iter()
        .filter(|c| COMMON_INAA_ELEMENTS.contains(&c.to_lowercase().as_str()))
        .cloned()
        .collect();
    if !matched.is_empty() {
        return matched;
    }
    columns
        .iter()
        .filter(|c| {
            let lowered = c.to_lowercase();
            lowered != "rowid" && lowered != "anid"
        })
        .cloned()
        .collect()
}

/// Columns with at least one cell that coerces to a number, in frame order:
/// the legacy loader's `mutate_all(as.numeric) %>% remove_empty("cols")`
/// (`DataLoader.R` `loadchemUI`). Blank cells, `NA` text, and `NaN` count as
/// missing, exactly like `as.numeric`.
pub fn numeric_candidate_columns(frame: &TextFrame) -> Vec<String> {
    frame
        .columns
        .iter()
        .enumerate()
        .filter(|(idx, _)| {
            frame.rows.iter().any(|row| {
                row.get(*idx)
                    .and_then(|cell| cell.as_deref())
                    .and_then(|cell| parse_r_numeric(cell.trim()))
                    .is_some_and(|v| !v.is_nan())
            })
        })
        .map(|(_, name)| name.clone())
        .collect()
}

/// The measured-column suggestion for a loaded frame: `default_chem_columns`
/// over the numeric non-empty columns only, so text columns are never
/// suggested as measured values (`DataLoader.R:211-213`).
pub fn default_chem_columns_for_frame(frame: &TextFrame) -> Vec<String> {
    default_chem_columns(&numeric_candidate_columns(frame))
}

/// `default_id_column`: first case-insensitive `anid` match, if any.
pub fn default_id_column(columns: &[String]) -> Option<String> {
    columns.iter().find(|c| c.to_lowercase() == "anid").cloned()
}

/// `guess_numeric_columns_fast`: sample the first `sample_n` rows, require at
/// least `min_parse_rate` of the non-empty cells to parse as numeric.
///
/// True blanks (empty fields) drop out of the denominator exactly like the R
/// `!is.na(vals) & nzchar(trimws(vals))` filter; literal `NA` text stays in
/// the denominator and fails parsing, matching `as.numeric("NA")`.
pub fn guess_numeric_columns_fast(
    frame: &TextFrame,
    exclude: &[&str],
    sample_n: usize,
    min_parse_rate: f64,
) -> Vec<String> {
    let sample_n = sample_n.min(frame.n_rows());
    let mut out = Vec::new();
    for (idx, name) in frame.columns.iter().enumerate() {
        if name.is_empty() || exclude.contains(&name.as_str()) || out.contains(name) {
            continue;
        }
        let mut parsed = 0usize;
        let mut attempted = 0usize;
        for row in frame.rows.iter().take(sample_n) {
            let Some(Some(cell)) = row.get(idx).map(|c| c.as_deref()) else {
                continue;
            };
            let trimmed = cell.trim();
            if trimmed.is_empty() {
                continue;
            }
            attempted += 1;
            // R's as.numeric("NaN") yields an is.na value, so it counts as a
            // parse failure for the numeric-like rate, exactly like "NA" text.
            // Infinite values parse fine (R accepts Inf).
            match parse_r_numeric(trimmed) {
                Some(v) if v.is_finite() => parsed += 1,
                Some(v) if v.is_infinite() => parsed += 1,
                _ => {}
            }
        }
        if attempted > 0 && (parsed as f64) / (attempted as f64) >= min_parse_rate {
            out.push(name.clone());
        }
    }
    out
}

/// Group partitions sorted by group value, matching `count()` + `arrange()`.
pub fn group_partitions(
    frame: &TextFrame,
    group_col: &str,
) -> Result<Vec<(String, usize)>, ImportError> {
    let idx = frame
        .column_index(group_col)
        .ok_or_else(|| ImportError::Parse(format!("missing group column {group_col}")))?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut blanks = 0usize;
    for row in &frame.rows {
        match &row[idx] {
            Some(v) if !v.trim().is_empty() => *counts.entry(v.clone()).or_insert(0) += 1,
            _ => blanks += 1,
        }
    }
    if blanks > 0 {
        // Section 7.2 item 12: blank group values require an explicit
        // destination choice; plain partitioning rejects them.
        return Err(ImportError::Parse(format!(
            "group column {group_col} has {blanks} blank value(s); choose an explicit destination"
        )));
    }
    Ok(counts.into_iter().collect())
}

/// Numeric coercion of one column (`as.numeric(as.character(x))`): blank and
/// `NA` become NaN like R's NA_real_.
pub fn numeric_column(frame: &TextFrame, name: &str) -> Result<Vec<f64>, ImportError> {
    let col = frame
        .column(name)
        .ok_or_else(|| ImportError::Parse(format!("missing column {name}")))?;
    Ok(col
        .into_iter()
        .map(|cell| cell.and_then(parse_r_numeric).unwrap_or(f64::NAN))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numeric_like_rates_with_blank_denominator() {
        let frame = TextFrame {
            columns: vec!["rowid".into(), "v".into(), "txt".into()],
            rows: vec![
                vec![Some("1".into()), Some("1.5".into()), Some("a".into())],
                vec![Some("2".into()), None, Some("b".into())],
                vec![Some("3".into()), Some("2".into()), Some("c".into())],
                vec![Some("4".into()), Some("NA".into()), Some("d".into())],
            ],
        };
        // "v": attempted = 3 (1.5, 2, literal NA), parsed = 2 -> 0.667 < 0.95.
        // "txt": attempted = 4, parsed = 0.
        assert_eq!(
            guess_numeric_columns_fast(&frame, &["rowid"], 1500, 0.95),
            Vec::<String>::new()
        );
        let relaxed = guess_numeric_columns_fast(&frame, &["rowid"], 1500, 0.5);
        assert_eq!(relaxed, vec!["v".to_string()]);
    }

    #[test]
    fn default_measured_columns_skip_text_and_empty_columns() {
        let frame = TextFrame {
            columns: vec![
                "rowid".into(),
                "anid".into(),
                "Site".into(),
                "Depth".into(),
                "Blank".into(),
                "Notes".into(),
            ],
            rows: vec![
                vec![
                    Some("1".into()),
                    Some("A1".into()),
                    Some("Baca".into()),
                    Some("1.5".into()),
                    None,
                    Some("NA".into()),
                ],
                vec![
                    Some("2".into()),
                    Some("A2".into()),
                    Some("Zuni".into()),
                    Some("x".into()),
                    None,
                    Some("NaN".into()),
                ],
            ],
        };
        assert_eq!(numeric_candidate_columns(&frame), vec!["rowid", "Depth"]);
        // No INAA names: every numeric column except rowid/anid.
        assert_eq!(default_chem_columns_for_frame(&frame), vec!["Depth"]);
        // The column-name-only rule would have suggested the text columns.
        assert!(default_chem_columns(&frame.columns).contains(&"Site".to_string()));
    }

    #[test]
    fn data_loader_prepends_rowid_and_cleans_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mini.csv");
        std::fs::write(&path, "anid,Sub-region,as\nA1,x,1.5\nA2,y,\n").unwrap();
        let frame = data_loader(&path).unwrap();
        assert_eq!(frame.columns, vec!["rowid", "anid", "Sub_region", "as"]);
        assert_eq!(frame.rows[0][0].as_deref(), Some("1"));
        assert_eq!(frame.rows[1][0].as_deref(), Some("2"));
        assert_eq!(frame.rows[1][3], None);
    }

    #[test]
    fn tsv_sources_split_on_tabs() {
        let dir = tempfile::tempdir().unwrap();
        // Commas inside TSV fields are data, not separators.
        let body = "anid\tSite\tfe\nA1\tBaca, NM\t2.5\nA2\tZuni\t\n";
        let path = dir.path().join("mini.tsv");
        std::fs::write(&path, body).unwrap();
        let frame = data_loader(&path).unwrap();
        assert_eq!(frame.columns, vec!["rowid", "anid", "Site", "fe"]);
        assert_eq!(frame.rows[0][2].as_deref(), Some("Baca, NM"));
        assert_eq!(frame.rows[0][3].as_deref(), Some("2.5"));
        assert_eq!(frame.rows[1][3], None);

        // An explicit format wins over the (meaningless) extension.
        let staged = dir.path().join("upload.bin");
        std::fs::write(&staged, body).unwrap();
        let loaded = load_source(&staged, SourceFormat::Tsv, None).unwrap();
        assert_eq!(loaded.frame.columns.len(), 4);
        assert_eq!(loaded.sheet, None);
        assert!(loaded.sheets.is_empty());
        assert_eq!(SourceFormat::from_extension("TSV"), Some(SourceFormat::Tsv));
        assert_eq!(SourceFormat::from_extension("xls"), None);
    }
}
