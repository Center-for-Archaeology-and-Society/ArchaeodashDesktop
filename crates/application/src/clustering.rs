//! Clustering, membership-probability, and nearest-match use cases
//! (Phase 6, golden parity procedures 9-11; IMPLEMENTATION.md Section 8):
//! k-means/PAM/ward.D2/DIANA fits, the WSS and silhouette diagnostic series,
//! `group.mem.probs`, and `calcEDistance` over one group file, optionally
//! after an ephemeral transformation. Results are returned to the caller and
//! never persisted (Section 5 storage invariant: no analysis values reach
//! group files).

use std::path::PathBuf;
use std::path::{Component, Path};

use archaeodash_analysis::{
    get_eligible, lda, pca, CancellationToken, ColumnMatrix, DistanceMetric, EuclideanMatch,
    LinkageMethod, MembershipMethod,
};
use archaeodash_contracts::{
    AnalysisSourceDto, ClusterDiagnosticsRequest, ClusterDiagnosticsResponse,
    ClusterDistanceMetricDto, ClusterFitRequest, ClusterFitResponse, ClusterLinkageDto,
    ClusterMethod, EuclideanMatchDto, EuclideanMatchesRequest, EuclideanMatchesResponse,
    MembershipMethodDto, MembershipProbabilitiesRequest, MembershipProbabilitiesResponse,
    TransformationDefinition,
};
use archaeodash_data_io::{read_group_file, GroupFileData};
use archaeodash_domain::DomainError;

use super::transforms::TransformService;

/// Legacy `set.seed(seed + k)` base seed of the diagnostic series (golden
/// capture procedure 9).
const DEFAULT_KMEANS_SEED: i64 = 20260914;
/// Keep pairwise algorithms below predictable memory bounds (an f64 matrix
/// alone uses 8 bytes per cell, with several temporary matrices in PAM).
const MAX_DISTANCE_CELLS: usize = 1_000_000;
const MAX_INPUT_CELLS: usize = 4_000_000;
const MAX_ORDINATION_FEATURES: usize = 512;

fn checked_seed(seed: i64, context: &str) -> Result<i32, DomainError> {
    i32::try_from(seed).map_err(|_| {
        validation(
            "cluster_seed_range",
            format!("{context}: seed must fit a signed 32-bit integer"),
        )
    })
}

fn validation(code: &str, message: impl Into<String>) -> DomainError {
    DomainError::validation(code, message)
}

fn io_err(e: std::io::Error) -> DomainError {
    DomainError::Internal(Box::new(e))
}

fn import_err(e: archaeodash_data_io::ImportError) -> DomainError {
    DomainError::Internal(Box::new(e))
}

/// Clustering/membership use cases rooted at one local project directory,
/// sharing the transform definition engine with `TransformService`.
pub struct ClusterService {
    root: PathBuf,
}

impl ClusterService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, DomainError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        let root = std::fs::canonicalize(root).map_err(io_err)?;
        Ok(Self { root })
    }

    /// Resolve an existing project file and reject both lexical traversal and
    /// symlinks that leave the canonical project root.
    fn group_path(&self, rel: &str) -> Result<PathBuf, DomainError> {
        let mut path = self.root.clone();
        for component in Path::new(rel).components() {
            match component {
                Component::Normal(part) => path.push(part),
                Component::CurDir => {}
                other => {
                    return Err(validation(
                        "cluster_path",
                        format!("path {rel:?} escapes the project boundary: {other:?}"),
                    ))
                }
            }
        }
        let resolved = std::fs::canonicalize(path).map_err(io_err)?;
        if !resolved.starts_with(&self.root) {
            return Err(validation(
                "cluster_path",
                format!("path {rel:?} escapes the project boundary"),
            ));
        }
        Ok(resolved)
    }

    fn read_group(&self, rel: &str) -> Result<GroupFileData, DomainError> {
        let path = self.group_path(rel)?;
        archaeodash_data_io::check_group_read_limits(
            &path,
            1_000_000,
            MAX_INPUT_CELLS as u64,
            128 * 1024 * 1024,
        )
        .map_err(|error| match error {
            archaeodash_data_io::ImportError::Limit(message) => {
                validation("cluster_resource_limit", message)
            }
            other => import_err(other),
        })?;
        read_group_file(&path).map_err(import_err)
    }

    fn validate_dimensions(
        n: usize,
        p: usize,
        context: &str,
        needs_distances: bool,
    ) -> Result<(), DomainError> {
        let cells = n.saturating_mul(p);
        if cells > MAX_INPUT_CELLS {
            return Err(validation(
                "cluster_resource_limit",
                format!("{context}: input has {cells} cells; limit is {MAX_INPUT_CELLS}"),
            ));
        }
        if needs_distances && n.saturating_mul(n) > MAX_DISTANCE_CELLS {
            return Err(validation(
                "cluster_resource_limit",
                format!("{context}: {n} rows exceed the pairwise distance limit"),
            ));
        }
        Ok(())
    }

    /// Builds the analysis input matrix: raw measured elemental columns, or
    /// the requested columns of the transformed matrix when a definition is
    /// given (same ephemeral semantics as the ordination input).
    fn input_matrix(
        data: &GroupFileData,
        columns: &[String],
        transformation: Option<&TransformationDefinition>,
        context: &str,
    ) -> Result<archaeodash_analysis::ColumnMatrix, DomainError> {
        match transformation {
            None => TransformService::matrix_from_group(data, columns, context),
            Some(definition) => {
                let (full, _) = TransformService::apply_definition(data, definition, context)?;
                let mut names = Vec::with_capacity(columns.len());
                let mut cols = Vec::with_capacity(columns.len());
                for name in columns {
                    let idx = full.names.iter().position(|n| n == name).ok_or_else(|| {
                        validation(
                            "missing_cluster_column",
                            format!("{context}: column {name:?} is absent after transformation"),
                        )
                    })?;
                    names.push(full.names[idx].clone());
                    cols.push(full.cols[idx].clone());
                }
                Ok(archaeodash_analysis::ColumnMatrix { names, cols })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn source_matrix(
        data: &GroupFileData,
        columns: &[String],
        transformation: Option<&TransformationDefinition>,
        source: AnalysisSourceDto,
        pc_count: Option<u32>,
        source_group_column: Option<&str>,
        umap_seed: Option<u64>,
        context: &str,
        cancel: &CancellationToken,
    ) -> Result<ColumnMatrix, DomainError> {
        cancel.check()?;
        if pc_count == Some(0) {
            return Err(validation(
                "cluster_pc_count",
                "component count must be at least 1",
            ));
        }
        let estimated_features = transformation.map_or(columns.len(), |definition| {
            definition
                .elemental_columns
                .len()
                .saturating_add(definition.ratios.len().saturating_mul(3))
                .max(columns.len())
        });
        let cells = data.rows.len().saturating_mul(estimated_features);
        if cells > MAX_INPUT_CELLS {
            return Err(validation(
                "cluster_resource_limit",
                format!("{context}: analysis input has {cells} cells; limit is {MAX_INPUT_CELLS}"),
            ));
        }
        if source != AnalysisSourceDto::Elements && estimated_features > MAX_ORDINATION_FEATURES {
            return Err(validation("cluster_source_resource_limit", format!(
                "{context}: ordination input has {estimated_features} features; limit is {MAX_ORDINATION_FEATURES}"
            )));
        }
        if source == AnalysisSourceDto::Umap
            && data.rows.len().saturating_mul(data.rows.len()) > MAX_DISTANCE_CELLS
        {
            return Err(validation(
                "cluster_resource_limit",
                format!(
                "{context}: UMAP input exceeds the pairwise limit of {MAX_DISTANCE_CELLS} cells"
            ),
            ));
        }
        match (source, pc_count) {
            (AnalysisSourceDto::Pca, Some(count))
                if count as usize > data.rows.len().min(estimated_features) =>
            {
                return Err(validation("cluster_pc_count", format!(
                    "{context}: PCA component count exceeds the maximum supported input rank {}", data.rows.len().min(estimated_features)
                )));
            }
            (AnalysisSourceDto::Umap, Some(count)) if count > 2 => {
                return Err(validation(
                    "cluster_pc_count",
                    format!("{context}: UMAP provides at most 2 dimensions, got {count}"),
                ));
            }
            _ => {}
        }
        let base = Self::input_matrix(data, columns, transformation, context)?;
        let (names, values) = match source {
            AnalysisSourceDto::Elements => return Ok(base),
            AnalysisSourceDto::Pca => {
                let fitted = pca(&base, false)?;
                cancel.check()?;
                ("PC".to_string(), fitted.scores)
            }
            AnalysisSourceDto::Umap => {
                let fitted = archaeodash_analysis::umap_cancellable(
                    &base,
                    umap_seed.unwrap_or(20260914),
                    cancel,
                )?;
                ("V".to_string(), fitted.layout)
            }
            AnalysisSourceDto::Lda => {
                let group_column = source_group_column.ok_or_else(|| {
                    validation(
                        "cluster_source_group_required",
                        "LDA source requires source_group_column",
                    )
                })?;
                let groups = Self::group_labels(data, group_column, context)?;
                if let Some(count) = pc_count {
                    let levels: std::collections::HashSet<&str> =
                        groups.iter().map(String::as_str).collect();
                    let max_components = base.cols.len().min(levels.len().saturating_sub(1));
                    if count as usize > max_components {
                        return Err(validation(
                            "cluster_pc_count",
                            format!("{context}: LDA component count exceeds available rank {max_components}"),
                        ));
                    }
                }
                let fitted = lda(&base, &groups, 3)?;
                cancel.check()?;
                ("LD".to_string(), fitted.scores)
            }
        };
        let available = values.first().map_or(0, Vec::len);
        let count = pc_count.map(|n| n as usize).unwrap_or(available);
        if count == 0 || count > available {
            return Err(validation(
                "cluster_pc_count",
                format!("{context}: component count must be in 1..={available}, got {count}"),
            ));
        }
        Ok(ColumnMatrix {
            names: (1..=count).map(|i| format!("{names}{i}")).collect(),
            cols: (0..count)
                .map(|j| values.iter().map(|row| row[j]).collect())
                .collect(),
        })
    }

    fn distance_metric(
        metric: ClusterDistanceMetricDto,
        p: f64,
    ) -> Result<DistanceMetric, DomainError> {
        match metric {
            ClusterDistanceMetricDto::Euclidean => Ok(DistanceMetric::Euclidean),
            ClusterDistanceMetricDto::Manhattan => Ok(DistanceMetric::Manhattan),
            ClusterDistanceMetricDto::Maximum => Ok(DistanceMetric::Maximum),
            ClusterDistanceMetricDto::Minkowski if p.is_finite() && p >= 1.0 => {
                Ok(DistanceMetric::Minkowski { p })
            }
            ClusterDistanceMetricDto::Minkowski => Err(validation(
                "cluster_metric_parameter",
                "Minkowski p must be finite and >= 1",
            )),
        }
    }

    fn linkage_method(linkage: ClusterLinkageDto) -> LinkageMethod {
        match linkage {
            ClusterLinkageDto::Average => LinkageMethod::Average,
            ClusterLinkageDto::Complete => LinkageMethod::Complete,
            ClusterLinkageDto::WardD => LinkageMethod::WardD,
            ClusterLinkageDto::WardD2 => LinkageMethod::WardD2,
        }
    }

    fn metric_distances(
        rows: &[Vec<f64>],
        metric: DistanceMetric,
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f64>>, DomainError> {
        let n = rows.len();
        let mut distances = vec![vec![0.0; n]; n];
        for i in 0..n {
            cancel.check()?;
            for j in (i + 1)..n {
                let d = match metric {
                    DistanceMetric::Euclidean => rows[i]
                        .iter()
                        .zip(&rows[j])
                        .map(|(a, b)| (a - b) * (a - b))
                        .sum::<f64>()
                        .sqrt(),
                    DistanceMetric::Manhattan => rows[i]
                        .iter()
                        .zip(&rows[j])
                        .map(|(a, b)| (a - b).abs())
                        .sum(),
                    DistanceMetric::Maximum => rows[i]
                        .iter()
                        .zip(&rows[j])
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0, f64::max),
                    DistanceMetric::Minkowski { p } => rows[i]
                        .iter()
                        .zip(&rows[j])
                        .map(|(a, b)| (a - b).abs().powf(p))
                        .sum::<f64>()
                        .powf(1.0 / p),
                };
                distances[i][j] = d;
                distances[j][i] = d;
            }
        }
        Ok(distances)
    }

    /// factoextra `.get_withinSS`: sum each cluster's lower-triangle squared
    /// pairwise distances and divide by cluster size. For Euclidean data this
    /// equals the centroid WSS and also matches `fviz_nbclust(..., pam)`.
    fn within_pair_ss(dist: &[Vec<f64>], labels: &[i32], k: usize) -> f64 {
        (1..=k)
            .map(|cluster| {
                let members: Vec<usize> = labels
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &label)| (label as usize == cluster).then_some(i))
                    .collect();
                if members.is_empty() {
                    return 0.0;
                }
                let mut pair_sum = 0.0;
                for i in 0..members.len() {
                    for j in 0..i {
                        pair_sum += dist[members[i]][members[j]].powi(2);
                    }
                }
                pair_sum / members.len() as f64
            })
            .sum()
    }

    fn cluster_plot(matrix: &ColumnMatrix) -> (Vec<[f64; 2]>, Vec<String>, Option<String>) {
        let n = matrix.n_rows();
        if matrix.cols.len() > 2 && matrix.cols.len() <= MAX_ORDINATION_FEATURES {
            if let Ok(result) = pca(matrix, true) {
                if result.scores.first().is_some_and(|row| row.len() >= 2) {
                    return (
                        result.scores.iter().map(|row| [row[0], row[1]]).collect(),
                        vec!["PC1".into(), "PC2".into()],
                        None,
                    );
                }
            }
        }
        let raw: Vec<[f64; 2]> = (0..n)
            .map(|i| {
                [
                    matrix.cols.first().map_or(0.0, |column| column[i]),
                    matrix.cols.get(1).map_or(0.0, |column| column[i]),
                ]
            })
            .collect();
        if matrix.cols.len() > MAX_ORDINATION_FEATURES {
            return (
                raw,
                matrix.names.iter().take(2).cloned().collect(),
                Some(
                    "Cluster plot PCA skipped because the input exceeds the 512-feature plot limit"
                        .into(),
                ),
            );
        }
        if n < 2 {
            return (raw, matrix.names.iter().take(2).cloned().collect(), Some("Cluster plot used raw axes because at least two rows are required for standardization".into()));
        }
        let standardized: Option<Vec<[f64; 2]>> = (0..matrix.cols.len().min(2))
            .map(|j| {
                let col = &matrix.cols[j];
                let mean = col.iter().sum::<f64>() / n as f64;
                let sd =
                    (col.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt();
                (sd.is_finite() && sd > 0.0)
                    .then(|| col.iter().map(|v| (v - mean) / sd).collect::<Vec<_>>())
            })
            .collect::<Option<Vec<_>>>()
            .map(|cols| {
                (0..n)
                    .map(|i| {
                        [
                            cols.first().map_or(0.0, |col| col[i]),
                            cols.get(1).map_or(0.0, |col| col[i]),
                        ]
                    })
                    .collect()
            });
        match standardized {
            Some(coords) => (coords, matrix.names.iter().take(2).cloned().collect(), None),
            None => (raw, matrix.names.iter().take(2).cloned().collect(), Some("Cluster plot used raw axes because one or more plot dimensions have zero variance".into())),
        }
    }

    /// Rejects NA/non-finite cells: the clustering ports run on a complete
    /// (NaN-free) numeric matrix, like R `prcomp`/`stats::dist` inputs.
    fn require_complete(
        matrix: &archaeodash_analysis::ColumnMatrix,
        context: &str,
    ) -> Result<(), DomainError> {
        for (j, name) in matrix.names.iter().enumerate() {
            if matrix.cols[j].iter().any(|v| !v.is_finite()) {
                return Err(validation(
                    "cluster_missing_values",
                    format!("{context}: column {name:?} has NA values"),
                ));
            }
        }
        Ok(())
    }

    /// Row-major copy of the matrix for the distance-matrix helpers.
    fn rows(matrix: &archaeodash_analysis::ColumnMatrix) -> Vec<Vec<f64>> {
        let n = matrix.n_rows();
        let p = matrix.cols.len();
        (0..n)
            .map(|i| (0..p).map(|j| matrix.cols[j][i]).collect())
            .collect()
    }

    /// Group labels for one descriptive column; blank cells keep the legacy
    /// `as.character(NA)` tolerance (they never match an eligible group).
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
        Ok(data
            .rows
            .iter()
            .map(|r| {
                r.descriptive
                    .get(idx)
                    .cloned()
                    .flatten()
                    .unwrap_or_default()
            })
            .collect())
    }

    /// Sample ID values for one ID column: the legacy `rowid` key column,
    /// the visible ID column, or any descriptive column. Missing values
    /// become empty strings (`as.character(NA)` renderings).
    fn id_values(
        data: &GroupFileData,
        id_column: &str,
        context: &str,
    ) -> Result<Vec<String>, DomainError> {
        let roles = &data.profile.roles;
        if id_column == roles.legacy_rowid {
            return Ok(data
                .rows
                .iter()
                .enumerate()
                .map(|(index, r)| {
                    r.legacy_rowid
                        .clone()
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| (index + 1).to_string())
                })
                .collect());
        }
        if id_column == roles.visible_id {
            return Ok(data
                .rows
                .iter()
                .map(|r| r.visible.clone().unwrap_or_default())
                .collect());
        }
        if let Some(idx) = roles.descriptive.iter().position(|c| c == id_column) {
            return Ok(data
                .rows
                .iter()
                .map(|r| {
                    r.descriptive
                        .get(idx)
                        .cloned()
                        .flatten()
                        .unwrap_or_default()
                })
                .collect());
        }
        Err(validation(
            "missing_id_column",
            format!("{context}: column {id_column:?} is not an available ID column"),
        ))
    }

    /// Stable row keys for `calcEDistance`: the legacy `rowid` column when
    /// present, else the hidden analytical UUID (still unique, so the
    /// self-exclusion and per-observation grouping stay correct).
    fn rowid_keys(data: &GroupFileData) -> Vec<String> {
        data.rows
            .iter()
            .map(|r| {
                r.legacy_rowid
                    .clone()
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| r.uuid.to_string())
            })
            .collect()
    }

    /// The WSS elbow and mean-silhouette diagnostic series (golden capture
    /// procedure 9): `wss[k]` for `k = 1..=max_k` via seeded k-means
    /// (`set.seed(seed + k)`, `iter.max = 100`, `nstart = 25`; `k = 1` is the
    /// grand-mean total sum of squares) and the mean silhouette width for
    /// `k = 2..=max_k`. `max_k` clamps to `2..=min(n - 1, 20)`.
    pub fn cluster_diagnostics(
        &self,
        req: &ClusterDiagnosticsRequest,
    ) -> Result<ClusterDiagnosticsResponse, DomainError> {
        self.cluster_diagnostics_cancellable(req, &CancellationToken::default())
    }

    pub fn cluster_diagnostics_cancellable(
        &self,
        req: &ClusterDiagnosticsRequest,
        cancel: &CancellationToken,
    ) -> Result<ClusterDiagnosticsResponse, DomainError> {
        cancel.check()?;
        if req.columns.is_empty() {
            return Err(validation(
                "cluster_empty",
                "clustering requires at least one column",
            ));
        }
        let data = self.read_group(&req.path)?;
        Self::validate_dimensions(data.rows.len(), req.columns.len(), &req.path, true)?;
        let matrix = Self::source_matrix(
            &data,
            &req.columns,
            req.transformation.as_ref(),
            req.source,
            req.pc_count,
            req.source_group_column.as_deref(),
            req.umap_seed,
            &req.path,
            cancel,
        )?;
        Self::require_complete(&matrix, &req.path)?;
        let n = matrix.n_rows();
        Self::validate_dimensions(n, matrix.cols.len(), &req.path, true)?;
        if n < 3 {
            return Err(validation(
                "cluster_k_range",
                format!(
                    "{}: clustering diagnostics require at least three rows, got {n}",
                    req.path
                ),
            ));
        }
        let max_k = (req.max_k as usize).clamp(2, 20).min(n - 1);
        let seed = checked_seed(req.seed, &req.path)?;
        if seed.checked_add(max_k as i32).is_none() {
            return Err(validation(
                "cluster_seed_range",
                format!(
                    "{}: seed plus diagnostic k exceeds signed 32-bit range",
                    req.path
                ),
            ));
        }
        let rows = Self::rows(&matrix);
        let metric = Self::distance_metric(req.metric, req.minkowski_p)?;
        let dist = Self::metric_distances(&rows, metric, cancel)?;
        let mut wss = Vec::with_capacity(max_k);
        let mut silhouette = Vec::with_capacity(max_k.saturating_sub(1));
        for k in 1..=max_k {
            cancel.check()?;
            let (labels, total) = match req.diagnostic_method {
                archaeodash_contracts::ClusterDiagnosticMethodDto::Kmeans => {
                    if metric != DistanceMetric::Euclidean {
                        return Err(validation(
                            "cluster_metric_method",
                            "k-means diagnostics use Euclidean distance",
                        ));
                    }
                    let fit = archaeodash_analysis::kmeans_cancellable(
                        &matrix,
                        k,
                        100,
                        25,
                        seed.saturating_add(k as i32),
                        cancel,
                    )?;
                    (fit.cluster, fit.tot_withinss)
                }
                archaeodash_contracts::ClusterDiagnosticMethodDto::Pam => {
                    let fit = archaeodash_analysis::pam_with_metric_cancellable(
                        &matrix, k, metric, cancel,
                    )?;
                    let total = Self::within_pair_ss(&dist, &fit.clustering, k);
                    (fit.clustering, total)
                }
            };
            wss.push(total);
            if k >= 2 {
                let mean =
                    archaeodash_analysis::silhouette_mean_cancellable(&dist, &labels, k, cancel)?;
                silhouette.push(if mean.is_nan() { None } else { Some(mean) });
            }
        }
        Ok(ClusterDiagnosticsResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            column_names: matrix.names.clone(),
            source: req.source,
            metric: req.metric,
            linkage: req.linkage,
            diagnostic_method: req.diagnostic_method,
            n_rows: n as u64,
            wss,
            silhouette,
        })
    }

    /// One clustering fit (`kmeans`, `pam`, `hclust ward.D2`, or `diana`)
    /// over one group file, with per-row silhouette widths for the
    /// partitioning families.
    pub fn cluster_fit(&self, req: &ClusterFitRequest) -> Result<ClusterFitResponse, DomainError> {
        self.cluster_fit_cancellable(req, &CancellationToken::default())
    }

    pub fn cluster_fit_cancellable(
        &self,
        req: &ClusterFitRequest,
        cancel: &CancellationToken,
    ) -> Result<ClusterFitResponse, DomainError> {
        cancel.check()?;
        if req.method == ClusterMethod::HclustWardD2
            && (req.metric != ClusterDistanceMetricDto::Euclidean
                || req.linkage != ClusterLinkageDto::WardD2)
        {
            return Err(validation(
                "cluster_metric_method",
                "hclust_ward_d2 requires Euclidean distance and Ward.D2 linkage",
            ));
        }
        if req.columns.is_empty() {
            return Err(validation(
                "cluster_empty",
                "clustering requires at least one column",
            ));
        }
        let data = self.read_group(&req.path)?;
        Self::validate_dimensions(data.rows.len(), req.columns.len(), &req.path, true)?;
        let matrix = Self::source_matrix(
            &data,
            &req.columns,
            req.transformation.as_ref(),
            req.source,
            req.pc_count,
            req.source_group_column.as_deref(),
            req.umap_seed,
            &req.path,
            cancel,
        )?;
        Self::require_complete(&matrix, &req.path)?;
        let n = matrix.n_rows();
        Self::validate_dimensions(n, matrix.cols.len(), &req.path, true)?;

        let plot_coordinates: Vec<[f64; 2]> = (0..n)
            .map(|i| {
                [
                    matrix.cols.first().map_or(0.0, |column| column[i]),
                    matrix.cols.get(1).map_or(0.0, |column| column[i]),
                ]
            })
            .collect();
        let plot_column_names = matrix.names.iter().take(2).cloned().collect();
        let (cluster_plot_coordinates, cluster_plot_column_names, plot_warning) = match req.method {
            ClusterMethod::Kmeans | ClusterMethod::Pam => Self::cluster_plot(&matrix),
            _ => (Vec::new(), Vec::new(), None),
        };
        let mut response = ClusterFitResponse {
            path: req.path.clone(),
            analytical_uuids: data.rows.iter().map(|r| r.uuid.to_string()).collect(),
            revision_id: data.profile.revision_id.clone(),
            method: req.method,
            source: req.source,
            column_names: matrix.names.clone(),
            metric: req.metric,
            linkage: req.linkage,
            plot_coordinates,
            plot_column_names,
            plot_groups: match req.plot_group_column.as_deref() {
                Some(column) => Self::group_labels(&data, column, &req.path)?,
                None => Vec::new(),
            },
            cluster_plot_coordinates,
            cluster_plot_column_names,
            plot_warning,
            n_rows: n as u64,
            cluster: None,
            size: None,
            tot_withinss: None,
            centers: None,
            medoids: None,
            merge: None,
            height: None,
            order: None,
            silhouette: None,
        };
        match req.method {
            ClusterMethod::Kmeans => {
                if req.metric != ClusterDistanceMetricDto::Euclidean {
                    return Err(validation(
                        "cluster_metric_method",
                        "k-means uses Euclidean distance",
                    ));
                }
                let k = Self::required_k(req.k, "kmeans")?;
                let seed = checked_seed(req.seed.unwrap_or(DEFAULT_KMEANS_SEED), &req.path)?;
                if req.iter_max == 0 || req.iter_max > 1_000 || req.nstart == 0 || req.nstart > 100
                {
                    return Err(validation(
                        "cluster_kmeans_parameters",
                        "iter_max must be 1..=1000 and nstart must be 1..=100",
                    ));
                }
                let fit = archaeodash_analysis::kmeans_cancellable(
                    &matrix,
                    k,
                    req.iter_max as usize,
                    req.nstart as usize,
                    seed,
                    cancel,
                )?;
                response.cluster = Some(fit.cluster.iter().map(|&c| c as u32).collect());
                response.size = Some(fit.size.iter().map(|&s| s as u32).collect());
                response.tot_withinss = Some(fit.tot_withinss);
                response.centers = Some(fit.centers);
                response.silhouette = Some(Self::row_silhouettes(
                    &matrix,
                    &fit.cluster,
                    k,
                    DistanceMetric::Euclidean,
                    cancel,
                )?);
            }
            ClusterMethod::Pam => {
                let k = Self::required_k(req.k, "pam")?;
                let fit = archaeodash_analysis::pam_with_metric_cancellable(
                    &matrix,
                    k,
                    Self::distance_metric(req.metric, req.minkowski_p)?,
                    cancel,
                )?;
                response.cluster = Some(fit.clustering.iter().map(|&c| c as u32).collect());
                response.medoids = Some(fit.medoids.iter().map(|&m| m as u32).collect());
                response.silhouette = Some(Self::row_silhouettes(
                    &matrix,
                    &fit.clustering,
                    k,
                    Self::distance_metric(req.metric, req.minkowski_p)?,
                    cancel,
                )?);
            }
            ClusterMethod::HclustWardD2 => {
                let fit = archaeodash_analysis::hclust_ward_d2_cancellable(&matrix, cancel)?;
                response.merge = Some(fit.merge);
                response.height = Some(fit.height);
                response.order = Some(fit.order.iter().map(|&o| o as u32).collect());
            }
            ClusterMethod::Hclust => {
                let fit = archaeodash_analysis::hclust_cancellable(
                    &matrix,
                    Self::distance_metric(req.metric, req.minkowski_p)?,
                    Self::linkage_method(req.linkage),
                    cancel,
                )?;
                response.merge = Some(fit.merge);
                response.height = Some(fit.height);
                response.order = Some(fit.order.iter().map(|&o| o as u32).collect());
            }
            ClusterMethod::Diana => {
                let fit = archaeodash_analysis::diana_with_metric_cancellable(
                    &matrix,
                    Self::distance_metric(req.metric, req.minkowski_p)?,
                    cancel,
                )?;
                response.merge = Some(fit.merge);
                response.height = Some(fit.height);
                response.order = Some(fit.order.iter().map(|&o| o as u32).collect());
            }
        }
        Ok(response)
    }

    /// `k` is required for the partitioning families.
    fn required_k(k: Option<u32>, method: &str) -> Result<usize, DomainError> {
        k.map(|k| k as usize).ok_or_else(|| {
            validation(
                "cluster_k_range",
                format!("{method} requires an explicit k"),
            )
        })
    }

    /// Per-row silhouette widths for one 1-based clustering; `null` where
    /// NaN (trivial `k`); singleton widths are zero.
    fn row_silhouettes(
        matrix: &archaeodash_analysis::ColumnMatrix,
        clustering: &[i32],
        k: usize,
        metric: DistanceMetric,
        cancel: &CancellationToken,
    ) -> Result<Vec<Option<f64>>, DomainError> {
        let rows = Self::rows(matrix);
        let dist = Self::metric_distances(&rows, metric, cancel)?;
        Ok(
            archaeodash_analysis::silhouette_widths_cancellable(&dist, clustering, k, cancel)?
                .into_iter()
                .map(|w| if w.is_nan() { None } else { Some(w) })
                .collect(),
        )
    }

    /// `group.mem.probs` over one group file: eligibility via `getEligible`
    /// (`n > max(n_features, n_groups) + 1`), per-row probability cells in
    /// eligible-group order, best group, and the effective method (the
    /// legacy Hotellings-to-Mahalanobis `tryCatch` fallback).
    pub fn membership_probabilities(
        &self,
        req: &MembershipProbabilitiesRequest,
    ) -> Result<MembershipProbabilitiesResponse, DomainError> {
        self.membership_probabilities_cancellable(req, &CancellationToken::default())
    }

    pub fn membership_probabilities_cancellable(
        &self,
        req: &MembershipProbabilitiesRequest,
        cancel: &CancellationToken,
    ) -> Result<MembershipProbabilitiesResponse, DomainError> {
        cancel.check()?;
        let data = self.read_group(&req.path)?;
        Self::validate_dimensions(data.rows.len(), req.columns.len(), &req.path, false)?;
        let matrix = Self::source_matrix(
            &data,
            &req.columns,
            req.transformation.as_ref(),
            req.source,
            req.pc_count,
            req.source_group_column.as_deref(),
            req.umap_seed,
            &req.path,
            cancel,
        )?;
        if matrix.cols.len() > MAX_ORDINATION_FEATURES {
            return Err(validation(
                "cluster_source_resource_limit",
                format!(
                    "{}: membership supports at most {MAX_ORDINATION_FEATURES} analysis columns",
                    req.path
                ),
            ));
        }
        let ids = Self::id_values(&data, &req.id_column, &req.path)?;
        let groups = Self::group_labels(&data, &req.group_column, &req.path)?;
        let mut eligible = get_eligible(&groups, matrix.cols.len());
        if let Some(selected) = &req.projection_groups {
            if selected.iter().any(|group| !groups.contains(group)) {
                return Err(validation(
                    "membership_projection_group",
                    "projection_groups contains an unknown group",
                ));
            }
            eligible.retain(|group| selected.contains(group));
        }
        if data.rows.len().saturating_mul(eligible.len()) > MAX_DISTANCE_CELLS {
            return Err(validation(
                "cluster_resource_limit",
                format!("{}: membership result exceeds the cell limit", req.path),
            ));
        }
        let method = match req.method {
            MembershipMethodDto::Hotellings => MembershipMethod::Hotellings,
            MembershipMethodDto::Mahalanobis => MembershipMethod::Mahalanobis,
        };
        let (table, effective) = archaeodash_analysis::group_mem_probs_tracked_cancellable(
            &ids,
            &groups,
            &req.group_column,
            &matrix,
            &matrix.names,
            &eligible,
            method,
            cancel,
        )?;
        let non_finite_null = |v: f64| if v.is_finite() { Some(v) } else { None };
        Ok(MembershipProbabilitiesResponse {
            path: req.path.clone(),
            source: req.source,
            column_names: matrix.names.clone(),
            analytical_uuids: data.rows.iter().map(|r| r.uuid.to_string()).collect(),
            revision_id: data.profile.revision_id.clone(),
            effective_method: match effective {
                MembershipMethod::Hotellings => MembershipMethodDto::Hotellings,
                MembershipMethod::Mahalanobis => MembershipMethodDto::Mahalanobis,
            },
            requested_method: req.method,
            fallback_reason: (req.method == MembershipMethodDto::Hotellings
                && effective == MembershipMethod::Mahalanobis)
                .then(|| "hotellings_computation_failed".to_string()),
            projection_included: groups
                .iter()
                .map(|group| {
                    req.projection_groups
                        .as_ref()
                        .is_none_or(|selected| selected.contains(group))
                })
                .collect(),
            eligible_groups: table.eligible,
            ids: table.rows.iter().map(|r| r.id.clone()).collect(),
            groups: table.rows.iter().map(|r| r.group_val.clone()).collect(),
            probabilities: table
                .rows
                .iter()
                .map(|r| r.probs.iter().map(|&v| non_finite_null(v)).collect())
                .collect(),
            best_group: table.rows.iter().map(|r| r.best_group.clone()).collect(),
            best_value: table
                .rows
                .iter()
                .map(|r| r.best_value.and_then(non_finite_null))
                .collect(),
            in_group: table
                .rows
                .iter()
                .map(|r| r.in_group.unwrap_or(false))
                .collect(),
        })
    }

    /// `calcEDistance` over one group file: nearest matches by Euclidean
    /// distance with self-exclusion, the legacy per-observation
    /// `slice_head(limit)` before the cross-group filter, and the final
    /// `arrange(observation, distance)`. With no projection selection the
    /// legacy UI default applies: every distinct non-blank group.
    pub fn euclidean_matches(
        &self,
        req: &EuclideanMatchesRequest,
    ) -> Result<EuclideanMatchesResponse, DomainError> {
        self.euclidean_matches_cancellable(req, &CancellationToken::default())
    }

    pub fn euclidean_matches_cancellable(
        &self,
        req: &EuclideanMatchesRequest,
        cancel: &CancellationToken,
    ) -> Result<EuclideanMatchesResponse, DomainError> {
        cancel.check()?;
        if !(1..=100).contains(&req.limit) {
            return Err(validation(
                "euclidean_limit_range",
                "limit must lie between 1 and 100",
            ));
        }
        let data = self.read_group(&req.path)?;
        Self::validate_dimensions(data.rows.len(), req.columns.len(), &req.path, false)?;
        if data.rows.len().saturating_mul(data.rows.len()) > MAX_DISTANCE_CELLS {
            return Err(validation(
                "cluster_resource_limit",
                format!(
                    "{}: Euclidean matching exceeds the pair-comparison quota",
                    req.path
                ),
            ));
        }
        let matrix = Self::source_matrix(
            &data,
            &req.columns,
            req.transformation.as_ref(),
            req.source,
            req.pc_count,
            req.source_group_column.as_deref(),
            req.umap_seed,
            &req.path,
            cancel,
        )?;
        // Hidden analytical UUIDs uniquely identify observations even when
        // user-visible legacy rowid values are duplicated. Use them for the
        // distance helper's self-exclusion/grouping, then restore output keys.
        let internal_rowids: Vec<String> = data.rows.iter().map(|r| r.uuid.to_string()).collect();
        let output_rowids = Self::rowid_keys(&data);
        let visible_rowids: std::collections::HashMap<String, String> = data
            .rows
            .iter()
            .zip(output_rowids.iter())
            .map(|(row, key)| (row.uuid.to_string(), key.clone()))
            .collect();
        let ids = Self::id_values(&data, &req.id_column, &req.path)?;
        let groups = Self::group_labels(&data, &req.group_column, &req.path)?;
        let mut projection: Vec<String> = match &req.projection_groups {
            Some(selected) => selected.iter().filter(|g| !g.is_empty()).cloned().collect(),
            None => groups.iter().filter(|g| !g.is_empty()).cloned().collect(),
        };
        projection.sort();
        projection.dedup();
        if projection.iter().any(|group| !groups.contains(group)) {
            return Err(validation(
                "euclidean_projection_group",
                "projection_groups contains an unknown group",
            ));
        }
        let matches: Vec<EuclideanMatch> = archaeodash_analysis::calc_e_distance_cancellable(
            &internal_rowids,
            &ids,
            &groups,
            &matrix,
            &projection,
            req.limit as usize,
            req.within_group,
            cancel,
        )?;
        Ok(EuclideanMatchesResponse {
            path: req.path.clone(),
            source: req.source,
            column_names: matrix.names.clone(),
            revision_id: data.profile.revision_id.clone(),
            rows: matches
                .into_iter()
                .map(|m: EuclideanMatch| EuclideanMatchDto {
                    rowid: visible_rowids
                        .get(&m.rowid)
                        .cloned()
                        .unwrap_or_else(|| m.rowid.clone()),
                    analytical_uuid: m.rowid,
                    match_analytical_uuid: m.match_rowid.clone(),
                    id: m.id,
                    match_id: m.match_id,
                    distance: m.distance.is_finite().then_some(m.distance),
                    group: m.group,
                    match_group: m.match_group,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use crate::groups::GroupService;
    use crate::import::ImportService;
    use archaeodash_contracts::{ImportCommitRequest, MergeGroupsRequest};
    use archaeodash_data_io::{read_group_file, write_group_rows};

    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Thirty rows cycling through five groups over two elemental columns
    /// (`as` strictly increasing, `fe` near-constant): every row's nearest
    /// neighbours are cross-group, which keeps the Euclidean top-3 stable
    /// through the legacy slice_head-then-filter order, and six rows per
    /// group stay below the `get_eligible` threshold (n > max(nc, ng) + 1).
    fn service_with_merged_group() -> (ClusterService, std::path::PathBuf, String) {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-clustering-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let sites = ["A", "B", "C", "D", "E"];
        let mut rows = String::from("anid,Site,as,fe\n");
        for i in 0..30 {
            rows.push_str(&format!(
                "X{i},{},{},{}\n",
                sites[i % 5],
                1.0 + f64::from(i as u32) * 0.1,
                3.0 + f64::from((i % 7) as u32) * 0.05
            ));
        }
        std::fs::write(dir.join("mini.csv"), rows).expect("write source");
        let import = ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                source: "mini.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
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
            ClusterService::new(&dir).expect("cluster service"),
            dir,
            merge.outputs[0].path.clone(),
        )
    }

    #[test]
    fn diagnostics_series_shapes_clamps_and_is_ephemeral() {
        let (service, dir, path) = service_with_merged_group();
        let before = std::fs::read(dir.join(&path)).expect("read group");
        let response = service
            .cluster_diagnostics(&ClusterDiagnosticsRequest {
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Euclidean,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::WardD2,
                diagnostic_method: archaeodash_contracts::ClusterDiagnosticMethodDto::Kmeans,
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                max_k: 10,
                seed: 20260914,
            })
            .expect("diagnostics");
        assert_eq!(response.n_rows, 30);
        assert_eq!(response.column_names, vec!["as", "fe"]);
        // max_k clamps to min(n - 1, 20) = 20 -> 10 stays; k = 1..=10 wss,
        // k = 2..=10 silhouette.
        assert_eq!(response.wss.len(), 10);
        assert_eq!(response.silhouette.len(), 9);
        assert!(response.silhouette.iter().all(|s| s.is_some()));
        // WSS decreases (weakly) with k and k = 1 is the total sum of squares.
        assert!(response.wss[0] > response.wss[1]);
        assert!(response.wss.windows(2).all(|w| w[0] >= w[1]));
        // Deterministic in the seed.
        let again = service
            .cluster_diagnostics(&ClusterDiagnosticsRequest {
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Euclidean,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::WardD2,
                diagnostic_method: archaeodash_contracts::ClusterDiagnosticMethodDto::Kmeans,
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                max_k: 10,
                seed: 20260914,
            })
            .expect("diagnostics again");
        assert_eq!(response.wss, again.wss);
        // Ephemeral: the group file is byte-identical afterwards.
        assert_eq!(std::fs::read(dir.join(&path)).expect("read group"), before);

        let pam_diagnostics = service
            .cluster_diagnostics(&ClusterDiagnosticsRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Manhattan,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::Average,
                diagnostic_method: archaeodash_contracts::ClusterDiagnosticMethodDto::Pam,
                max_k: 4,
                seed: 20260914,
            })
            .expect("PAM Manhattan diagnostics");
        assert_eq!(
            pam_diagnostics.diagnostic_method,
            archaeodash_contracts::ClusterDiagnosticMethodDto::Pam
        );
        assert_eq!(pam_diagnostics.metric, ClusterDistanceMetricDto::Manhattan);
        assert_eq!(pam_diagnostics.wss.len(), 4);
        assert_eq!(pam_diagnostics.silhouette.len(), 3);

        // max_k above the n - 1 and 20 ceilings clamps instead of failing.
        let clamped = service
            .cluster_diagnostics(&ClusterDiagnosticsRequest {
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Euclidean,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::WardD2,
                diagnostic_method: archaeodash_contracts::ClusterDiagnosticMethodDto::Kmeans,
                path,
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                max_k: 500,
                seed: 1,
            })
            .expect("clamped");
        assert_eq!(clamped.wss.len(), 20);
        assert_eq!(clamped.silhouette.len(), 19);

        // NA cells are rejected before any fitting.
        let (service, dir, path) = service_with_merged_group();
        let err = service
            .cluster_fit(&ClusterFitRequest {
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Euclidean,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::WardD2,
                path,
                columns: vec!["cu".into()],
                transformation: None,
                method: ClusterMethod::Kmeans,
                k: Some(2),
                iter_max: 100,
                nstart: 25,
                seed: None,
                plot_group_column: None,
            })
            .expect_err("non-elemental column");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "missing_elemental_column"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_paths_outside_project_including_symlinks() {
        let (service, dir, path) = service_with_merged_group();
        let outside = dir
            .parent()
            .expect("parent")
            .join(format!("outside-{}", std::process::id()));
        std::fs::write(&outside, b"not a group").expect("outside file");
        std::os::unix::fs::symlink(&outside, dir.join("escape.parquet")).expect("symlink");
        for request_path in ["../outside", "escape.parquet"] {
            let err = service
                .cluster_fit(&ClusterFitRequest {
                    source: AnalysisSourceDto::Elements,
                    pc_count: None,
                    source_group_column: None,
                    umap_seed: None,
                    metric: ClusterDistanceMetricDto::Euclidean,
                    minkowski_p: 2.0,
                    linkage: ClusterLinkageDto::WardD2,
                    path: request_path.into(),
                    columns: vec!["as".into()],
                    transformation: None,
                    method: ClusterMethod::Kmeans,
                    k: Some(2),
                    iter_max: 100,
                    nstart: 25,
                    seed: None,
                    plot_group_column: None,
                })
                .expect_err("outside path rejected");
            assert!(
                matches!(err, DomainError::Validation { ref code, .. } if code == "cluster_path")
            );
        }
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = path;
    }

    #[test]
    fn fit_kmeans_pam_hclust_diana_shapes() {
        let (service, _dir, path) = service_with_merged_group();
        let request = |method, k: Option<u32>| ClusterFitRequest {
            source: AnalysisSourceDto::Elements,
            pc_count: None,
            source_group_column: None,
            umap_seed: None,
            metric: ClusterDistanceMetricDto::Euclidean,
            minkowski_p: 2.0,
            linkage: ClusterLinkageDto::WardD2,
            path: path.clone(),
            columns: vec!["as".into(), "fe".into()],
            transformation: None,
            method,
            k,
            iter_max: 100,
            nstart: 25,
            seed: Some(20260914),
            plot_group_column: None,
        };
        let kmeans_fit = service
            .cluster_fit(&request(ClusterMethod::Kmeans, Some(3)))
            .expect("kmeans");
        assert_eq!(kmeans_fit.n_rows, 30);
        assert_eq!(kmeans_fit.analytical_uuids.len(), 30);
        assert_eq!(
            kmeans_fit
                .analytical_uuids
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            30
        );
        let cluster = kmeans_fit.cluster.expect("kmeans cluster");
        assert_eq!(cluster.len(), 30);
        assert!(cluster.iter().all(|&c| (1..=3).contains(&c)));
        let size = kmeans_fit.size.expect("kmeans size");
        assert_eq!(size.iter().sum::<u32>(), 30);
        let centers = kmeans_fit.centers.expect("kmeans centers");
        assert_eq!(centers.len(), 3);
        assert_eq!(centers[0].len(), 2);
        assert!(kmeans_fit.tot_withinss.expect("totss").is_finite());
        let sil = kmeans_fit.silhouette.expect("kmeans silhouette");
        assert_eq!(sil.len(), 30);
        assert!(sil.iter().all(Option::is_some));

        let pam_fit = service
            .cluster_fit(&request(ClusterMethod::Pam, Some(3)))
            .expect("pam");
        assert_eq!(pam_fit.cluster.expect("pam cluster").len(), 30);
        let medoids = pam_fit.medoids.expect("pam medoids");
        assert_eq!(medoids.len(), 3);
        assert!(medoids.iter().all(|&m| (1..=30).contains(&m)));
        assert!(pam_fit.centers.is_none() && pam_fit.merge.is_none());
        assert!(pam_fit.silhouette.expect("pam silhouette").len() == 30);

        for method in [ClusterMethod::HclustWardD2, ClusterMethod::Diana] {
            let fit = service.cluster_fit(&request(method, None)).expect("fit");
            assert_eq!(fit.method, method);
            let merge = fit.merge.expect("merge");
            assert_eq!(merge.len(), 29);
            let order = fit.order.expect("order");
            assert_eq!(order.len(), 30);
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (1..=30).collect::<Vec<u32>>());
            assert_eq!(fit.height.expect("height").len(), 29);
            assert!(fit.cluster.is_none() && fit.silhouette.is_none());
            // R merge coding: negative leaves, positive stage refs.
            assert!(merge
                .iter()
                .all(|row| row.iter().all(|&v| v <= 29 && v != 0)));
        }

        // k is required for the partitioning families and bounded by n.
        for method in [ClusterMethod::Kmeans, ClusterMethod::Pam] {
            let err = service
                .cluster_fit(&request(method, None))
                .expect_err("missing k");
            assert!(matches!(
                err,
                DomainError::Validation { ref code, .. } if code == "cluster_k_range"
            ));
        }
        let err = service
            .cluster_fit(&request(ClusterMethod::Pam, Some(30)))
            .expect_err("k >= n");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "pam_input"
        ));
        // Small-n diagnostics gate: two rows cannot span k = 2..=min(n-1, 20).
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-clustering-tiny-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("tiny.csv"),
            "anid,Site,as,fe\nA1,A,1,3\nA2,A,2,4\n",
        )
        .expect("write source");
        let import = ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                source: "tiny.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
            })
            .expect("commit");
        let service = ClusterService::new(&dir).expect("cluster service");
        let err = service
            .cluster_diagnostics(&ClusterDiagnosticsRequest {
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Euclidean,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::WardD2,
                diagnostic_method: archaeodash_contracts::ClusterDiagnosticMethodDto::Kmeans,
                path: commit.groups[0].path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                max_k: 10,
                seed: 1,
            })
            .expect_err("two rows rejected");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "cluster_k_range"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pca_source_limits_components_and_preserves_row_identity_revision_and_storage() {
        let (service, dir, path) = service_with_merged_group();
        let before = std::fs::read(dir.join(&path)).expect("source bytes");
        let data = read_group_file(&dir.join(&path)).expect("source rows");
        let expected_ids: Vec<String> = data.rows.iter().map(|row| row.uuid.to_string()).collect();
        let revision = data.profile.revision_id.clone();
        let mut request = ClusterFitRequest {
            path: path.clone(),
            columns: vec!["as".into(), "fe".into()],
            transformation: None,
            source: AnalysisSourceDto::Pca,
            pc_count: Some(1),
            source_group_column: None,
            umap_seed: None,
            metric: ClusterDistanceMetricDto::Euclidean,
            minkowski_p: 2.0,
            linkage: ClusterLinkageDto::WardD2,
            method: ClusterMethod::Kmeans,
            k: Some(3),
            iter_max: 100,
            nstart: 25,
            seed: Some(15),
            plot_group_column: None,
        };
        let fit = service.cluster_fit(&request).expect("PCA fit");
        assert_eq!(fit.analytical_uuids, expected_ids);
        assert_eq!(fit.revision_id, revision);
        assert_eq!(fit.column_names, vec!["PC1"]);
        assert_eq!(fit.cluster.as_ref().expect("clusters").len(), 30);
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("source bytes unchanged"),
            before
        );

        request.pc_count = Some(3);
        let err = service.cluster_fit(&request).expect_err("invalid PC count");
        assert!(
            matches!(err, DomainError::Validation { ref code, .. } if code == "cluster_pc_count")
        );
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("source remains unchanged"),
            before
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn umap_and_lda_sources_recompute_ephemerally_and_euclidean_projects_groups() {
        let (service, dir, path) = service_with_merged_group();
        let before = std::fs::read(dir.join(&path)).expect("source bytes");
        for (source, source_group_column, pc_count) in [
            (AnalysisSourceDto::Umap, None, Some(1)),
            (AnalysisSourceDto::Lda, Some("Site".to_string()), Some(1)),
        ] {
            let fit = service
                .cluster_fit(&ClusterFitRequest {
                    path: path.clone(),
                    columns: vec!["as".into(), "fe".into()],
                    transformation: None,
                    source,
                    pc_count,
                    source_group_column,
                    umap_seed: Some(20260914),
                    metric: ClusterDistanceMetricDto::Euclidean,
                    minkowski_p: 2.0,
                    linkage: ClusterLinkageDto::WardD2,
                    method: ClusterMethod::Kmeans,
                    k: Some(3),
                    iter_max: 100,
                    nstart: 25,
                    seed: Some(20260914),
                    plot_group_column: None,
                })
                .expect("ephemeral derived-source clustering");
            assert_eq!(fit.source, source);
            assert_eq!(fit.column_names.len(), 1);
            assert_eq!(fit.n_rows, 30);
            assert_eq!(fit.analytical_uuids.len(), 30);
        }
        let err = service
            .cluster_fit(&ClusterFitRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                source: AnalysisSourceDto::Lda,
                pc_count: Some(1),
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Euclidean,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::WardD2,
                method: ClusterMethod::Kmeans,
                k: Some(3),
                iter_max: 100,
                nstart: 25,
                seed: None,
                plot_group_column: None,
            })
            .expect_err("LDA requires group");
        assert!(
            matches!(err, DomainError::Validation { ref code, .. } if code == "cluster_source_group_required")
        );

        let response = service
            .euclidean_matches(&EuclideanMatchesRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                projection_groups: Some(vec!["A".into(), "B".into()]),
                group_column: "Site".into(),
                id_column: "anid".into(),
                limit: 3,
                within_group: true,
            })
            .expect("projected Euclidean candidates");
        assert!(response
            .rows
            .iter()
            .all(|row| ["A", "B"].contains(&row.match_group.as_str())));
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("source remains ephemeral"),
            before
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Membership fixture: four elemental columns keep the five groups
    /// eligible (`n = 6 > max(4, 5) + 1 = 6` is false — so this fixture uses
    /// six rows per group, n = 30 stays, but the eligible rule is exercised
    /// through a three-group file with eight rows per group).
    fn membership_fixture() -> (ClusterService, std::path::PathBuf, String) {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-membership-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mut rows = String::from("anid,Site,as,fe,co,zn\n");
        let mut i = 0u32;
        for site in ["A", "B", "C"] {
            for _ in 0..8 {
                let v = f64::from(i);
                rows.push_str(&format!(
                    "X{i},{site},{},{},{},{}\n",
                    2.0 + (v * 0.37).sin(),
                    3.0 + (v * 0.23).cos(),
                    5.0 + (v * 0.61 + 0.3).sin(),
                    2.0 + (v * 0.47 + 0.8).cos()
                ));
                i += 1;
            }
        }
        std::fs::write(dir.join("mem.csv"), rows).expect("write source");
        let import = ImportService::new(&dir).expect("import service");
        let commit = import
            .commit(&ImportCommitRequest {
                source: "mem.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
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
            ClusterService::new(&dir).expect("cluster service"),
            dir,
            merge.outputs[0].path.clone(),
        )
    }

    #[test]
    fn membership_hotellings_matches_rows_and_eligibility() {
        let (service, _dir, path) = membership_fixture();
        let response = service
            .membership_probabilities(&MembershipProbabilitiesRequest {
                transformation: None,
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                projection_groups: None,
                path: path.clone(),
                columns: vec!["as".into(), "fe".into(), "co".into(), "zn".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                method: MembershipMethodDto::Hotellings,
            })
            .expect("membership");
        assert_eq!(response.path, path);
        assert_eq!(response.effective_method, MembershipMethodDto::Hotellings);
        // n = 24 rows, 4 features, 3 groups: threshold n > max(4, 3) + 1 = 5,
        // every 8-row group is eligible, sorted byte order.
        assert_eq!(response.eligible_groups, vec!["A", "B", "C"]);
        assert_eq!(response.ids.len(), 24);
        assert_eq!(response.analytical_uuids.len(), 24);
        assert_eq!(
            response
                .analytical_uuids
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            24
        );
        assert_eq!(response.groups.len(), 24);
        assert_eq!(response.probabilities.len(), 24);
        assert_eq!(response.probabilities[0].len(), 3);
        // Probability cells are round(p, 5) * 100 in [0, 100].
        for cell in response.probabilities.iter().flatten().flatten() {
            assert!((0.0..=100.0).contains(cell));
        }
        // Best group over the non-NA cells; InGroup mirrors the comparison.
        for (i, best) in response.best_group.iter().enumerate() {
            let best = best.as_ref().expect("best group");
            let pick = response
                .eligible_groups
                .iter()
                .position(|g| g == best)
                .expect("eligible");
            assert_eq!(response.probabilities[i][pick], response.best_value[i]);
            assert_eq!(response.in_group[i], *best == response.groups[i]);
        }
    }

    #[test]
    fn membership_pca_source_uses_requested_pc_count_and_validates_it() {
        let (service, dir, path) = membership_fixture();
        let before = std::fs::read(dir.join(&path)).expect("source bytes");
        let mut request = MembershipProbabilitiesRequest {
            path: path.clone(),
            columns: vec!["as".into(), "fe".into(), "co".into(), "zn".into()],
            transformation: None,
            source: AnalysisSourceDto::Pca,
            pc_count: Some(1),
            source_group_column: None,
            umap_seed: None,
            projection_groups: None,
            group_column: "Site".into(),
            id_column: "anid".into(),
            method: MembershipMethodDto::Mahalanobis,
        };
        let response = service
            .membership_probabilities(&request)
            .expect("PCA membership");
        assert_eq!(response.source, AnalysisSourceDto::Pca);
        assert_eq!(response.column_names, vec!["PC1"]);
        assert_eq!(response.analytical_uuids.len(), 24);
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("unchanged source"),
            before
        );

        request.pc_count = Some(5);
        let err = service
            .membership_probabilities(&request)
            .expect_err("excess component count");
        assert!(
            matches!(err, DomainError::Validation { ref code, .. } if code == "cluster_pc_count")
        );
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("still unchanged"),
            before
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn membership_falls_back_to_mahalanobis_and_reports_effective_method() {
        let (service, _dir, path) = membership_fixture();
        let response = service
            .membership_probabilities(&MembershipProbabilitiesRequest {
                transformation: None,
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                projection_groups: None,
                path: path.clone(),
                columns: vec!["as".into(), "fe".into(), "co".into(), "zn".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                method: MembershipMethodDto::Mahalanobis,
            })
            .expect("membership");
        assert_eq!(response.effective_method, MembershipMethodDto::Mahalanobis);
        assert_eq!(response.eligible_groups, vec!["A", "B", "C"]);
        // The self row is excluded from its own reference group, so the
        // diagonal comparison against the own group is well defined but the
        // table shape stays one cell per (row, eligible group).
        assert_eq!(response.probabilities.len(), 24);
    }

    #[test]
    fn membership_rejects_when_no_group_is_eligible() {
        // 2 rows per group: n = 2 <= max(2, 3) + 1, nothing is eligible.
        let (service, dir, path) = service_with_merged_group();
        let err = service
            .membership_probabilities(&MembershipProbabilitiesRequest {
                transformation: None,
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                projection_groups: None,
                path,
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                method: MembershipMethodDto::Hotellings,
            })
            .expect_err("no eligible groups");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "membership_no_eligible_groups"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn euclidean_matches_respect_limit_and_within_group() {
        let (service, dir, path) = service_with_merged_group();
        let before = std::fs::read(dir.join(&path)).expect("read group");
        let request = |limit: u32, within_group: bool| EuclideanMatchesRequest {
            transformation: None,
            source: AnalysisSourceDto::Elements,
            pc_count: None,
            source_group_column: None,
            umap_seed: None,
            projection_groups: None,
            path: path.clone(),
            columns: vec!["as".into(), "fe".into()],
            group_column: "Site".into(),
            id_column: "anid".into(),
            limit,
            within_group,
        };
        let response = service
            .euclidean_matches(&request(3, false))
            .expect("matches");
        assert_eq!(response.path, path);
        assert!(!response.revision_id.is_empty());
        // 30 observations x 3 cross-group matches each (the interleaved
        // fixture keeps the top-3 nearest neighbours cross-group).
        assert_eq!(response.rows.len(), 30 * 3);
        for row in &response.rows {
            assert_ne!(row.rowid, row.match_id, "self excluded");
            assert_ne!(row.group, row.match_group, "cross-group only");
        }
        // Ordered by observation ID then distance.
        for w in response.rows.windows(2) {
            let same_obs = w[0].id == w[1].id;
            assert!(
                w[0].id < w[1].id || (same_obs && w[0].distance <= w[1].distance),
                "rows sorted by (observation, distance)"
            );
        }
        // within_group = true keeps every non-self pair under the cap:
        // 30 observations x 29 candidates.
        let within = service
            .euclidean_matches(&request(100, true))
            .expect("within-group matches");
        assert_eq!(within.rows.len(), 30 * 29);
        // Ephemeral: the group file is byte-identical afterwards.
        assert_eq!(std::fs::read(dir.join(&path)).expect("read group"), before);

        // Limit outside 1..=100 is a validation error.
        let err = service
            .euclidean_matches(&request(0, false))
            .expect_err("limit zero");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "euclidean_limit_range"
        ));
        let err = service
            .euclidean_matches(&request(101, false))
            .expect_err("limit above 100");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "euclidean_limit_range"
        ));

        // Unknown ID column is a validation error.
        let mut bad = request(5, false);
        bad.id_column = "nope".into();
        let err = service.euclidean_matches(&bad).expect_err("bad id column");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "missing_id_column"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_legacy_rowids_do_not_merge_observation_identity() {
        let (service, dir, path) = service_with_merged_group();
        let file = dir.join(&path);
        let mut data = read_group_file(&file).expect("read group");
        data.rows[0].legacy_rowid = Some("duplicate".into());
        data.rows[1].legacy_rowid = Some("duplicate".into());
        data.rows[0].visible = Some("duplicate".into());
        data.rows[1].visible = Some("duplicate".into());
        let uuids = [data.rows[0].uuid.to_string(), data.rows[1].uuid.to_string()];
        write_group_rows(&file, data.profile, &data.rows).expect("write duplicated rowids");

        let response = service
            .euclidean_matches(&EuclideanMatchesRequest {
                transformation: None,
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                projection_groups: None,
                path,
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                limit: 100,
                within_group: true,
            })
            .expect("matches");
        for uuid in &uuids {
            assert!(response.rows.iter().any(|row| {
                row.analytical_uuid == *uuid
                    && row.match_analytical_uuid != *uuid
                    && uuids.contains(&row.match_analytical_uuid)
                    && row.id == "duplicate"
                    && row.match_id == "duplicate"
                    && row.rowid == "duplicate"
            }), "each observation remains correlated with its UUID-backed match despite duplicate visible and legacy IDs");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_legacy_rowid_display_id_falls_back_to_ordinal_not_uuid() {
        let (service, dir, path) = service_with_merged_group();
        let file = dir.join(&path);
        let mut data = read_group_file(&file).expect("read group");
        let rowid = data.profile.roles.legacy_rowid.clone();
        let ids: Vec<String> = data.rows.iter().map(|row| row.uuid.to_string()).collect();
        for row in &mut data.rows {
            row.legacy_rowid = None;
        }
        write_group_rows(&file, data.profile, &data.rows).expect("write missing rowids");
        let response = service
            .euclidean_matches(&EuclideanMatchesRequest {
                path,
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                projection_groups: None,
                group_column: "Site".into(),
                id_column: rowid,
                limit: 1,
                within_group: true,
            })
            .expect("matches with ordinal display IDs");
        assert!(!response.rows.is_empty());
        assert!(response.rows.iter().all(|row| !ids.contains(&row.id)));
        assert!(response.rows.iter().any(|row| row.id == "1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_cells_return_null_distance_and_round_trip_json() {
        let (service, dir, path) = service_with_merged_group();
        let file = dir.join(&path);
        let mut data = read_group_file(&file).expect("read group");
        data.rows[0].elemental[0] = None;
        write_group_rows(&file, data.profile, &data.rows).expect("write missing cell");

        let response = service
            .euclidean_matches(&EuclideanMatchesRequest {
                transformation: None,
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                projection_groups: None,
                path,
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                limit: 100,
                within_group: true,
            })
            .expect("legacy distance matching accepts missing cells");
        assert!(response
            .rows
            .iter()
            .any(|row| row.id == "X0" && row.distance.is_none()));
        let json = serde_json::to_string(&response).expect("serialize null distances");
        assert!(json.contains("\"distance\":null"));
        let round_trip: EuclideanMatchesResponse =
            serde_json::from_str(&json).expect("deserialize null distance");
        assert_eq!(round_trip.rows.len(), response.rows.len());
        assert!(round_trip
            .rows
            .iter()
            .any(|row| row.id == "X0" && row.distance.is_none()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resource_limits_accept_boundary_and_reject_overage() {
        assert!(ClusterService::validate_dimensions(1_000, 1, "boundary", true).is_ok());
        assert!(matches!(
            ClusterService::validate_dimensions(1_001, 1, "over", true),
            Err(DomainError::Validation { ref code, .. }) if code == "cluster_resource_limit"
        ));
        assert!(ClusterService::validate_dimensions(1, MAX_INPUT_CELLS, "boundary", false).is_ok());
        assert!(matches!(
            ClusterService::validate_dimensions(1, MAX_INPUT_CELLS + 1, "over", false),
            Err(DomainError::Validation { ref code, .. }) if code == "cluster_resource_limit"
        ));
    }

    #[test]
    fn rejects_seeds_that_would_be_truncated_to_i32() {
        let (service, dir, path) = service_with_merged_group();
        let err = service
            .cluster_fit(&ClusterFitRequest {
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Euclidean,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::WardD2,
                path: path.clone(),
                columns: vec!["as".into()],
                transformation: None,
                method: ClusterMethod::Kmeans,
                k: Some(2),
                iter_max: 100,
                nstart: 25,
                seed: Some(i64::from(i32::MAX) + 1),
                plot_group_column: None,
            })
            .expect_err("out of range seed");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "cluster_seed_range"
        ));
        let err = service
            .cluster_diagnostics(&ClusterDiagnosticsRequest {
                source: AnalysisSourceDto::Elements,
                pc_count: None,
                source_group_column: None,
                umap_seed: None,
                metric: ClusterDistanceMetricDto::Euclidean,
                minkowski_p: 2.0,
                linkage: ClusterLinkageDto::WardD2,
                diagnostic_method: archaeodash_contracts::ClusterDiagnosticMethodDto::Kmeans,
                path,
                columns: vec!["as".into()],
                transformation: None,
                max_k: 3,
                seed: i64::from(i32::MAX),
            })
            .expect_err("seed plus k overflows");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "cluster_seed_range"
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
