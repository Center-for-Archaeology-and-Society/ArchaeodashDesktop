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
    ExportMeasuredDataRequest, ExportPcaScoresRequest, ExportResult, ExportTransformedRequest,
    PcaRequest,
};
use archaeodash_data_io::{csv_field, parse_r_numeric, r_format_double, read_group_file};
use archaeodash_domain::DomainError;

use super::ordination::OrdinationService;
use super::transforms::TransformService;

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

fn io_err(e: std::io::Error) -> DomainError {
    DomainError::Internal(Box::new(e))
}

fn import_err(e: archaeodash_data_io::ImportError) -> DomainError {
    DomainError::Internal(Box::new(e))
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
    /// `saveexportTab.R` extension rule.
    fn file_name(&self, path: &str) -> Result<String, DomainError> {
        let stem = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        ensure_csv_extension(stem)
    }

    /// Legacy `rvals$selectedData` export: visible ID, descriptive columns,
    /// then measured elemental columns rendered with `r_format_double`.
    /// Missing elemental values render as the legacy `fwrite` NA empty field.
    pub fn export_measured_data(
        &self,
        req: &ExportMeasuredDataRequest,
    ) -> Result<ExportResult, DomainError> {
        let data = read_group_file(&self.root.join(&req.path)).map_err(import_err)?;
        let roles = &data.profile.roles;
        let mut columns = Vec::with_capacity(2 + roles.descriptive.len() + roles.elemental.len());
        columns.push(roles.visible_id.clone());
        columns.extend(roles.descriptive.iter().cloned());
        columns.extend(roles.elemental.iter().cloned());
        let mut rows = Vec::with_capacity(data.rows.len());
        for row in &data.rows {
            let mut cells = Vec::with_capacity(columns.len());
            cells.push(text_cell(row.visible.as_deref(), req.raw_text));
            for value in &row.descriptive {
                cells.push(text_cell(value.as_deref(), req.raw_text));
            }
            for value in &row.elemental {
                cells.push(numeric_cell(*value));
            }
            rows.push(cells.join(","));
        }
        Ok(ExportResult {
            file_name: self.file_name(&req.path)?,
            media_type: "text/csv".to_string(),
            content: csv_text(&columns, &rows),
        })
    }

    /// Explicitly computed transformed result export (Section 5: calculated
    /// values are recomputed on demand and never persisted): visible ID,
    /// descriptive columns, then the transformed matrix columns.
    pub fn export_transformed(
        &self,
        req: &ExportTransformedRequest,
    ) -> Result<ExportResult, DomainError> {
        let data = read_group_file(&self.root.join(&req.path)).map_err(import_err)?;
        let (matrix, _) = TransformService::apply_definition(&data, &req.definition, &req.path)?;
        let roles = &data.profile.roles;
        let mut columns = Vec::with_capacity(2 + roles.descriptive.len() + matrix.names.len());
        columns.push(roles.visible_id.clone());
        columns.extend(roles.descriptive.iter().cloned());
        columns.extend(matrix.names.iter().cloned());
        let n_rows = matrix.n_rows();
        let mut rows = Vec::with_capacity(n_rows);
        for (i, row) in data.rows.iter().enumerate() {
            let mut cells = Vec::with_capacity(columns.len());
            cells.push(text_cell(row.visible.as_deref(), req.raw_text));
            for value in &row.descriptive {
                cells.push(text_cell(value.as_deref(), req.raw_text));
            }
            for col in &matrix.cols {
                cells.push(numeric_cell(Some(col[i])));
            }
            rows.push(cells.join(","));
        }
        Ok(ExportResult {
            file_name: self.file_name(&req.path)?,
            media_type: "text/csv".to_string(),
            content: csv_text(&columns, &rows),
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
        })?;
        let data = read_group_file(&self.root.join(&req.path)).map_err(import_err)?;
        let roles = &data.profile.roles;
        let mut columns = Vec::with_capacity(2 + roles.descriptive.len() + pca.score_names.len());
        columns.push(roles.visible_id.clone());
        columns.extend(roles.descriptive.iter().cloned());
        columns.extend(pca.score_names.iter().cloned());
        let mut rows = Vec::with_capacity(data.rows.len());
        for (i, row) in data.rows.iter().enumerate() {
            let mut cells = Vec::with_capacity(columns.len());
            // Scores are computed values; the guard always applies here.
            cells.push(text_cell(row.visible.as_deref(), false));
            for value in &row.descriptive {
                cells.push(text_cell(value.as_deref(), false));
            }
            for score in &pca.scores[i] {
                cells.push(csv_field(Some(&r_format_double(*score))));
            }
            rows.push(cells.join(","));
        }
        Ok(ExportResult {
            file_name: self.file_name(&req.path)?,
            media_type: "text/csv".to_string(),
            content: csv_text(&columns, &rows),
        })
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
                raw_text: false,
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
                raw_text: false,
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
                raw_text: true,
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
}
