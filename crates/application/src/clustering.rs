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
    calc_e_distance, dense_euclidean, diana, get_eligible, group_mem_probs_tracked, hclust_ward_d2,
    kmeans, pam, silhouette_widths, EuclideanMatch, MembershipMethod,
};
use archaeodash_contracts::{
    ClusterDiagnosticsRequest, ClusterDiagnosticsResponse, ClusterFitRequest, ClusterFitResponse,
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
        read_group_file(&self.group_path(rel)?).map_err(import_err)
    }

    fn validate_dimensions(
        n: usize,
        p: usize,
        context: &str,
        needs_distances: bool,
    ) -> Result<(), DomainError> {
        let cells = n.checked_mul(p).unwrap_or(usize::MAX);
        if cells > MAX_INPUT_CELLS {
            return Err(validation(
                "cluster_resource_limit",
                format!("{context}: input has {cells} cells; limit is {MAX_INPUT_CELLS}"),
            ));
        }
        if needs_distances && n.checked_mul(n).unwrap_or(usize::MAX) > MAX_DISTANCE_CELLS {
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
                .map(|r| {
                    r.legacy_rowid
                        .clone()
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| r.uuid.to_string())
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
        if req.columns.is_empty() {
            return Err(validation(
                "cluster_empty",
                "clustering requires at least one column",
            ));
        }
        let data = self.read_group(&req.path)?;
        Self::validate_dimensions(data.rows.len(), req.columns.len(), &req.path, true)?;
        let matrix =
            Self::input_matrix(&data, &req.columns, req.transformation.as_ref(), &req.path)?;
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
        let dist = dense_euclidean(&rows, n, rows.first().map_or(0, Vec::len));
        let mut wss = Vec::with_capacity(max_k);
        let mut silhouette = Vec::with_capacity(max_k.saturating_sub(1));
        for k in 1..=max_k {
            let fit = kmeans(&matrix, k, 100, 25, seed.saturating_add(k as i32))?;
            wss.push(fit.tot_withinss);
            if k >= 2 {
                let mean = diagnostics_mean(&dist, &fit.cluster, k);
                silhouette.push(if mean.is_nan() { None } else { Some(mean) });
            }
        }
        Ok(ClusterDiagnosticsResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            column_names: req.columns.clone(),
            n_rows: n as u64,
            wss,
            silhouette,
        })
    }

    /// One clustering fit (`kmeans`, `pam`, `hclust ward.D2`, or `diana`)
    /// over one group file, with per-row silhouette widths for the
    /// partitioning families.
    pub fn cluster_fit(&self, req: &ClusterFitRequest) -> Result<ClusterFitResponse, DomainError> {
        if req.columns.is_empty() {
            return Err(validation(
                "cluster_empty",
                "clustering requires at least one column",
            ));
        }
        let data = self.read_group(&req.path)?;
        Self::validate_dimensions(data.rows.len(), req.columns.len(), &req.path, true)?;
        let matrix =
            Self::input_matrix(&data, &req.columns, req.transformation.as_ref(), &req.path)?;
        Self::require_complete(&matrix, &req.path)?;
        let n = matrix.n_rows();
        Self::validate_dimensions(n, matrix.cols.len(), &req.path, true)?;

        let mut response = ClusterFitResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            method: req.method,
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
                let k = Self::required_k(req.k, "kmeans")?;
                let seed = checked_seed(req.seed.unwrap_or(DEFAULT_KMEANS_SEED), &req.path)?;
                if req.iter_max == 0 || req.iter_max > 1_000 || req.nstart == 0 || req.nstart > 100
                {
                    return Err(validation(
                        "cluster_kmeans_parameters",
                        "iter_max must be 1..=1000 and nstart must be 1..=100",
                    ));
                }
                let fit = kmeans(&matrix, k, req.iter_max as usize, req.nstart as usize, seed)?;
                response.cluster = Some(fit.cluster.iter().map(|&c| c as u32).collect());
                response.size = Some(fit.size.iter().map(|&s| s as u32).collect());
                response.tot_withinss = Some(fit.tot_withinss);
                response.centers = Some(fit.centers);
                response.silhouette = Some(Self::row_silhouettes(&matrix, &fit.cluster, k));
            }
            ClusterMethod::Pam => {
                let k = Self::required_k(req.k, "pam")?;
                let fit = pam(&matrix, k)?;
                response.cluster = Some(fit.clustering.iter().map(|&c| c as u32).collect());
                response.medoids = Some(fit.medoids.iter().map(|&m| m as u32).collect());
                response.silhouette = Some(Self::row_silhouettes(&matrix, &fit.clustering, k));
            }
            ClusterMethod::HclustWardD2 => {
                let fit = hclust_ward_d2(&matrix)?;
                response.merge = Some(fit.merge);
                response.height = Some(fit.height);
                response.order = Some(fit.order.iter().map(|&o| o as u32).collect());
            }
            ClusterMethod::Diana => {
                let fit = diana(&matrix)?;
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
    ) -> Vec<Option<f64>> {
        let n = matrix.n_rows();
        let rows = Self::rows(matrix);
        let dist = dense_euclidean(&rows, n, matrix.cols.len());
        silhouette_widths(&dist, clustering, k)
            .into_iter()
            .map(|w| if w.is_nan() { None } else { Some(w) })
            .collect()
    }

    /// `group.mem.probs` over one group file: eligibility via `getEligible`
    /// (`n > max(n_features, n_groups) + 1`), per-row probability cells in
    /// eligible-group order, best group, and the effective method (the
    /// legacy Hotellings-to-Mahalanobis `tryCatch` fallback).
    pub fn membership_probabilities(
        &self,
        req: &MembershipProbabilitiesRequest,
    ) -> Result<MembershipProbabilitiesResponse, DomainError> {
        let data = self.read_group(&req.path)?;
        Self::validate_dimensions(data.rows.len(), req.columns.len(), &req.path, false)?;
        let matrix = Self::input_matrix(&data, &req.columns, None, &req.path)?;
        let ids = Self::id_values(&data, &req.id_column, &req.path)?;
        let groups = Self::group_labels(&data, &req.group_column, &req.path)?;
        let eligible = get_eligible(&groups, matrix.cols.len());
        if data
            .rows
            .len()
            .checked_mul(eligible.len())
            .unwrap_or(usize::MAX)
            > MAX_DISTANCE_CELLS
        {
            return Err(validation(
                "cluster_resource_limit",
                format!("{}: membership result exceeds the cell limit", req.path),
            ));
        }
        let method = match req.method {
            MembershipMethodDto::Hotellings => MembershipMethod::Hotellings,
            MembershipMethodDto::Mahalanobis => MembershipMethod::Mahalanobis,
        };
        let (table, effective) = group_mem_probs_tracked(
            &ids,
            &groups,
            &req.group_column,
            &matrix,
            &req.columns,
            &eligible,
            method,
        )?;
        let non_finite_null = |v: f64| if v.is_finite() { Some(v) } else { None };
        Ok(MembershipProbabilitiesResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            effective_method: match effective {
                MembershipMethod::Hotellings => MembershipMethodDto::Hotellings,
                MembershipMethod::Mahalanobis => MembershipMethodDto::Mahalanobis,
            },
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
        if !(1..=100).contains(&req.limit) {
            return Err(validation(
                "euclidean_limit_range",
                "limit must lie between 1 and 100",
            ));
        }
        let data = self.read_group(&req.path)?;
        Self::validate_dimensions(data.rows.len(), req.columns.len(), &req.path, true)?;
        let matrix = Self::input_matrix(&data, &req.columns, None, &req.path)?;
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
        let mut projection: Vec<String> =
            groups.iter().filter(|g| !g.is_empty()).cloned().collect();
        projection.sort();
        projection.dedup();
        let matches: Vec<EuclideanMatch> = calc_e_distance(
            &internal_rowids,
            &ids,
            &groups,
            &matrix,
            &projection,
            req.limit as usize,
            req.within_group,
        )?;
        Ok(EuclideanMatchesResponse {
            path: req.path.clone(),
            revision_id: data.profile.revision_id.clone(),
            rows: matches
                .into_iter()
                .map(|m: EuclideanMatch| EuclideanMatchDto {
                    rowid: visible_rowids.get(&m.rowid).cloned().unwrap_or(m.rowid),
                    id: m.id,
                    match_id: m.match_id,
                    distance: m.distance,
                    group: m.group,
                    match_group: m.match_group,
                })
                .collect(),
        })
    }
}

/// Mean silhouette width (the diagnostics-series aggregate over
/// [`silhouette_widths`], NaN preserved for trivial `k`).
fn diagnostics_mean(dist: &[Vec<f64>], clustering: &[i32], k: usize) -> f64 {
    let widths = silhouette_widths(dist, clustering, k);
    let total: f64 = widths
        .iter()
        .map(|w| if w.is_nan() { 0.0 } else { *w })
        .sum();
    total / clustering.len() as f64
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

        // max_k above the n - 1 and 20 ceilings clamps instead of failing.
        let clamped = service
            .cluster_diagnostics(&ClusterDiagnosticsRequest {
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
                path,
                columns: vec!["cu".into()],
                transformation: None,
                method: ClusterMethod::Kmeans,
                k: Some(2),
                iter_max: 100,
                nstart: 25,
                seed: None,
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
                    path: request_path.into(),
                    columns: vec!["as".into()],
                    transformation: None,
                    method: ClusterMethod::Kmeans,
                    k: Some(2),
                    iter_max: 100,
                    nstart: 25,
                    seed: None,
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
            path: path.clone(),
            columns: vec!["as".into(), "fe".into()],
            transformation: None,
            method,
            k,
            iter_max: 100,
            nstart: 25,
            seed: Some(20260914),
        };
        let kmeans_fit = service
            .cluster_fit(&request(ClusterMethod::Kmeans, Some(3)))
            .expect("kmeans");
        assert_eq!(kmeans_fit.n_rows, 30);
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
    fn membership_falls_back_to_mahalanobis_and_reports_effective_method() {
        let (service, _dir, path) = membership_fixture();
        let response = service
            .membership_probabilities(&MembershipProbabilitiesRequest {
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
        write_group_rows(&file, data.profile, &data.rows).expect("write duplicated rowids");

        let response = service
            .euclidean_matches(&EuclideanMatchesRequest {
                path,
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                limit: 100,
                within_group: true,
            })
            .expect("matches");
        assert!(response.rows.iter().any(|row| {
            ((row.id == "X0" && row.match_id == "X1")
                || (row.id == "X1" && row.match_id == "X0"))
                && row.rowid == "duplicate"
        }), "distinct UUID-backed observations remain match candidates despite duplicate legacy keys");
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
                path: path.clone(),
                columns: vec!["as".into()],
                transformation: None,
                method: ClusterMethod::Kmeans,
                k: Some(2),
                iter_max: 100,
                nstart: 25,
                seed: Some(i64::from(i32::MAX) + 1),
            })
            .expect_err("out of range seed");
        assert!(matches!(
            err,
            DomainError::Validation { ref code, .. } if code == "cluster_seed_range"
        ));
        let err = service
            .cluster_diagnostics(&ClusterDiagnosticsRequest {
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
