//! Ordination use cases (Phase 4, Section 8.5): PCA, LDA, and UMAP over one
//! group file's measured elemental values (or several files concatenated
//! row-wise), optionally after an ephemeral transformation. No ordination
//! values reach group files (Section 5 storage invariant); results are kept
//! only in the disposable cache below.
//!
//! Cache (Section 6 `CacheStore`): each result is stored under
//! `.archaeodash/cache/ordination/<kind>/<key>.json`, keyed by the request,
//! every input file's size and modification time, the engine version, and
//! `CACHE_VERSION`. Any rewrite of an input file changes the key, so stale
//! results are never served; the Data Manager can precompute results that
//! pages then reuse instantly. The cache can be deleted at any time.

use std::path::{Path, PathBuf};

use archaeodash_analysis::{lda, pca, umap, ColumnMatrix, DEFAULT_SEED};
use archaeodash_contracts::{
    LdaRequest, LdaResponse, PcaRequest, PcaResponse, TransformationDefinition, UmapRequest,
    UmapResponse,
};
use archaeodash_data_io::{write_atomic, GroupFileData};
use archaeodash_domain::DomainError;
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};

use super::combined::CombinedInput;
use super::transforms::TransformService;

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

fn io_err(e: std::io::Error) -> DomainError {
    DomainError::Internal(Box::new(e))
}

/// Bump when an ordination algorithm or response shape changes so cached
/// results from older engines are never served.
const CACHE_VERSION: &str = "ordination-cache-1";
/// Most cached results kept per ordination kind; the oldest go first.
const CACHE_MAX_FILES: usize = 200;

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

    /// Reads `path` followed by each `extra_paths` entry as one combined
    /// input (`CombinedInput`: rows in request order, each file's rows in
    /// file order — the `GroupService::rows` order) and builds the ordination
    /// matrix over the pooled rows, so any transformation (e.g. z-score) is
    /// applied to all rows together. Every file must expose all requested
    /// columns.
    fn read_inputs(
        &self,
        path: &str,
        extra_paths: &[String],
        columns: &[String],
        transformation: Option<&TransformationDefinition>,
    ) -> Result<(CombinedInput, ColumnMatrix), DomainError> {
        let input = CombinedInput::read_local(&self.root, path, extra_paths)?;
        input.require_inputs(columns, transformation)?;
        let matrix = Self::input_matrix(&input.data, columns, transformation, &input.context())?;
        Ok((input, matrix))
    }

    /// Cache file for one request over `path` + `extra_paths`; `None` when an
    /// input file cannot be inspected (the computation then reports it).
    fn cache_file(
        &self,
        kind: &str,
        request: &impl Serialize,
        path: &str,
        extra_paths: &[String],
    ) -> Option<PathBuf> {
        let mut hasher = Sha256::new();
        hasher.update(CACHE_VERSION);
        hasher.update(env!("CARGO_PKG_VERSION"));
        hasher.update(kind);
        hasher.update(serde_json::to_vec(request).ok()?);
        for rel in std::iter::once(path).chain(extra_paths.iter().map(String::as_str)) {
            let meta = std::fs::metadata(self.root.join(rel)).ok()?;
            let modified = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos();
            hasher.update(rel.as_bytes());
            hasher.update([0]);
            hasher.update(meta.len().to_le_bytes());
            hasher.update(modified.to_le_bytes());
        }
        Some(
            self.root
                .join(".archaeodash/cache/ordination")
                .join(kind)
                .join(format!("{:x}.json", hasher.finalize())),
        )
    }

    fn cache_get<T: DeserializeOwned>(file: Option<&Path>) -> Option<T> {
        serde_json::from_slice(&std::fs::read(file?).ok()?).ok()
    }

    /// Best effort: a failed cache write never fails the ordination.
    fn cache_put<T: Serialize>(file: Option<&Path>, value: &T) {
        let Some(file) = file else { return };
        let Ok(bytes) = serde_json::to_vec(value) else {
            return;
        };
        if write_atomic(file, &bytes).is_err() {
            return;
        }
        let Some(dir) = file.parent() else { return };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        if files.len() > CACHE_MAX_FILES {
            files.sort();
            for (_, old) in &files[..files.len() - CACHE_MAX_FILES] {
                let _ = std::fs::remove_file(old);
            }
        }
    }

    /// PCA, served from the cache when this exact request already ran on
    /// the same input files.
    pub fn pca(&self, req: &PcaRequest) -> Result<PcaResponse, DomainError> {
        let file = self.cache_file("pca", req, &req.path, &req.extra_paths);
        if let Some(hit) = Self::cache_get(file.as_deref()) {
            return Ok(hit);
        }
        let result = self.compute_pca(req)?;
        Self::cache_put(file.as_deref(), &result);
        Ok(result)
    }

    /// LDA, served from the cache when possible (see [`Self::pca`]).
    pub fn lda(&self, req: &LdaRequest) -> Result<LdaResponse, DomainError> {
        let file = self.cache_file("lda", req, &req.path, &req.extra_paths);
        if let Some(hit) = Self::cache_get(file.as_deref()) {
            return Ok(hit);
        }
        let result = self.compute_lda(req)?;
        Self::cache_put(file.as_deref(), &result);
        Ok(result)
    }

    /// UMAP, served from the cache when possible (see [`Self::pca`]). UMAP
    /// is the slow one, so this is where precomputing pays off most.
    pub fn umap(&self, req: &UmapRequest) -> Result<UmapResponse, DomainError> {
        let file = self.cache_file("umap", req, &req.path, &req.extra_paths);
        if let Some(hit) = Self::cache_get(file.as_deref()) {
            return Ok(hit);
        }
        let result = self.compute_umap(req)?;
        Self::cache_put(file.as_deref(), &result);
        Ok(result)
    }

    /// prcomp-parity PCA (Section 15.4 procedure 6) over one group file.
    fn compute_pca(&self, req: &PcaRequest) -> Result<PcaResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "ordination_empty",
                "PCA requires at least one column",
            ));
        }
        let (input, matrix) = self.read_inputs(
            &req.path,
            &req.extra_paths,
            &req.columns,
            req.transformation.as_ref(),
        )?;
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
            revision_id: input.data.profile.revision_id.clone(),
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
    fn compute_lda(&self, req: &LdaRequest) -> Result<LdaResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "ordination_empty",
                "LDA requires at least one column",
            ));
        }
        let (input, matrix) = self.read_inputs(
            &req.path,
            &req.extra_paths,
            &req.columns,
            req.transformation.as_ref(),
        )?;
        let groups = if req.group_by_file {
            input.file_group_labels()
        } else {
            input.require_columns([req.group_column.as_str()])?;
            Self::group_labels(&input.data, &req.group_column, &input.context())?
        };
        // Legacy `getLDA` calls `validate_lda_groups` with its default of 3.
        let result = lda(&matrix, &groups, 3)?;
        let rank = result.svd.len();
        let score_names = (1..=rank).map(|i| format!("LD{i}")).collect();

        Ok(LdaResponse {
            path: req.path.clone(),
            revision_id: input.data.profile.revision_id.clone(),
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
    fn compute_umap(&self, req: &UmapRequest) -> Result<UmapResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "ordination_empty",
                "UMAP requires at least one column",
            ));
        }
        let (input, matrix) = self.read_inputs(
            &req.path,
            &req.extra_paths,
            &req.columns,
            req.transformation.as_ref(),
        )?;
        let seed = req.seed.unwrap_or(DEFAULT_SEED);
        let result = umap(&matrix, seed)?;
        let k = result.config.n_components;

        Ok(UmapResponse {
            path: req.path.clone(),
            revision_id: input.data.profile.revision_id.clone(),
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
    fn results_are_cached_per_request_and_input_file_state() {
        let (service, dir, path) = service_with_merged_group();
        let cache = dir.join(".archaeodash/cache/ordination/pca");
        let count = || std::fs::read_dir(&cache).map(|d| d.count()).unwrap_or(0);
        let request = PcaRequest {
            path: path.clone(),
            columns: vec!["as".into(), "fe".into()],
            scale: false,
            transformation: None,
            extra_paths: vec![],
        };
        let first = service.pca(&request).expect("pca");
        assert_eq!(count(), 1, "the first run stores its result");
        // A cached entry is what a repeat request returns (proved by
        // planting a marker in it).
        let entry = std::fs::read_dir(&cache)
            .expect("cache dir")
            .next()
            .expect("entry")
            .expect("entry")
            .path();
        let mut planted = first.clone();
        planted.revision_id = "from-cache".into();
        std::fs::write(&entry, serde_json::to_vec(&planted).expect("json")).expect("plant");
        assert_eq!(
            service.pca(&request).expect("cached").revision_id,
            "from-cache"
        );
        // A different request gets its own entry.
        service
            .pca(&PcaRequest {
                scale: true,
                ..request.clone()
            })
            .expect("scaled");
        assert_eq!(count(), 2);
        // Rewriting the group file invalidates its entries.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let bytes = std::fs::read(dir.join(&path)).expect("read group");
        std::fs::write(dir.join(&path), &bytes).expect("rewrite group");
        let fresh = service.pca(&request).expect("recomputed");
        assert_eq!(fresh.revision_id, first.revision_id);
        assert_eq!(count(), 3);
        // The cache never touches the group file itself.
        assert_eq!(std::fs::read(dir.join(&path)).expect("read group"), bytes);
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
                extra_paths: vec![],
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
                extra_paths: vec![],
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
                extra_paths: vec![],
                group_by_file: false,
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
                extra_paths: vec![],
                group_by_file: false,
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
                    precompute: Vec::new(),
                }),
                extra_paths: vec![],
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
                extra_paths: vec![],
                group_by_file: false,
            })
            .expect_err("missing group file");
        assert!(matches!(err, DomainError::Internal(_)));
    }

    /// Imports the three-group source without merging: one group file per
    /// site (A, B, C; two rows each), returned in import order.
    fn service_with_group_files() -> (OrdinationService, std::path::PathBuf, Vec<String>) {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-ordination-multi-{}-{n}",
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
        let paths = commit.groups.iter().map(|g| g.path.clone()).collect();
        (
            OrdinationService::new(&dir).expect("ordination service"),
            dir,
            paths,
        )
    }

    #[test]
    fn pca_over_multiple_group_files_concatenates_rows() {
        let (service, dir, paths) = service_with_group_files();
        assert_eq!(paths.len(), 3);
        let groups = GroupService::new(&dir).expect("group service");
        let expected_rows: usize = paths
            .iter()
            .map(|p| groups.rows(p).expect("rows").rows.len())
            .sum();
        let first_revision = groups.rows(&paths[0]).expect("rows").revision_id;
        let response = service
            .pca(&PcaRequest {
                path: paths[0].clone(),
                columns: vec!["as".into(), "fe".into()],
                scale: false,
                transformation: None,
                extra_paths: paths[1..].to_vec(),
            })
            .expect("multi-file pca");
        assert_eq!(expected_rows, 6);
        assert_eq!(response.scores.len(), expected_rows);
        assert_eq!(response.path, paths[0]);
        assert_eq!(response.revision_id, first_revision);
        // The center is the mean over the concatenated rows (all six units).
        assert!((response.center[0] - 34.5 / 6.0).abs() < 1e-12);
        assert!((response.center[1] - 24.0 / 6.0).abs() < 1e-12);

        // Listing a file twice is rejected rather than double-counting rows.
        let err = service
            .pca(&PcaRequest {
                path: paths[0].clone(),
                columns: vec!["as".into()],
                scale: false,
                transformation: None,
                extra_paths: vec![paths[0].clone()],
            })
            .expect_err("duplicate path");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "duplicate_input_path"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lda_group_by_file_uses_file_group_names() {
        let (service, dir, paths) = service_with_group_files();
        let response = service
            .lda(&LdaRequest {
                path: paths[0].clone(),
                columns: vec!["as".into(), "fe".into()],
                group_column: String::new(),
                transformation: None,
                extra_paths: paths[1..].to_vec(),
                group_by_file: true,
            })
            .expect("lda by file");
        assert_eq!(response.levels, vec!["A", "B", "C"]);
        assert_eq!(response.counts, vec![2, 2, 2]);
        assert_eq!(response.scores.len(), 6);

        // Same rows labelled by the descriptive Site column agree exactly.
        let by_column = service
            .lda(&LdaRequest {
                path: paths[0].clone(),
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                transformation: None,
                extra_paths: paths[1..].to_vec(),
                group_by_file: false,
            })
            .expect("lda by column");
        assert_eq!(by_column.levels, response.levels);
        assert_eq!(by_column.scores, response.scores);

        // The legacy three-group minimum still applies to file labels.
        let err = service
            .lda(&LdaRequest {
                path: paths[0].clone(),
                columns: vec!["as".into(), "fe".into()],
                group_column: String::new(),
                transformation: None,
                extra_paths: vec![paths[1].clone()],
                group_by_file: true,
            })
            .expect_err("two files rejected");
        assert!(err
            .to_string()
            .contains("LDA requires at least 3 groups. Current selection has 2."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_file_ordination_requires_columns_in_every_file() {
        let (service, dir, paths) = service_with_group_files();
        std::fs::write(
            dir.join("other.csv"),
            "anid,Site,as,cu\nD1,D,1,2\nD2,D,3,4\n",
        )
        .expect("write source");
        let import = crate::import::ImportService::new(&dir).expect("import service");
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
        let err = service
            .pca(&PcaRequest {
                path: paths[0].clone(),
                columns: vec!["as".into(), "fe".into()],
                scale: false,
                transformation: None,
                extra_paths: vec![other_path.clone()],
            })
            .expect_err("missing column in extra file");
        match err {
            DomainError::Validation {
                ref code,
                ref message,
                ..
            } => {
                assert_eq!(code, "missing_elemental_column");
                assert!(message.contains(&other_path), "{message}");
                assert!(message.contains("\"fe\""), "{message}");
            }
            other => panic!("unexpected error {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
