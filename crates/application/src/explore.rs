//! Explore use cases (Phase 4, Section 8 procedure 12, class E): the
//! missing-data profile, histogram breakpoints/counts, crosstab summaries,
//! and the compositional-profile long table over one group file, optionally
//! after an ephemeral transformation. Results are returned to the caller and
//! never persisted (Section 5 storage invariant: no explore values reach
//! group files).

use std::path::PathBuf;

use archaeodash_analysis::{ColumnMatrix, CrosstabMethod};
use archaeodash_contracts::{
    CompositionalProfileRow, CrosstabCountRow, CrosstabRows, CrosstabSummaryRow,
    ExploreCompositionalProfileRequest, ExploreCompositionalProfileResponse,
    ExploreCrosstabRequest, ExploreCrosstabResponse, ExploreHistogramRequest,
    ExploreHistogramResponse, ExploreMissingProfileRequest, ExploreMissingProfileResponse,
    HistogramBinning, MissingProfileRow, TransformationDefinition,
};
use archaeodash_data_io::{parse_r_numeric, r_format_double, GroupFileData};
use archaeodash_domain::DomainError;

use super::combined::CombinedInput;
use super::transforms::TransformService;
use crate::prepared::PoolScope;

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

fn io_err(e: std::io::Error) -> DomainError {
    DomainError::Internal(Box::new(e))
}

/// Explore use cases rooted at one local project directory, sharing the
/// transform definition engine with `TransformService`.
pub struct ExploreService {
    root: PathBuf,
}

impl ExploreService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, DomainError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        Ok(Self { root })
    }

    /// Builds the explore input matrix: raw measured elemental columns, or
    /// the requested columns of the transformed matrix when a definition is
    /// given (same ephemeral semantics as the ordination input).
    fn input_matrix(
        data: &GroupFileData,
        columns: &[String],
        transformation: Option<&TransformationDefinition>,
        context: &str,
        pool: Option<&PoolScope>,
    ) -> Result<ColumnMatrix, DomainError> {
        match transformation {
            None => TransformService::matrix_from_group(data, columns, context),
            Some(definition) => {
                let (full, _) =
                    TransformService::apply_definition_in(data, definition, context, pool)?;
                let mut names = Vec::with_capacity(columns.len());
                let mut cols = Vec::with_capacity(columns.len());
                for name in columns {
                    let idx = full.names.iter().position(|n| n == name).ok_or_else(|| {
                        validation(
                            "missing_explore_column",
                            format!("{context}: column {name:?} is absent after transformation"),
                        )
                    })?;
                    names.push(full.names[idx].clone());
                    cols.push(full.cols[idx].clone());
                }
                Ok(ColumnMatrix { names, cols })
            }
        }
    }

    /// Raw text of one descriptive column; empty cells become `None` (the
    /// legacy `NA` grouping key).
    fn descriptive_column(
        data: &GroupFileData,
        name: &str,
        context: &str,
    ) -> Result<Vec<Option<String>>, DomainError> {
        let roles = &data.profile.roles;
        let idx = roles
            .descriptive
            .iter()
            .position(|c| c == name)
            .ok_or_else(|| {
                validation(
                    "missing_group_column",
                    format!("{context}: column {name:?} is not a descriptive column"),
                )
            })?;
        Ok(data
            .rows
            .iter()
            .map(|r| {
                r.descriptive
                    .get(idx)
                    .cloned()
                    .flatten()
                    .filter(|v| !v.is_empty())
            })
            .collect())
    }

    /// Numeric values of one column: elemental columns read directly;
    /// descriptive columns coerce through `as.numeric(as.character(...))`
    /// (`parse_r_numeric`).
    fn numeric_values(
        data: &GroupFileData,
        name: &str,
        context: &str,
    ) -> Result<Vec<Option<f64>>, DomainError> {
        let roles = &data.profile.roles;
        if let Some(idx) = roles.elemental.iter().position(|c| c == name) {
            return Ok(data
                .rows
                .iter()
                .map(|r| r.elemental.get(idx).copied().flatten())
                .collect());
        }
        if roles.descriptive.iter().any(|c| c == name) {
            return Ok(Self::descriptive_column(data, name, context)?
                .into_iter()
                .map(|v| v.and_then(|t| parse_r_numeric(&t)))
                .collect());
        }
        Err(validation(
            "missing_explore_column",
            format!("{context}: column {name:?} is not available"),
        ))
    }

    /// Raw text values of one column for `count` crosstabs: descriptive
    /// columns read directly; elemental columns render through the
    /// R-compatible double formatter (inverse of the import coercion).
    fn count_value_texts(
        data: &GroupFileData,
        name: &str,
        context: &str,
    ) -> Result<Vec<Option<String>>, DomainError> {
        let roles = &data.profile.roles;
        if roles.descriptive.iter().any(|c| c == name) {
            return Self::descriptive_column(data, name, context);
        }
        if roles.elemental.iter().any(|c| c == name) {
            return Ok(Self::numeric_values(data, name, context)?
                .into_iter()
                .map(|v| v.filter(|v| v.is_finite()).map(r_format_double))
                .collect());
        }
        Err(validation(
            "missing_explore_column",
            format!("{context}: column {name:?} is not available"),
        ))
    }

    /// Grouping keys of one crosstab column: descriptive columns as text,
    /// elemental columns through the R double formatter (the legacy app
    /// allowed any column as the first crosstab field).
    fn group_keys(
        data: &GroupFileData,
        name: &str,
        context: &str,
    ) -> Result<Vec<Option<String>>, DomainError> {
        if data.profile.roles.descriptive.iter().any(|c| c == name) {
            return Self::descriptive_column(data, name, context);
        }
        if data.profile.roles.elemental.iter().any(|c| c == name) {
            return Self::count_value_texts(data, name, context);
        }
        Err(validation(
            "missing_group_column",
            format!("{context}: column {name:?} is not available"),
        ))
    }

    fn is_elemental(data: &GroupFileData, name: &str) -> bool {
        data.profile.roles.elemental.iter().any(|c| c == name)
    }

    /// `profile_missing` band summary (Section 8 procedure 12).
    pub fn missing_profile(
        &self,
        req: &ExploreMissingProfileRequest,
    ) -> Result<ExploreMissingProfileResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "explore_empty",
                "missing profile requires at least one column",
            ));
        }
        let input = CombinedInput::read_local(&self.root, &req.path, &req.extra_paths)?;
        input.require_inputs(&req.columns, req.transformation.as_ref())?;
        let data = input.data;
        let matrix = Self::input_matrix(
            &data,
            &req.columns,
            req.transformation.as_ref(),
            &req.path,
            Some(&PoolScope::for_request(
                &self.root,
                &req.path,
                &req.extra_paths,
            )),
        )?;
        let rows = archaeodash_analysis::missing_profile(&matrix)
            .into_iter()
            .map(|r| MissingProfileRow {
                feature: r.feature,
                num_missing: r.num_missing,
                pct_missing: r.pct_missing,
                band: r.band.label().to_string(),
            })
            .collect();
        Ok(ExploreMissingProfileResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            rows,
        })
    }

    /// `hist.default` breakpoints and counts for one column.
    pub fn histogram(
        &self,
        req: &ExploreHistogramRequest,
    ) -> Result<ExploreHistogramResponse, DomainError> {
        let input = CombinedInput::read_local(&self.root, &req.path, &req.extra_paths)?;
        input.require_inputs(
            std::slice::from_ref(&req.column),
            req.transformation.as_ref(),
        )?;
        let data = input.data;
        let matrix = Self::input_matrix(
            &data,
            std::slice::from_ref(&req.column),
            req.transformation.as_ref(),
            &req.path,
            Some(&PoolScope::for_request(
                &self.root,
                &req.path,
                &req.extra_paths,
            )),
        )?;
        let (breaks, counts) = match req.binning {
            HistogramBinning::Pretty => {
                archaeodash_analysis::histogram(&matrix.cols[0], req.bins as usize)?
            }
            HistogramBinning::EqualWidth => {
                archaeodash_analysis::explore::histogram_bins(&matrix.cols[0], req.bins as usize)?
            }
        };
        Ok(ExploreHistogramResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            column: req.column.clone(),
            breaks,
            counts,
        })
    }

    /// Legacy `compute_crosstab_summary`: `count` groups by both columns,
    /// `mean`/`median`/`sd` coerce the value column and group by the group
    /// column only.
    pub fn crosstab(
        &self,
        req: &ExploreCrosstabRequest,
    ) -> Result<ExploreCrosstabResponse, DomainError> {
        let input = CombinedInput::read_local(&self.root, &req.path, &req.extra_paths)?;
        input.require_columns([req.group_column.as_str(), req.value_column.as_str()])?;
        let data = input.data;
        let groups = Self::group_keys(&data, &req.group_column, &req.path)?;
        // Measured columns group and sort numerically, as `dplyr` orders a
        // numeric key; text keys keep the C-locale order.
        let numeric_group = Self::is_elemental(&data, &req.group_column);
        let method = CrosstabMethod::parse(&req.summary_method)?;
        let rows = match method {
            CrosstabMethod::Count => {
                let values = Self::count_value_texts(&data, &req.value_column, &req.path)?;
                let numeric_value = Self::is_elemental(&data, &req.value_column);
                let mut rows = archaeodash_analysis::crosstab_count(&groups, &values)?;
                if numeric_group || numeric_value {
                    rows.sort_by(|a, b| {
                        key_cmp(&a.group, &b.group, numeric_group)
                            .then_with(|| key_cmp(&a.value, &b.value, numeric_value))
                    });
                }
                CrosstabRows::Count {
                    rows: rows
                        .into_iter()
                        .map(|r| CrosstabCountRow {
                            group: r.group,
                            value: r.value,
                            count: r.count,
                        })
                        .collect(),
                }
            }
            method => {
                let values = Self::numeric_values(&data, &req.value_column, &req.path)?;
                let has_numeric = values.iter().any(|v| matches!(v, Some(v) if v.is_finite()));
                if !has_numeric {
                    return Err(validation(
                        "explore_crosstab_numeric",
                        format!(
                            "Column '{}' cannot be converted to numeric values.",
                            req.value_column
                        ),
                    ));
                }
                let mut summaries =
                    archaeodash_analysis::crosstab_value_summary(&groups, &values, method)?;
                if numeric_group {
                    summaries.sort_by(|a, b| key_cmp(&a.group, &b.group, true));
                }
                CrosstabRows::Summary {
                    result_column: format!("result-{}", req.value_column),
                    rows: summaries
                        .into_iter()
                        .map(|r| CrosstabSummaryRow {
                            group: r.group,
                            result: if r.result.is_finite() {
                                Some(r.result)
                            } else {
                                None
                            },
                        })
                        .collect(),
                }
            }
        };
        Ok(ExploreCrosstabResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            summary_method: req.summary_method.clone(),
            rows,
        })
    }

    /// `comp.profile` `pivot_longer` long table, optionally grouped.
    pub fn compositional_profile(
        &self,
        req: &ExploreCompositionalProfileRequest,
    ) -> Result<ExploreCompositionalProfileResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "explore_empty",
                "compositional profile requires at least one column",
            ));
        }
        let input = CombinedInput::read_local(&self.root, &req.path, &req.extra_paths)?;
        input.require_inputs(&req.columns, req.transformation.as_ref())?;
        input.require_columns(req.group_column.as_deref())?;
        let data = input.data;
        let matrix = Self::input_matrix(
            &data,
            &req.columns,
            req.transformation.as_ref(),
            &req.path,
            Some(&PoolScope::for_request(
                &self.root,
                &req.path,
                &req.extra_paths,
            )),
        )?;
        let labels = match &req.group_column {
            Some(name) => Some(Self::descriptive_column(&data, name, &req.path)?),
            None => None,
        };
        let rows = archaeodash_analysis::compositional_profile(&matrix, labels.as_deref())?;
        Ok(ExploreCompositionalProfileResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            rows: rows
                .into_iter()
                .map(|r| CompositionalProfileRow {
                    rowid: r.rowid,
                    element: r.element,
                    value: if r.value.is_finite() {
                        Some(r.value)
                    } else {
                        None
                    },
                    group_label: r.group_label,
                })
                .collect(),
        })
    }
}

/// Crosstab key order: numeric keys by value, text keys by bytes (C
/// locale), missing keys last.
fn key_cmp(a: &Option<String>, b: &Option<String>, numeric: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            if numeric {
                if let (Some(fx), Some(fy)) = (parse_r_numeric(x), parse_r_numeric(y)) {
                    return fx.total_cmp(&fy);
                }
            }
            x.as_bytes().cmp(y.as_bytes())
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use crate::groups::GroupService;
    use archaeodash_contracts::ImportCommitRequest;

    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Imports a three-group source and merges it into one file spanning all
    /// levels, returning the service and the merged group path.
    fn service_with_merged_group() -> (ExploreService, std::path::PathBuf, String) {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-explore-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("three.csv"),
            "anid,Site,as,fe\nA1,A,1.5,3\nA2,A,2,4\nB1,B,5,6\nB2,B,6,8\nC1,C,9,1\nC2,C,11,2\n",
        )
        .expect("write source");
        let import = crate::import::ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "three.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        let groups = GroupService::new(&dir).expect("group service");
        let sources: Vec<String> = commit.groups.iter().map(|g| g.path.clone()).collect();
        let merge = groups
            .merge_groups(&archaeodash_contracts::MergeGroupsRequest {
                sources,
                new_group_name: "Merged".into(),
            })
            .expect("merge");
        let path = merge.outputs[0].path.clone();
        (
            ExploreService::new(&dir).expect("explore service"),
            dir,
            path,
        )
    }

    #[test]
    fn missing_profile_orders_by_descending_missing_count() {
        let (service, dir, _path) = service_with_merged_group();
        // One empty `as` cell survives the import as NA.
        std::fs::write(
            dir.join("gaps.csv"),
            "anid,Site,as,fe\nA1,A,1.5,3\nA2,A,,4\nB1,B,5,6\nB2,B,6,8\nC1,C,9,1\nC2,C,11,2\n",
        )
        .expect("write source");
        let import = crate::import::ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "gaps.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        let groups = GroupService::new(&dir).expect("group service");
        let sources: Vec<String> = commit.groups.iter().map(|g| g.path.clone()).collect();
        let merge = groups
            .merge_groups(&archaeodash_contracts::MergeGroupsRequest {
                sources,
                new_group_name: "Gapped".into(),
            })
            .expect("merge");
        let response = service
            .missing_profile(&ExploreMissingProfileRequest {
                path: merge.outputs[0].path.clone(),
                extra_paths: vec![],
                columns: vec!["fe".into(), "as".into()],
                transformation: None,
            })
            .expect("missing profile");
        assert_eq!(response.rows.len(), 2);
        // Descending missing count: `as` (1/6 missing, OK) before `fe` (Good).
        assert_eq!(response.rows[0].feature, "as");
        assert_eq!(response.rows[0].band, "OK");
        assert!((response.rows[0].pct_missing - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!(response.rows[1].feature, "fe");
        assert_eq!(response.rows[1].band, "Good");
        assert_eq!(response.rows[1].num_missing, 0);
        assert!(!response.revision_id.is_empty());

        let err = service
            .missing_profile(&ExploreMissingProfileRequest {
                path: merge.outputs[0].path.clone(),
                extra_paths: vec![],
                columns: vec!["zz".into()],
                transformation: None,
            })
            .expect_err("unknown column");
        assert!(err.to_string().contains("missing_elemental_column"));
    }

    #[test]
    fn crosstab_groups_by_a_measured_column_in_numeric_order() {
        let (service, dir, path) = service_with_merged_group();
        let response = service
            .crosstab(&ExploreCrosstabRequest {
                path: path.clone(),
                extra_paths: vec![],
                group_column: "as".into(),
                value_column: "fe".into(),
                summary_method: "mean".into(),
            })
            .expect("measured group column");
        let CrosstabRows::Summary { rows, .. } = response.rows else {
            panic!("expected summary rows");
        };
        let pairs: Vec<(Option<String>, Option<f64>)> =
            rows.into_iter().map(|r| (r.group, r.result)).collect();
        let some = |s: &str| Some(s.to_string());
        // Numeric order (1.5 < 2 < ... < 11), not text order ("11" < "2").
        assert_eq!(
            pairs,
            vec![
                (some("1.5"), Some(3.0)),
                (some("2"), Some(4.0)),
                (some("5"), Some(6.0)),
                (some("6"), Some(8.0)),
                (some("9"), Some(1.0)),
                (some("11"), Some(2.0)),
            ]
        );

        let counts = service
            .crosstab(&ExploreCrosstabRequest {
                path,
                extra_paths: vec![],
                group_column: "fe".into(),
                value_column: "as".into(),
                summary_method: "count".into(),
            })
            .expect("measured count crosstab");
        let CrosstabRows::Count { rows } = counts.rows else {
            panic!("expected count rows");
        };
        let groups: Vec<String> = rows.into_iter().filter_map(|r| r.group).collect();
        assert_eq!(groups, vec!["1", "2", "3", "4", "6", "8"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn histogram_equal_width_returns_the_requested_bin_count() {
        let (service, dir, path) = service_with_merged_group();
        let request = |binning| ExploreHistogramRequest {
            path: path.clone(),
            extra_paths: vec![],
            column: "as".into(),
            bins: 7,
            binning,
            transformation: None,
        };
        let equal = service
            .histogram(&request(HistogramBinning::EqualWidth))
            .expect("equal-width histogram");
        assert_eq!(equal.counts.len(), 7);
        assert_eq!(equal.counts.iter().sum::<u64>(), 6);
        let pretty = service
            .histogram(&request(HistogramBinning::Pretty))
            .expect("pretty histogram");
        assert_eq!(pretty.breaks.len(), pretty.counts.len() + 1);
        assert_eq!(pretty.counts.iter().sum::<u64>(), 6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_file_crosstab_groups_by_synthetic_file_column() {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-explore-multi-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("three.csv"),
            "anid,Site,Ware,as,fe\nA1,A,x,1.5,3\nA2,A,y,2,4\nB1,B,x,5,6\nB2,B,x,6,8\nC1,C,y,9,1\n",
        )
        .expect("write source");
        let import = crate::import::ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                sheet: None,
                source: "three.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
                group_name: None,
            })
            .expect("commit");
        let paths: Vec<String> = commit.groups.iter().map(|g| g.path.clone()).collect();
        let service = ExploreService::new(&dir).expect("explore service");
        let response = service
            .crosstab(&ExploreCrosstabRequest {
                path: paths[0].clone(),
                extra_paths: paths[1..].to_vec(),
                group_column: "Group".into(),
                value_column: "Ware".into(),
                summary_method: "count".into(),
            })
            .expect("multi-file crosstab");
        assert_eq!(response.path, paths[0]);
        let CrosstabRows::Count { rows } = response.rows else {
            panic!("expected count rows");
        };
        let triples: Vec<(Option<String>, Option<String>, u64)> = rows
            .into_iter()
            .map(|r| (r.group, r.value, r.count))
            .collect();
        let some = |s: &str| Some(s.to_string());
        assert_eq!(
            triples,
            vec![
                (some("A"), some("x"), 1),
                (some("A"), some("y"), 1),
                (some("B"), some("x"), 2),
                (some("C"), some("y"), 1),
            ]
        );

        // The synthetic column also resolves for a single file.
        let single = service
            .crosstab(&ExploreCrosstabRequest {
                path: paths[1].clone(),
                extra_paths: vec![],
                group_column: "Group".into(),
                value_column: "as".into(),
                summary_method: "mean".into(),
            })
            .expect("single-file crosstab");
        let CrosstabRows::Summary { rows, .. } = single.rows else {
            panic!("expected summary rows");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].group.as_deref(), Some("B"));
        assert_eq!(rows[0].result, Some(5.5));

        // A listed path twice is rejected.
        let err = service
            .crosstab(&ExploreCrosstabRequest {
                path: paths[0].clone(),
                extra_paths: vec![paths[0].clone()],
                group_column: "Group".into(),
                value_column: "Ware".into(),
                summary_method: "count".into(),
            })
            .expect_err("duplicate path");
        assert!(err.to_string().contains("duplicate_input_path"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
