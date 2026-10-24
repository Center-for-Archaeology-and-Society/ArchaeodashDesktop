//! Result-export use cases (Section 7.3): the measured chemical data frame,
//! the explicitly computed transformed result, and the PCA score frame, each
//! serialized as a CSV string returned to the caller.
//!
//! Legacy disposition (`R/saveexportTab.R`): `rio::export` of
//! `rvals$selectedData` (chemical), `rvals$pcaData` (PCA), or
//! `rvals$membershipProbs` (membership). The legacy `pcaData` reactive never
//! existed; per Section 3.2 the PCA export is the computed `pcadf` score
//! frame instead. Membership probabilities are Phase 6 and out of scope.
//!
//! All exports are ephemeral: nothing is written into the project (Section 5
//! storage invariant: calculated values never reach a group Parquet file).
//! The desktop client saves the returned text through a native save dialog;
//! the hosted streaming-download route arrives with the Phase 7 job surface.
//!
//! CSV cells reuse the golden-14 `fwrite` necessary-quoting writer
//! (`archaeodash_data_io::csv_field`) on pre-escaped text. Section 7.3 adds
//! a formula-injection guard by default: a text cell whose first character
//! is one of `= + - @` and which does not parse as a finite number via
//! `parse_r_numeric` gets a leading `'` prefix, so negative numbers such as
//! `-1.5` are never escaped. `raw_text = true` is the clearly-labeled raw
//! option and reproduces the byte-exact legacy `fwrite` output for the
//! measured-data case. XLSX/TSV formats are deferred per the Section 7.1
//! capability matrix, so the legacy filename rule always yields `.csv`.

use std::path::{Path, PathBuf};

use archaeodash_contracts::{
    ExportFormat, ExportMeasuredDataRequest, ExportPcaScoresRequest, ExportResult,
    ExportTableRequest, ExportTableSheet, ExportTransformedRequest, PcaRequest, TableCell,
};
use archaeodash_data_io::{csv_field, parse_r_numeric, r_format_double, GroupRow};
use archaeodash_domain::DomainError;
use base64::Engine as _;
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};

use super::combined::CombinedInput;
use super::ordination::OrdinationService;
use super::transforms::TransformService;

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

fn io_err(e: std::io::Error) -> DomainError {
    DomainError::Internal(Box::new(e))
}

fn xlsx_err(e: XlsxError) -> DomainError {
    DomainError::Internal(Box::new(e))
}

/// Media type of XLSX workbook exports.
pub const XLSX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// Excel's per-sheet row limit (header included).
const XLSX_MAX_ROWS: usize = 1_048_576;
/// Excel's per-cell text limit.
const XLSX_MAX_TEXT: usize = 32_767;

/// A valid, unique Excel sheet name: at most 31 characters, none of
/// `[ ] : * ? / \`, no leading/trailing apostrophe, and not "History"
/// (reserved). Clashes (case-insensitive) get a ` (2)`, ` (3)`… suffix.
fn sheet_name(raw: &str, used: &mut Vec<String>) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if "[]:*?/\\".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('\'').trim();
    let base = if cleaned.is_empty() { "Group" } else { cleaned };
    let taken = |name: &str, used: &[String]| {
        let lower = name.to_lowercase();
        lower == "history" || used.iter().any(|u| u.to_lowercase() == lower)
    };
    let mut n = 1;
    loop {
        let suffix = if n == 1 {
            String::new()
        } else {
            format!(" ({n})")
        };
        let keep = 31 - suffix.chars().count();
        let candidate = format!("{}{suffix}", base.chars().take(keep).collect::<String>());
        if !taken(&candidate, used) {
            used.push(candidate.clone());
            return candidate;
        }
        n += 1;
    }
}

/// Writes text into a cell (never as a formula), within Excel's length limit.
fn xlsx_text(sheet: &mut Worksheet, row: u32, col: u16, text: &str) -> Result<(), XlsxError> {
    if text.chars().count() > XLSX_MAX_TEXT {
        let cut: String = text.chars().take(XLSX_MAX_TEXT).collect();
        sheet.write_string(row, col, cut)?;
    } else {
        sheet.write_string(row, col, text)?;
    }
    Ok(())
}

/// One worksheet: bold frozen header, then text cells for the ID and
/// descriptive columns and numbers for measured values (missing = blank).
fn write_measured_sheet(
    book: &mut Workbook,
    name: &str,
    columns: &[String],
    rows: &[GroupRow],
    n_desc: usize,
) -> Result<(), XlsxError> {
    let header = Format::new().set_bold();
    let sheet = book.add_worksheet();
    sheet.set_name(name)?;
    for (c, column) in columns.iter().enumerate() {
        sheet.write_string_with_format(0, c as u16, column, &header)?;
    }
    sheet.set_freeze_panes(1, 0)?;
    for (r, row) in rows.iter().enumerate() {
        let excel_row = (r + 1) as u32;
        let mut col: u16 = 0;
        if let Some(text) = row.visible.as_deref() {
            xlsx_text(sheet, excel_row, col, text)?;
        }
        col += 1;
        for value in &row.descriptive[..n_desc] {
            if let Some(text) = value.as_deref() {
                xlsx_text(sheet, excel_row, col, text)?;
            }
            col += 1;
        }
        for value in &row.elemental {
            if let Some(number) = value.filter(|v| v.is_finite()) {
                sheet.write_number(excel_row, col, number)?;
            }
            col += 1;
        }
    }
    Ok(())
}

/// Legacy `saveexportTab.R` filename rule: a user-supplied name containing
/// `.` is kept as typed, anything else gets the default extension appended.
/// XLSX/TSV export is deferred per the Section 7.1 capability matrix, so the
/// appended extension is always `.csv`. Empty names are rejected.
pub fn ensure_csv_extension(name: &str) -> Result<String, DomainError> {
    if name.trim().is_empty() {
        return Err(validation("export_empty_name", "export file name is empty"));
    }
    if name.contains('.') {
        Ok(name.to_string())
    } else {
        Ok(format!("{name}.csv"))
    }
}

/// True when a text cell needs the Section 7.3 spreadsheet formula-injection
/// guard: first character `=`, `+`, `-`, or `@` and the text does not parse
/// as a finite number (so `-1.5` and `+1e5` pass through untouched).
fn needs_formula_guard(text: &str) -> bool {
    match text.chars().next() {
        Some('=' | '+' | '-' | '@') => !parse_r_numeric(text).is_some_and(|v| v.is_finite()),
        _ => false,
    }
}

/// One text CSV cell: the Section 7.3 guard (unless raw output was requested)
/// followed by `fwrite` necessary-quoting. `None` is the legacy NA empty
/// field (`""`).
fn text_cell(value: Option<&str>, raw_text: bool) -> String {
    let text = value.unwrap_or("");
    let guarded = if raw_text || !needs_formula_guard(text) {
        text.to_string()
    } else {
        format!("'{text}")
    };
    csv_field(Some(&guarded))
}

/// One numeric CSV cell: R-compatible double text; missing or non-finite
/// values render as the legacy `fwrite` NA empty field (`""`).
fn numeric_cell(value: Option<f64>) -> String {
    csv_field(
        value
            .filter(|v| v.is_finite())
            .map(r_format_double)
            .as_deref(),
    )
}

/// Header row plus data rows with `\n` endings and a trailing newline,
/// matching `write_text_csv` formatting.
fn csv_text(columns: &[String], rows: &[String]) -> String {
    let mut out = String::new();
    out.push_str(
        &columns
            .iter()
            .map(|c| csv_field(Some(c)))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('\n');
    for row in rows {
        out.push_str(row);
        out.push('\n');
    }
    out
}

/// Result-export use cases rooted at one local project directory, sharing
/// the transform and ordination engines with `TransformService` and
/// `OrdinationService`.
pub struct ExportService {
    root: PathBuf,
}

impl ExportService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, DomainError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        Ok(Self { root })
    }

    /// Suggested download filename: the group file stem under the legacy
    /// `saveexportTab.R` extension rule; a multi-file export is named
    /// `<first-stem>-and-<N>-more.csv`.
    fn file_name(&self, path: &str, extra_paths: &[String]) -> Result<String, DomainError> {
        let stem = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if extra_paths.is_empty() {
            ensure_csv_extension(stem)
        } else {
            ensure_csv_extension(&format!("{stem}-and-{}-more", extra_paths.len()))
        }
    }

    /// The CSV file name with `.xlsx` in place of `.csv`.
    fn xlsx_file_name(&self, path: &str, extra_paths: &[String]) -> Result<String, DomainError> {
        let csv = self.file_name(path, extra_paths)?;
        Ok(match csv.strip_suffix(".csv") {
            Some(stem) => format!("{stem}.xlsx"),
            None => format!("{csv}.xlsx"),
        })
    }

    /// Measured-data workbook: with several groups, an "All groups" sheet
    /// (with the group column) followed by one sheet per group; with one
    /// group, a single sheet named after it. Cells are typed (text vs
    /// numbers) and text is never evaluated, so no formula guard is needed.
    fn measured_workbook(
        &self,
        req: &ExportMeasuredDataRequest,
        input: &CombinedInput,
        columns: &[String],
        n_desc: usize,
    ) -> Result<ExportResult, DomainError> {
        let rows = &input.data.rows;
        if rows.len() + 1 > XLSX_MAX_ROWS {
            return Err(validation(
                "export_too_many_rows",
                format!(
                    "{} rows do not fit in one Excel sheet (limit {}); export CSV instead",
                    rows.len(),
                    XLSX_MAX_ROWS - 1
                ),
            ));
        }
        let mut book = Workbook::new();
        let mut used = Vec::new();
        if input.is_multi() {
            let name = sheet_name("All groups", &mut used);
            write_measured_sheet(&mut book, &name, columns, rows, n_desc).map_err(xlsx_err)?;
        }
        let mut start = 0;
        for source in &input.sources {
            let end = (start + source.row_count).min(rows.len());
            let name = sheet_name(&source.group_name, &mut used);
            write_measured_sheet(&mut book, &name, columns, &rows[start..end], n_desc)
                .map_err(xlsx_err)?;
            start = end;
        }
        let bytes = book.save_to_buffer().map_err(xlsx_err)?;
        Ok(ExportResult {
            file_name: self.xlsx_file_name(&req.path, &req.extra_paths)?,
            media_type: XLSX_MEDIA_TYPE.to_string(),
            content: base64::engine::general_purpose::STANDARD.encode(bytes),
            content_encoding: Some("base64".to_string()),
        })
    }

    /// Reads the export input. Single-file exports keep their historical
    /// columns (no synthetic file-group column); multi-file exports include
    /// it as the last descriptive column. Returns the input and the number
    /// of leading descriptive cells to write per row.
    fn read_input(
        &self,
        path: &str,
        extra_paths: &[String],
    ) -> Result<(CombinedInput, usize), DomainError> {
        let input = CombinedInput::read_local(&self.root, path, extra_paths)?;
        let n = if input.is_multi() {
            input.data.profile.roles.descriptive.len()
        } else {
            input.real_descriptive().len()
        };
        Ok((input, n))
    }

    /// Legacy `rvals$selectedData` export: visible ID, descriptive columns,
    /// then measured elemental columns rendered with `r_format_double`.
    /// Missing elemental values render as the legacy `fwrite` NA empty field.
    pub fn export_measured_data(
        &self,
        req: &ExportMeasuredDataRequest,
    ) -> Result<ExportResult, DomainError> {
        let (input, n_desc) = self.read_input(&req.path, &req.extra_paths)?;
        let data = &input.data;
        let roles = &data.profile.roles;
        let mut columns = Vec::with_capacity(2 + n_desc + roles.elemental.len());
        columns.push(roles.visible_id.clone());
        columns.extend(roles.descriptive[..n_desc].iter().cloned());
        columns.extend(roles.elemental.iter().cloned());
        if req.format == ExportFormat::Xlsx {
            return self.measured_workbook(req, &input, &columns, n_desc);
        }
        let mut rows = Vec::with_capacity(data.rows.len());
        for row in &data.rows {
            let mut cells = Vec::with_capacity(columns.len());
            cells.push(text_cell(row.visible.as_deref(), req.raw_text));
            for value in &row.descriptive[..n_desc] {
                cells.push(text_cell(value.as_deref(), req.raw_text));
            }
            for value in &row.elemental {
                cells.push(numeric_cell(*value));
            }
            rows.push(cells.join(","));
        }
        Ok(ExportResult {
            file_name: self.file_name(&req.path, &req.extra_paths)?,
            media_type: "text/csv".to_string(),
            content: csv_text(&columns, &rows),
            content_encoding: None,
        })
    }

    /// Explicitly computed transformed result export (Section 5: calculated
    /// values are recomputed on demand and never persisted): visible ID,
    /// descriptive columns, then the transformed matrix columns.
    pub fn export_transformed(
        &self,
        req: &ExportTransformedRequest,
    ) -> Result<ExportResult, DomainError> {
        let (input, n_desc) = self.read_input(&req.path, &req.extra_paths)?;
        input.require_inputs(&[], Some(&req.definition))?;
        let data = &input.data;
        let (matrix, _) =
            TransformService::apply_definition(data, &req.definition, &input.context())?;
        let roles = &data.profile.roles;
        let mut columns = Vec::with_capacity(2 + n_desc + matrix.names.len());
        columns.push(roles.visible_id.clone());
        columns.extend(roles.descriptive[..n_desc].iter().cloned());
        columns.extend(matrix.names.iter().cloned());
        let n_rows = matrix.n_rows();
        let mut rows = Vec::with_capacity(n_rows);
        for (i, row) in data.rows.iter().enumerate() {
            let mut cells = Vec::with_capacity(columns.len());
            cells.push(text_cell(row.visible.as_deref(), req.raw_text));
            for value in &row.descriptive[..n_desc] {
                cells.push(text_cell(value.as_deref(), req.raw_text));
            }
            for col in &matrix.cols {
                cells.push(numeric_cell(Some(col[i])));
            }
            rows.push(cells.join(","));
        }
        Ok(ExportResult {
            file_name: self.file_name(&req.path, &req.extra_paths)?,
            media_type: "text/csv".to_string(),
            content: csv_text(&columns, &rows),
            content_encoding: None,
        })
    }

    /// PCA score-frame export: the Section 3.2 correction of the legacy
    /// `rvals$pcaData` bug. Rows follow the legacy `pcadf` shape — visible
    /// ID and descriptive columns followed by the `PC1..PCk` scores —
    /// computed through the same `OrdinationService` path as the PCA view.
    pub fn export_pca_scores(
        &self,
        req: &ExportPcaScoresRequest,
    ) -> Result<ExportResult, DomainError> {
        let ordination = OrdinationService::new(&self.root)?;
        let pca = ordination.pca(&PcaRequest {
            path: req.path.clone(),
            columns: req.columns.clone(),
            scale: req.scale,
            transformation: req.transformation.clone(),
            extra_paths: req.extra_paths.clone(),
        })?;
        let (input, n_desc) = self.read_input(&req.path, &req.extra_paths)?;
        let data = &input.data;
        let roles = &data.profile.roles;
        let mut columns = Vec::with_capacity(2 + n_desc + pca.score_names.len());
        columns.push(roles.visible_id.clone());
        columns.extend(roles.descriptive[..n_desc].iter().cloned());
        columns.extend(pca.score_names.iter().cloned());
        let mut rows = Vec::with_capacity(data.rows.len());
        for (i, row) in data.rows.iter().enumerate() {
            let mut cells = Vec::with_capacity(columns.len());
            // Scores are computed values; the guard always applies here.
            cells.push(text_cell(row.visible.as_deref(), false));
            for value in &row.descriptive[..n_desc] {
                cells.push(text_cell(value.as_deref(), false));
            }
            for score in &pca.scores[i] {
                cells.push(csv_field(Some(&r_format_double(*score))));
            }
            rows.push(cells.join(","));
        }
        Ok(ExportResult {
            file_name: self.file_name(&req.path, &req.extra_paths)?,
            media_type: "text/csv".to_string(),
            content: csv_text(&columns, &rows),
            content_encoding: None,
        })
    }
}

/// Most cells one table export may hold (all sheets together).
const TABLE_MAX_CELLS: usize = 20_000_000;

/// A safe file stem from a suggested name: no directories, no control or
/// reserved characters, no extension, never empty.
fn table_file_stem(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let base = match base.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 => stem,
        _ => base,
    };
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    let stem: String = cleaned.chars().take(120).collect();
    if stem.is_empty() {
        "table".to_string()
    } else {
        stem
    }
}

fn table_csv(sheet: &ExportTableSheet) -> String {
    let rows: Vec<String> = sheet
        .rows
        .iter()
        .map(|row| {
            (0..sheet.columns.len())
                .map(|c| match row.get(c).cloned().flatten() {
                    Some(TableCell::Number(v)) => numeric_cell(Some(v)),
                    Some(TableCell::Text(t)) => text_cell(Some(&t), false),
                    None => csv_field(None),
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    csv_text(&sheet.columns, &rows)
}

fn write_table_sheet(
    book: &mut Workbook,
    name: &str,
    sheet: &ExportTableSheet,
) -> Result<(), XlsxError> {
    let header = Format::new().set_bold();
    let ws = book.add_worksheet();
    ws.set_name(name)?;
    for (c, column) in sheet.columns.iter().enumerate() {
        ws.write_string_with_format(0, c as u16, column, &header)?;
    }
    ws.set_freeze_panes(1, 0)?;
    for (r, row) in sheet.rows.iter().enumerate() {
        let excel_row = (r + 1) as u32;
        for (c, value) in row.iter().take(sheet.columns.len()).enumerate() {
            match value {
                Some(TableCell::Number(v)) if v.is_finite() => {
                    ws.write_number(excel_row, c as u16, *v)?;
                }
                Some(TableCell::Text(t)) => xlsx_text(ws, excel_row, c as u16, t)?,
                _ => {}
            }
        }
    }
    // Column widths from the content; skipped for very large sheets.
    if sheet.rows.len() * sheet.columns.len().max(1) <= 200_000 {
        ws.autofit();
    }
    Ok(())
}

/// Any result table the client shows, as CSV (first sheet) or an XLSX
/// workbook (one worksheet per sheet). Pure: no project access.
pub fn export_table(req: &ExportTableRequest) -> Result<ExportResult, DomainError> {
    if req.sheets.is_empty() {
        return Err(validation("export_table_empty", "nothing to export"));
    }
    let mut cells = 0usize;
    for sheet in &req.sheets {
        if sheet.columns.is_empty() {
            return Err(validation(
                "export_table_no_columns",
                "an exported table needs at least one column",
            ));
        }
        if req.format == ExportFormat::Xlsx && sheet.rows.len() + 1 > XLSX_MAX_ROWS {
            return Err(validation(
                "export_too_many_rows",
                format!(
                    "{} rows do not fit in one Excel sheet (limit {}); export CSV instead",
                    sheet.rows.len(),
                    XLSX_MAX_ROWS - 1
                ),
            ));
        }
        cells = cells.saturating_add(sheet.rows.len().saturating_mul(sheet.columns.len()));
    }
    if cells > TABLE_MAX_CELLS {
        return Err(validation(
            "export_table_too_large",
            format!("the table has more than {TABLE_MAX_CELLS} cells"),
        ));
    }
    let stem = table_file_stem(&req.file_name);
    match req.format {
        ExportFormat::Csv => Ok(ExportResult {
            file_name: format!("{stem}.csv"),
            media_type: "text/csv".to_string(),
            content: table_csv(&req.sheets[0]),
            content_encoding: None,
        }),
        ExportFormat::Xlsx => {
            let mut book = Workbook::new();
            let mut used = Vec::new();
            for sheet in &req.sheets {
                let name = sheet_name(&sheet.name, &mut used);
                write_table_sheet(&mut book, &name, sheet).map_err(xlsx_err)?;
            }
            let bytes = book.save_to_buffer().map_err(xlsx_err)?;
            Ok(ExportResult {
                file_name: format!("{stem}.xlsx"),
                media_type: XLSX_MEDIA_TYPE.to_string(),
                content: base64::engine::general_purpose::STANDARD.encode(bytes),
                content_encoding: Some("base64".to_string()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use crate::import::ImportService;
    use archaeodash_contracts::{
        ImportCommitRequest, ImputationMethod, RatioMode, RatioSpecDto, TransformMethod,
        TransformationDefinition,
    };

    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Imports one three-row source and returns the service, project root,
    /// and the committed group path.
    fn service_with_group() -> (ExportService, std::path::PathBuf, String) {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-exports-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Baca,5,6\n",
        )
        .expect("write source");
        let import = ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "mini.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        (
            ExportService::new(&dir).expect("export service"),
            dir,
            commit.groups[0].path.clone(),
        )
    }

    #[test]
    fn filename_rule_matches_legacy_saveexport() {
        assert_eq!(ensure_csv_extension("results").unwrap(), "results.csv");
        assert_eq!(ensure_csv_extension("results.csv").unwrap(), "results.csv");
        // A name containing `.` is kept as typed (legacy rule).
        assert_eq!(
            ensure_csv_extension("results.v2.csv").unwrap(),
            "results.v2.csv"
        );
        let err = ensure_csv_extension("   ").expect_err("empty name");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "export_empty_name"
        ));
    }

    #[test]
    fn table_export_writes_csv_and_xlsx() {
        let sheet = ExportTableSheet {
            name: "UMAP scores".into(),
            columns: vec!["ANID".into(), "Group".into(), "V1".into()],
            rows: vec![
                vec![
                    Some(TableCell::Text("A1".into())),
                    Some(TableCell::Text("=Baca".into())),
                    Some(TableCell::Number(1.25)),
                ],
                vec![Some(TableCell::Text("A2".into())), None],
            ],
        };
        let csv = export_table(&ExportTableRequest {
            file_name: "../umap scores.xlsx".into(),
            format: ExportFormat::Csv,
            sheets: vec![sheet.clone()],
        })
        .expect("csv");
        assert_eq!(csv.file_name, "umap scores.csv");
        // Formula guard on text; numbers stay numbers; short rows pad empty.
        assert_eq!(csv.content, "ANID,Group,V1\nA1,'=Baca,1.25\nA2,\"\",\"\"\n");

        let xlsx = export_table(&ExportTableRequest {
            file_name: "".into(),
            format: ExportFormat::Xlsx,
            sheets: vec![
                sheet.clone(),
                ExportTableSheet {
                    name: "UMAP scores".into(),
                    ..sheet
                },
            ],
        })
        .expect("xlsx");
        assert_eq!(xlsx.file_name, "table.xlsx");
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-table-export-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("t.xlsx");
        std::fs::write(
            &file,
            base64::engine::general_purpose::STANDARD
                .decode(&xlsx.content)
                .expect("base64"),
        )
        .expect("write");
        let sheets = archaeodash_data_io::xlsx_sheet_names(&file).expect("sheets");
        assert_eq!(sheets, ["UMAP scores", "UMAP scores (2)"]);
        let (frame, _) =
            archaeodash_data_io::read_xlsx_table(&file, Some("UMAP scores")).expect("read");
        assert_eq!(frame.columns, ["ANID", "Group", "V1"]);
        assert_eq!(frame.rows[0][1].as_deref(), Some("=Baca"));
        let _ = std::fs::remove_dir_all(&dir);

        let empty = export_table(&ExportTableRequest {
            file_name: "x".into(),
            format: ExportFormat::Csv,
            sheets: vec![],
        });
        assert!(empty.is_err());
    }

    #[test]
    fn sheet_names_are_valid_and_unique() {
        let mut used = Vec::new();
        assert_eq!(sheet_name("All groups", &mut used), "All groups");
        assert_eq!(sheet_name("a/b:c*d?", &mut used), "a_b_c_d_");
        assert_eq!(sheet_name("all GROUPS", &mut used), "all GROUPS (2)");
        assert_eq!(sheet_name("History", &mut used), "History (2)");
        assert_eq!(sheet_name("'  '", &mut used), "Group");
        let long = sheet_name(&"x".repeat(40), &mut used);
        assert_eq!(long.chars().count(), 31);
        let long2 = sheet_name(&"x".repeat(40), &mut used);
        assert_eq!(long2, format!("{} (2)", "x".repeat(27)));
    }

    #[test]
    fn measured_export_xlsx_has_all_and_per_group_sheets() {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-exports-xlsx-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("two.csv"),
            "anid,Site,Note,as,fe\nA1,Baca,=x,1.5,3\nA2,Chaco,,2,\nA3,Baca,ok,5,6\n",
        )
        .expect("write source");
        let commit = ImportService::new(&dir)
            .expect("import service")
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "two.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        let paths: Vec<String> = commit.groups.iter().map(|g| g.path.clone()).collect();
        assert_eq!(paths.len(), 2);
        let service = ExportService::new(&dir).expect("export service");
        let result = service
            .export_measured_data(&ExportMeasuredDataRequest {
                path: paths[0].clone(),
                extra_paths: paths[1..].to_vec(),
                raw_text: false,
                format: ExportFormat::Xlsx,
            })
            .expect("export xlsx");
        assert_eq!(result.media_type, XLSX_MEDIA_TYPE);
        assert_eq!(result.content_encoding.as_deref(), Some("base64"));
        assert!(
            result.file_name.ends_with("-and-1-more.xlsx"),
            "{}",
            result.file_name
        );
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&result.content)
            .expect("base64");
        let file = dir.join("out.xlsx");
        std::fs::write(&file, bytes).expect("write xlsx");

        let sheets = archaeodash_data_io::xlsx_sheet_names(&file).expect("sheets");
        assert_eq!(sheets.len(), 3);
        assert_eq!(sheets[0], "All groups");
        let (all, _) =
            archaeodash_data_io::read_xlsx_table(&file, Some("All groups")).expect("all");
        let col = |name: &str| {
            all.columns
                .iter()
                .position(|c| c == name)
                .unwrap_or_else(|| panic!("no {name} column in {:?}", all.columns))
        };
        let (note_col, group_col) = (col("Note"), col("Group"));
        assert_eq!(all.columns[0], "anid");
        assert_eq!(all.columns[all.columns.len() - 2..], ["as", "fe"]);
        assert_eq!(all.n_rows(), 3);
        // Text is stored as text (no formula guard needed), numbers as numbers.
        let note = all
            .rows
            .iter()
            .find_map(|r| r[note_col].clone().filter(|v| v.starts_with('=')));
        assert_eq!(note.as_deref(), Some("=x"));
        // Each group sheet holds only its own rows.
        let mut per_group = 0;
        for name in &sheets[1..] {
            let (frame, _) =
                archaeodash_data_io::read_xlsx_table(&file, Some(name)).expect("group");
            assert!(frame
                .rows
                .iter()
                .all(|r| r[group_col].as_deref() == Some(name.as_str())));
            per_group += frame.n_rows();
        }
        assert_eq!(per_group, 3);

        // CSV stays the default when `format` is omitted on the wire.
        let req: ExportMeasuredDataRequest =
            serde_json::from_str(&format!(r#"{{"path":"{}"}}"#, paths[0])).expect("json");
        assert_eq!(req.format, ExportFormat::Csv);
        let csv = service.export_measured_data(&req).expect("csv");
        assert_eq!(csv.content_encoding, None);
        assert!(csv.file_name.ends_with(".csv"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn escaping_guard_rules() {
        // Spreadsheet-dangerous text is guarded by default.
        assert_eq!(text_cell(Some("=cmd"), false), "'=cmd");
        assert_eq!(text_cell(Some("+sum"), false), "'+sum");
        assert_eq!(text_cell(Some("@x"), false), "'@x");
        assert_eq!(text_cell(Some("-abc"), false), "'-abc");
        // Numbers that merely start with a sign character are untouched.
        assert_eq!(text_cell(Some("-1.5"), false), "-1.5");
        assert_eq!(text_cell(Some("+1e5"), false), "+1e5");
        // Plain text and empty/NA cells are untouched.
        assert_eq!(text_cell(Some("Baca"), false), "Baca");
        assert_eq!(text_cell(None, false), "\"\"");
        // raw_text disables the guard (byte-exact legacy fwrite output).
        assert_eq!(text_cell(Some("=cmd"), true), "=cmd");
        assert_eq!(text_cell(Some("@x"), true), "@x");
    }

    #[test]
    fn measured_export_columns_and_escaping() {
        let (service, dir, path) = service_with_group();
        let before = std::fs::read(dir.join(&path)).expect("read group");
        let result = service
            .export_measured_data(&ExportMeasuredDataRequest {
                path: path.clone(),
                extra_paths: vec![],
                raw_text: false,
                format: Default::default(),
            })
            .expect("export");
        assert_eq!(result.file_name, "Baca.csv");
        assert_eq!(result.media_type, "text/csv");
        let lines: Vec<&str> = result.content.trim_end_matches('\n').split('\n').collect();
        // Visible ID first, then descriptive, then elemental columns.
        assert_eq!(lines[0], "anid,Site,as,fe");
        assert_eq!(lines[1], "A1,Baca,1.5,3");
        assert_eq!(lines[2], "A2,Baca,2,4");
        assert_eq!(lines[3], "A3,Baca,5,6");
        // The hidden analytical_uuid never appears in an export.
        assert!(!result.content.contains("analytical_uuid"));
        assert!(!result.content.contains("legacy_rowid"));

        // A formula-looking visible ID is guarded unless raw_text is set;
        // the measured values themselves are never guarded.
        std::fs::write(
            dir.join("tricky.csv"),
            "anid,Site,as,fe\n=SUM(A1),Baca,-1.5,3\n@x,Baca,5,1e5\n",
        )
        .expect("write source");
        let import = ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "tricky.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit tricky");
        let tricky_path = commit.groups[0].path.clone();
        let guarded = service
            .export_measured_data(&ExportMeasuredDataRequest {
                path: tricky_path.clone(),
                extra_paths: vec![],
                raw_text: false,
                format: Default::default(),
            })
            .expect("guarded export");
        let lines: Vec<&str> = guarded.content.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines[1], "'=SUM(A1),Baca,-1.5,3");
        // 1e5 parses as a finite number, so no guard; r_format_double
        // renders it positionally like R's as.character.
        assert_eq!(lines[2], "'@x,Baca,5,100000");
        let raw = service
            .export_measured_data(&ExportMeasuredDataRequest {
                path: tricky_path,
                extra_paths: vec![],
                raw_text: true,
                format: Default::default(),
            })
            .expect("raw export");
        let lines: Vec<&str> = raw.content.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines[1], "=SUM(A1),Baca,-1.5,3");

        // Ephemeral: the group file is byte-identical after exporting.
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("read group"),
            before,
            "group file byte-identical after measured export"
        );
    }

    #[test]
    fn transformed_export_shape_and_ephemeral_values() {
        let (service, dir, path) = service_with_group();
        let before = std::fs::read(dir.join(&path)).expect("read group");
        let result = service
            .export_transformed(&ExportTransformedRequest {
                path: path.clone(),
                extra_paths: vec![],
                definition: TransformationDefinition {
                    name: "ratios".into(),
                    transform_method: TransformMethod::None,
                    imputation_method: ImputationMethod::None,
                    imputation_seed: None,
                    elemental_columns: vec!["as".into(), "fe".into()],
                    descriptive_columns: vec![],
                    group_column: None,
                    ratios: vec![RatioSpecDto {
                        output_name: None,
                        numerator: "as".into(),
                        denominator: "fe".into(),
                    }],
                    ratio_mode: RatioMode::Append,
                    precompute: Vec::new(),
                },
                raw_text: false,
            })
            .expect("export transformed");
        let lines: Vec<&str> = result.content.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines[0], "anid,Site,as,fe,as_fe");
        assert_eq!(lines[1], "A1,Baca,1.5,3,0.5");
        assert_eq!(lines[2], "A2,Baca,2,4,0.5");
        assert_eq!(lines[3], "A3,Baca,5,6,0.833333333333333");
        // Ephemeral: no calculated values reach the group file.
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("read group"),
            before,
            "group file byte-identical after transformed export"
        );

        // Descriptive columns cannot be transformed (validation error).
        let err = service
            .export_transformed(&ExportTransformedRequest {
                path,
                extra_paths: vec![],
                definition: TransformationDefinition {
                    name: "bad".into(),
                    transform_method: TransformMethod::None,
                    imputation_method: ImputationMethod::None,
                    imputation_seed: None,
                    elemental_columns: vec!["Site".into()],
                    descriptive_columns: vec![],
                    group_column: None,
                    ratios: vec![],
                    ratio_mode: RatioMode::Append,
                    precompute: Vec::new(),
                },
                raw_text: false,
            })
            .expect_err("descriptive column rejected");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "missing_elemental_column"
        ));
    }

    #[test]
    fn pca_scores_export_shape() {
        let (service, _dir, path) = service_with_group();
        let result = service
            .export_pca_scores(&ExportPcaScoresRequest {
                path: path.clone(),
                extra_paths: vec![],
                columns: vec!["as".into(), "fe".into()],
                scale: false,
                transformation: None,
            })
            .expect("export pca");
        assert_eq!(result.file_name, "Baca.csv");
        assert_eq!(result.media_type, "text/csv");
        let lines: Vec<&str> = result.content.trim_end_matches('\n').split('\n').collect();
        // Legacy pcadf shape: visible ID and descriptive columns first, then
        // the PC1..PCk score columns.
        assert_eq!(lines[0], "anid,Site,PC1,PC2");
        assert_eq!(lines.len(), 4, "one row per analytical unit");
        assert!(!result.content.contains("analytical_uuid"));

        // Scores are mean-centered per component (prcomp convention).
        let header: Vec<&str> = lines[0].split(',').collect();
        let pc1_index = header.iter().position(|c| *c == "PC1").expect("PC1");
        let pc1: Vec<f64> = lines[1..]
            .iter()
            .map(|l| {
                l.split(',')
                    .nth(pc1_index)
                    .expect("cell")
                    .parse()
                    .expect("finite score")
            })
            .collect();
        let mean = pc1.iter().sum::<f64>() / pc1.len() as f64;
        assert!(mean.abs() < 1e-9);

        // Unknown column is a validation error.
        let err = service
            .export_pca_scores(&ExportPcaScoresRequest {
                path,
                extra_paths: vec![],
                columns: vec!["cu".into()],
                scale: false,
                transformation: None,
            })
            .expect_err("unknown column");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "missing_elemental_column"
        ));
    }

    #[test]
    fn multi_file_measured_export_adds_file_group_column() {
        let (service, dir, path) = service_with_group();
        std::fs::write(
            dir.join("other.csv"),
            "anid,Site,Ware,fe,as,zn\nB1,Hooper,x,7,8,9\n",
        )
        .expect("write source");
        let import = ImportService::new(&dir).expect("import service");
        let other = import
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "other.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit other");
        let other_path = other.groups[0].path.clone();
        let result = service
            .export_measured_data(&ExportMeasuredDataRequest {
                path: path.clone(),
                extra_paths: vec![other_path.clone()],
                raw_text: false,
                format: Default::default(),
            })
            .expect("multi-file export");
        assert_eq!(result.file_name, "Baca-and-1-more.csv");
        let lines: Vec<&str> = result.content.trim_end_matches('\n').split('\n').collect();
        // Shared elemental columns only; descriptive union plus "Group".
        assert_eq!(lines[0], "anid,Site,Ware,Group,as,fe");
        assert_eq!(lines[1], "A1,Baca,\"\",Baca,1.5,3");
        assert_eq!(lines[3], "A3,Baca,\"\",Baca,5,6");
        assert_eq!(lines[4], "B1,Hooper,x,Hooper,8,7");
        assert_eq!(lines.len(), 5);

        // Single-file exports stay unchanged (no synthetic column).
        let single = service
            .export_measured_data(&ExportMeasuredDataRequest {
                path: other_path.clone(),
                extra_paths: vec![],
                raw_text: false,
                format: Default::default(),
            })
            .expect("single export");
        assert!(single.content.starts_with("anid,Site,Ware,fe,as,zn\n"));

        // Transformed export over both files pools rows and keeps the column.
        let transformed = service
            .export_transformed(&ExportTransformedRequest {
                path: path.clone(),
                extra_paths: vec![other_path.clone()],
                definition: TransformationDefinition {
                    name: "none".into(),
                    transform_method: TransformMethod::None,
                    imputation_method: ImputationMethod::None,
                    imputation_seed: None,
                    elemental_columns: vec!["as".into()],
                    descriptive_columns: vec![],
                    group_column: None,
                    ratios: vec![],
                    ratio_mode: RatioMode::Append,
                    precompute: Vec::new(),
                },
                raw_text: false,
            })
            .expect("multi-file transformed export");
        let lines: Vec<&str> = transformed
            .content
            .trim_end_matches('\n')
            .split('\n')
            .collect();
        assert_eq!(lines[0], "anid,Site,Ware,Group,as");
        assert_eq!(lines[4], "B1,Hooper,x,Hooper,8");

        // A column missing from one file is rejected, naming that file.
        let err = service
            .export_transformed(&ExportTransformedRequest {
                path,
                extra_paths: vec![other_path.clone()],
                definition: TransformationDefinition {
                    name: "zn".into(),
                    transform_method: TransformMethod::None,
                    imputation_method: ImputationMethod::None,
                    imputation_seed: None,
                    elemental_columns: vec!["zn".into()],
                    descriptive_columns: vec![],
                    group_column: None,
                    ratios: vec![],
                    ratio_mode: RatioMode::Append,
                    precompute: Vec::new(),
                },
                raw_text: false,
            })
            .expect_err("zn missing in first file");
        assert!(
            err.to_string().contains("missing_elemental_column"),
            "{err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
