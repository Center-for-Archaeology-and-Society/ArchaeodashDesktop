//! Ordination use cases (Phase 4, Section 8.5): PCA, LDA, and UMAP over one
//! group file's measured elemental values, optionally after an ephemeral
//! transformation. Results are returned to the caller and never persisted
//! (Section 5 storage invariant: no ordination values reach group files).

use std::path::PathBuf;

use archaeodash_analysis::{lda, pca, umap, ColumnMatrix, DEFAULT_SEED};
use archaeodash_contracts::{
    LdaRequest, LdaResponse, PcaRequest, PcaResponse, TransformationDefinition, UmapRequest,
    UmapResponse,
};
use archaeodash_data_io::{read_group_file, GroupFileData};
use archaeodash_domain::DomainError;

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

/// Ordination use cases rooted at one local project directory, sharing the
/// transform definition engine with `TransformService`.
pub struct OrdinationService {
    root: PathBuf,
}

impl OrdinationService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, DomainError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        Ok(Self { root })
    }

    /// Builds the ordination input matrix: raw measured elemental columns, or
    /// the requested columns of the transformed matrix when a definition is
    /// given (the definition's base transform and ratios run first).
    fn input_matrix(
        data: &GroupFileData,
        columns: &[String],
        transformation: Option<&TransformationDefinition>,
        context: &str,
    ) -> Result<ColumnMatrix, DomainError> {
        match transformation {
            None => TransformService::matrix_from_group(data, columns, context),
            Some(definition) => {
                let (full, _) = TransformService::apply_definition(data, definition, context)?;
                let mut names = Vec::with_capacity(columns.len());
                let mut cols = Vec::with_capacity(columns.len());
                for name in columns {
                    let idx = full.names.iter().position(|n| n == name).ok_or_else(|| {
                        validation(
                            "missing_ordination_column",
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

    /// Group labels for one descriptive column; every row needs a non-empty
    /// label because `MASS::lda` allows no NA groupings.
    fn group_labels(
        data: &GroupFileData,
        group_column: &str,
        context: &str,
    ) -> Result<Vec<String>, DomainError> {
        let roles = &data.profile.roles;
        let idx = roles
            .descriptive
            .iter()
            .position(|c| c == group_column)
            .ok_or_else(|| {
                validation(
                    "missing_group_column",
                    format!("{context}: column {group_column:?} is not a descriptive column"),
                )
            })?;
        let mut labels = Vec::with_capacity(data.rows.len());
        for row in &data.rows {
            match row.descriptive.get(idx) {
                Some(Some(value)) if !value.is_empty() => labels.push(value.clone()),
                _ => {
                    return Err(validation(
                        "lda_missing_group",
                        format!(
                            "{context}: row {} has a missing {group_column:?} label",
                            labels.len() + 1
                        ),
                    ))
                }
            }
        }
        Ok(labels)
    }

    /// prcomp-parity PCA (Section 15.4 procedure 6) over one group file.
    pub fn pca(&self, req: &PcaRequest) -> Result<PcaResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "ordination_empty",
                "PCA requires at least one column",
            ));
        }
        let data = read_group_file(&self.root.join(&req.path)).map_err(import_err)?;
        let matrix =
            Self::input_matrix(&data, &req.columns, req.transformation.as_ref(), &req.path)?;
        let result = pca(&matrix, req.scale)?;

        // Explained variance shares from sdev^2; zero total (degenerate
        // constant data with scale = false) keeps every share at zero.
        let k = result.sdev.len();
        let total: f64 = result.sdev.iter().map(|s| s * s).sum();
        let mut explained_variance = Vec::with_capacity(k);
        let mut cumulative_variance = Vec::with_capacity(k);
        let mut running = 0.0;
        for s in &result.sdev {
            let share = if total > 0.0 { s * s / total } else { 0.0 };
            explained_variance.push(share);
            running += share;
            cumulative_variance.push(running);
        }
        let score_names = (1..=k).map(|i| format!("PC{i}")).collect();

        Ok(PcaResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            column_names: req.columns.clone(),
            score_names,
            sdev: result.sdev,
            explained_variance,
            cumulative_variance,
            center: result.center,
            scale: result.scale,
            rotation: result.rotation,
            scores: result.scores,
        })
    }

    /// lda-parity LDA (Section 15.4 procedure 8) over one group file, with
    /// the legacy `validate_lda_groups` three-group minimum.
    pub fn lda(&self, req: &LdaRequest) -> Result<LdaResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "ordination_empty",
                "LDA requires at least one column",
            ));
        }
        let data = read_group_file(&self.root.join(&req.path)).map_err(import_err)?;
        let matrix =
            Self::input_matrix(&data, &req.columns, req.transformation.as_ref(), &req.path)?;
        let groups = Self::group_labels(&data, &req.group_column, &req.path)?;
        // Legacy `getLDA` calls `validate_lda_groups` with its default of 3.
        let result = lda(&matrix, &groups, 3)?;
        let rank = result.svd.len();
        let score_names = (1..=rank).map(|i| format!("LD{i}")).collect();

        Ok(LdaResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            column_names: req.columns.clone(),
            levels: result.levels,
            prior: result.prior,
            counts: result.counts.iter().map(|&c| c as u64).collect(),
            means: result.means,
            scaling: result.scaling,
            svd: result.svd,
            score_names,
            scores: result.scores,
            warnings: result.warnings,
        })
    }

    /// Legacy `umap::umap(method = "naive")` parity (Section 15.4 procedure
    /// 7, class D) over one group file, with a deterministic seed replacing
    /// the legacy unseeded global stream.
    pub fn umap(&self, req: &UmapRequest) -> Result<UmapResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "ordination_empty",
                "UMAP requires at least one column",
            ));
        }
        let data = read_group_file(&self.root.join(&req.path)).map_err(import_err)?;
        let matrix =
            Self::input_matrix(&data, &req.columns, req.transformation.as_ref(), &req.path)?;
        let seed = req.seed.unwrap_or(DEFAULT_SEED);
        let result = umap(&matrix, seed)?;
        let k = result.config.n_components;

        Ok(UmapResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            column_names: req.columns.clone(),
            score_names: (1..=k).map(|i| format!("V{i}")).collect(),
            embedding: result.layout,
            seed,
            n_neighbors: result.config.n_neighbors,
            n_epochs: result.config.n_epochs,
            a: result.config.a,
            b: result.config.b,
            warnings: result.warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use crate::groups::GroupService;
    use archaeodash_contracts::{
        ImportCommitRequest, ImputationMethod, MergeGroupsRequest, RatioMode, RatioSpecDto,
        TransformMethod, TransformationDefinition,
    };

    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Imports a three-group source (one file per group), merges the groups
    /// into one file that spans all levels, and returns the service, project
    /// root, and the merged group path for ordination.
    fn service_with_merged_group() -> (OrdinationService, std::path::PathBuf, String) {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-ordination-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as,fe\nA1,A,1.5,3\nA2,A,2,4\nB1,B,5,6\nB2,B,6,8\nC1,C,9,1\nC2,C,11,2\n",
        )
        .expect("write source");
        let import = crate::import::ImportService::new(&dir).expect("import service");
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
        let groups = GroupService::new(&dir).expect("group service");
        let sources: Vec<String> = commit.groups.iter().map(|g| g.path.clone()).collect();
        let merge = groups
            .merge_groups(&MergeGroupsRequest {
                sources,
                new_group_name: "Merged".into(),
            })
            .expect("merge");
        (
            OrdinationService::new(&dir).expect("ordination service"),
            dir,
            merge.outputs[0].path.clone(),
        )
    }

    #[test]
    fn pca_ephemeral_over_group_file() {
        let (service, dir, path) = service_with_merged_group();
        let before = std::fs::read(dir.join(&path)).expect("read group");
        let response = service
            .pca(&PcaRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                scale: false,
                transformation: None,
            })
            .expect("pca");
        assert_eq!(response.column_names, vec!["as", "fe"]);
        assert_eq!(response.score_names, vec!["PC1", "PC2"]);
        assert_eq!(response.sdev.len(), 2);
        assert_eq!(response.rotation.len(), 2); // variable-major rows
        assert_eq!(response.scores.len(), 6); // one row per analytical unit
        assert_eq!(response.center.len(), 2);
        assert!(response.scale.is_none());
        // Explained variance shares sum to one.
        let total: f64 = response.explained_variance.iter().sum();
        assert!((total - 1.0).abs() < 1e-9);
        assert!((response.cumulative_variance[1] - 1.0).abs() < 1e-9);
        // Ephemeral: the group file is byte-identical after ordination.
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("read group"),
            before,
            "group file byte-identical after PCA"
        );

        // Non-elemental columns are rejected by name.
        let err = service
            .pca(&PcaRequest {
                path,
                columns: vec!["cu".into()],
                scale: false,
                transformation: None,
            })
            .expect_err("non-elemental column");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "missing_elemental_column"
        ));
    }

    #[test]
    fn lda_reports_levels_priors_and_recentered_scores() {
        let (service, _dir, path) = service_with_merged_group();
        let response = service
            .lda(&LdaRequest {
                path,
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                transformation: None,
            })
            .expect("lda");
        assert_eq!(response.levels, vec!["A", "B", "C"]);
        assert_eq!(response.counts, vec![2, 2, 2]);
        assert_eq!(response.score_names, vec!["LD1", "LD2"]);
        assert_eq!(response.scores.len(), 6);
        assert_eq!(response.means.len(), 3);
        // Prior proportions in sorted-level order.
        for prior in &response.prior {
            assert!((prior - 1.0 / 3.0).abs() < 1e-12);
        }
        // Scores are re-centered per column (legacy capture convention).
        for c in 0..response.score_names.len() {
            let mean: f64 = response.scores.iter().map(|r| r[c]).sum::<f64>() / 6.0;
            assert!(mean.abs() < 1e-9);
        }
        assert!(response.warnings.is_empty());
    }

    #[test]
    fn lda_enforces_legacy_three_group_minimum() {
        // A fresh two-group project: the merged file holds only levels A/B,
        // so the legacy `validate_lda_groups` gate (min 3) fires.
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-ordination-2g-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("two.csv"),
            "anid,Site,as,fe\nA1,A,1.5,3\nA2,A,2,4\nB1,B,5,6\nB2,B,6,8\n",
        )
        .expect("write source");
        let import = crate::import::ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                source: "two.csv".into(),
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
            .merge_groups(&MergeGroupsRequest {
                sources,
                new_group_name: "Merged".into(),
            })
            .expect("merge");

        let service = OrdinationService::new(&dir).expect("ordination service");
        let err = service
            .lda(&LdaRequest {
                path: merge.outputs[0].path.clone(),
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                transformation: None,
            })
            .expect_err("two groups rejected");
        assert!(err
            .to_string()
            .contains("LDA requires at least 3 groups. Current selection has 2."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pca_accepts_transformed_matrix_input() {
        let (service, dir, path) = service_with_merged_group();
        let before = std::fs::read(dir.join(&path)).expect("read group");
        // zScore then PCA over the transformed columns plus a ratio output;
        // the definition applies on demand and is never persisted.
        let response = service
            .pca(&PcaRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into(), "as_fe".into()],
                scale: false,
                transformation: Some(TransformationDefinition {
                    name: "z".into(),
                    transform_method: TransformMethod::ZScore,
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
                }),
            })
            .expect("pca after transform");
        assert_eq!(response.column_names, vec!["as", "fe", "as_fe"]);
        assert_eq!(response.score_names, vec!["PC1", "PC2", "PC3"]);
        // Ephemeral: the merged group file is byte-identical afterwards, and
        // no transformation definition was persisted.
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("read group"),
            before,
            "group file byte-identical after ordination"
        );
        assert!(!dir.join(".archaeodash/transformations").exists());
    }

    #[test]
    fn lda_missing_group_file_is_internal_error() {
        let (service, _dir, _path) = service_with_merged_group();
        let err = service
            .lda(&LdaRequest {
                path: "groups/Missing.parquet".into(),
                columns: vec!["as".into()],
                group_column: "Site".into(),
                transformation: None,
            })
            .expect_err("missing group file");
        assert!(matches!(err, DomainError::Internal(_)));
    }
}
