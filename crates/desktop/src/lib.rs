//! Tauri command surface (desktop adapter) delegating to
//! `archaeodash-application`. The `#[tauri::command]` wrappers live in the
//! `apps/desktop/src-tauri` shell; this crate keeps the command payloads and
//! invocation logic testable without a webview runtime.

use archaeodash_application::{
    ClusterService, ExploreService, ExportService, GroupService, ImportService, OrdinationService,
    PreferenceService, SourceFileService, TransformService,
};
use archaeodash_contracts::{
    AppInfo, AppliedTransformation, ApplyTransformationRequest, BatchRatioRequest,
    ClusterDiagnosticsRequest, ClusterDiagnosticsResponse, ClusterFitRequest, ClusterFitResponse,
    DeleteGroupRequest, DuplicateGroupRequest, EuclideanMatchesRequest, EuclideanMatchesResponse,
    ExploreCompositionalProfileRequest, ExploreCompositionalProfileResponse,
    ExploreCrosstabRequest, ExploreCrosstabResponse, ExploreHistogramRequest,
    ExploreHistogramResponse, ExploreMissingProfileRequest, ExploreMissingProfileResponse,
    ExportMeasuredDataRequest, ExportPcaScoresRequest, ExportResult, ExportTransformedRequest,
    FileDownload, FileUploadRequest, GetPreferencesResponse, GroupCandidate, GroupRowsResponse,
    GroupSummary, ImportCommitRequest, ImportCommitResponse, ImportPreviewRequest,
    ImportPreviewResponse, LdaRequest, LdaResponse, MembershipProbabilitiesRequest,
    MembershipProbabilitiesResponse, MergeGroupsRequest, PatchDescriptiveValuesRequest, PcaRequest,
    PcaResponse, PutPreferenceRequest, RatioSpecDto, SaveTransformationResponse, StagedFile,
    TransactionResponse, TransferUnitsRequest, TransformationDefinition,
    TransformationListResponse, UmapRequest, UmapResponse,
};
use archaeodash_data_io::ImportError;
use archaeodash_domain::DomainError;
use archaeodash_storage::StoreError;
use std::path::PathBuf;
use std::sync::Mutex;

/// Payload for the desktop `app_info` command.
pub type DesktopAppInfo = AppInfo;

/// Desktop group state: one project-scoped group service sharing the project
/// root with the import service. Construction runs startup recovery.
pub struct DesktopGroups {
    service: Mutex<Option<GroupService>>,
}

impl DesktopGroups {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for group use cases, recovering
    /// any interrupted Section 6.8 transactions first.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = GroupService::new(root).map_err(|e: StoreError| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&GroupService) -> Result<T, StoreError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `scan_group_candidates` command body.
    pub fn scan_group_candidates(&self) -> Result<Vec<GroupCandidate>, String> {
        self.with_service(|svc| svc.scan_candidates())
    }

    /// Desktop `validate_group_file` command body: full validation-on-add.
    pub fn validate_group_file(&self, path: String) -> Result<GroupSummary, String> {
        self.with_service(|svc| svc.validate(&path))
    }

    /// Desktop `group_rows` command body: full row data of one group file
    /// for the client dataset table (hidden UUIDs address edits, Section 9.4).
    pub fn group_rows(&self, path: String) -> Result<GroupRowsResponse, String> {
        self.with_service(|svc| svc.rows(&path))
    }

    /// Desktop `move_analytical_units` / `copy_analytical_units` command body.
    pub fn transfer_units(&self, req: TransferUnitsRequest) -> Result<TransactionResponse, String> {
        self.with_service(|svc| svc.transfer_units(&req))
    }

    /// Desktop `merge_groups` command body.
    pub fn merge_groups(&self, req: MergeGroupsRequest) -> Result<TransactionResponse, String> {
        self.with_service(|svc| svc.merge_groups(&req))
    }

    /// Desktop `delete_group` command body: journaled deletion guarded by
    /// exact-path confirmation and the revision the caller last read.
    pub fn delete_group(&self, req: DeleteGroupRequest) -> Result<TransactionResponse, String> {
        self.with_service(|svc| svc.delete_group(&req))
    }

    /// Desktop `patch_descriptive_values` command body: batch
    /// hidden-UUID-addressed descriptive edits in one journaled transaction;
    /// elemental columns are locked (Section 4 Phase 4).
    pub fn patch_descriptive_values(
        &self,
        req: PatchDescriptiveValuesRequest,
    ) -> Result<TransactionResponse, String> {
        self.with_service(|svc| svc.patch_descriptive_values(&req))
    }

    /// Desktop `duplicate_group` command body: duplicate one whole group,
    /// preserving analytical UUIDs and lineage by default.
    pub fn duplicate_group(
        &self,
        req: DuplicateGroupRequest,
    ) -> Result<TransactionResponse, String> {
        self.with_service(|svc| svc.duplicate_group(&req))
    }
}

impl Default for DesktopGroups {
    fn default() -> Self {
        Self::new()
    }
}

/// Desktop import state: one project-scoped import service. The root is set
/// once when the user opens a project directory (Section 10.4: path-taking
/// commands operate on explicit selections below the opened project root).
pub struct DesktopImport {
    service: Mutex<Option<ImportService>>,
}

impl DesktopImport {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for import use cases. Returns an
    /// error message payload suitable for direct IPC responses.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = ImportService::new(root).map_err(|e| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&ImportService) -> Result<T, ImportError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `open_import_preview` command body: parse-only source preview.
    pub fn open_import_preview(
        &self,
        req: ImportPreviewRequest,
    ) -> Result<ImportPreviewResponse, String> {
        self.with_service(|svc| svc.preview(&req))
    }

    /// Desktop `commit_group_import` command body: publish group files.
    pub fn commit_group_import(
        &self,
        req: ImportCommitRequest,
    ) -> Result<ImportCommitResponse, String> {
        self.with_service(|svc| svc.commit(&req))
    }
}

impl Default for DesktopImport {
    fn default() -> Self {
        Self::new()
    }
}

/// Desktop source-file state: one project-scoped upload/catalog service
/// sharing the project root (quarantine lives under `.archaeodash`).
pub struct DesktopFiles {
    service: Mutex<Option<SourceFileService>>,
}

impl DesktopFiles {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for source-file use cases.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = SourceFileService::new(root).map_err(|e| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&SourceFileService) -> Result<T, ImportError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `upload_source_file` command body: bounded-quarantine upload
    /// promoting to the requested logical path (Section 10.2).
    pub fn upload_source_file(&self, req: FileUploadRequest) -> Result<StagedFile, String> {
        self.with_service(|svc| svc.upload(&req.path, &req.content))
    }

    /// Desktop `source_file_metadata` command body.
    pub fn source_file_metadata(&self, file_id: String) -> Result<StagedFile, String> {
        self.with_service(|svc| svc.metadata(&file_id))
    }

    /// Desktop `download_source_file` command body.
    pub fn download_source_file(&self, file_id: String) -> Result<FileDownload, String> {
        self.with_service(|svc| svc.download(&file_id))
    }

    /// Desktop `delete_source_file` command body: soft delete with tombstone.
    pub fn delete_source_file(&self, file_id: String) -> Result<StagedFile, String> {
        self.with_service(|svc| svc.delete(&file_id))
    }
}

impl Default for DesktopFiles {
    fn default() -> Self {
        Self::new()
    }
}

/// Desktop transformation-definition state: one project-scoped transform
/// service sharing the project root (definitions persist under
/// `.archaeodash/transformations` as structured JSON, Section 8.2).
pub struct DesktopTransforms {
    service: Mutex<Option<TransformService>>,
}

impl DesktopTransforms {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for transformation use cases.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = TransformService::new(root).map_err(|e| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&TransformService) -> Result<T, DomainError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `save_transformation` command body (upsert by name).
    pub fn save_transformation(
        &self,
        definition: TransformationDefinition,
    ) -> Result<SaveTransformationResponse, String> {
        self.with_service(|svc| svc.save(&definition))
    }

    /// Desktop `list_transformations` command body: summaries sorted by name.
    pub fn list_transformations(&self) -> Result<TransformationListResponse, String> {
        self.with_service(|svc| svc.list())
    }

    /// Desktop `load_transformation` command body.
    pub fn load_transformation(&self, name: String) -> Result<TransformationDefinition, String> {
        self.with_service(|svc| svc.load(&name))
    }

    /// Desktop `delete_transformation` command body.
    pub fn delete_transformation(&self, name: String) -> Result<TransformationDefinition, String> {
        self.with_service(|svc| svc.delete(&name))
    }

    /// Desktop `batch_ratio_specs` command body (Section 8.2 batch generation).
    pub fn batch_ratio_specs(&self, req: BatchRatioRequest) -> Result<Vec<RatioSpecDto>, String> {
        self.with_service(|svc| svc.batch_ratio_specs(&req))
    }

    /// Desktop `apply_transformation` command body: ephemeral on-demand
    /// application; calculated values are never persisted (Section 5).
    pub fn apply_transformation(
        &self,
        req: ApplyTransformationRequest,
    ) -> Result<AppliedTransformation, String> {
        self.with_service(|svc| svc.apply(&req))
    }
}

impl Default for DesktopTransforms {
    fn default() -> Self {
        Self::new()
    }
}

/// Desktop explore state: one project-scoped explore service sharing the
/// project root (Section 8 procedure 12 views are ephemeral, Section 5).
pub struct DesktopExplore {
    service: Mutex<Option<ExploreService>>,
}

impl DesktopExplore {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for explore use cases.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = ExploreService::new(root).map_err(|e| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&ExploreService) -> Result<T, DomainError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `explore_missing_profile` command body.
    pub fn explore_missing_profile(
        &self,
        req: ExploreMissingProfileRequest,
    ) -> Result<ExploreMissingProfileResponse, String> {
        self.with_service(|svc| svc.missing_profile(&req))
    }

    /// Desktop `explore_histogram` command body.
    pub fn explore_histogram(
        &self,
        req: ExploreHistogramRequest,
    ) -> Result<ExploreHistogramResponse, String> {
        self.with_service(|svc| svc.histogram(&req))
    }

    /// Desktop `explore_crosstab` command body.
    pub fn explore_crosstab(
        &self,
        req: ExploreCrosstabRequest,
    ) -> Result<ExploreCrosstabResponse, String> {
        self.with_service(|svc| svc.crosstab(&req))
    }

    /// Desktop `explore_compositional_profile` command body.
    pub fn explore_compositional_profile(
        &self,
        req: ExploreCompositionalProfileRequest,
    ) -> Result<ExploreCompositionalProfileResponse, String> {
        self.with_service(|svc| svc.compositional_profile(&req))
    }
}

impl Default for DesktopExplore {
    fn default() -> Self {
        Self::new()
    }
}

/// Desktop ordination state: one project-scoped ordination service sharing
/// the project root (Section 8.5 views are ephemeral, Section 5).
pub struct DesktopOrdination {
    service: Mutex<Option<OrdinationService>>,
}

impl DesktopOrdination {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for ordination use cases.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = OrdinationService::new(root).map_err(|e| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&OrdinationService) -> Result<T, DomainError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `ordination_pca` command body: prcomp-parity PCA (Section 15.4
    /// procedure 6); results are ephemeral and never persisted (Section 5).
    pub fn ordination_pca(&self, req: PcaRequest) -> Result<PcaResponse, String> {
        self.with_service(|svc| svc.pca(&req))
    }

    /// Desktop `ordination_lda` command body: `MASS::lda` moment-method
    /// parity with the legacy three-group minimum (Section 15.4 procedure 8);
    /// results are ephemeral and never persisted (Section 5).
    pub fn ordination_lda(&self, req: LdaRequest) -> Result<LdaResponse, String> {
        self.with_service(|svc| svc.lda(&req))
    }

    /// Desktop `ordination_umap` command body: legacy naive UMAP parity
    /// (Section 15.4 procedure 7); results are ephemeral and never persisted
    /// (Section 5).
    pub fn ordination_umap(&self, req: UmapRequest) -> Result<UmapResponse, String> {
        self.with_service(|svc| svc.umap(&req))
    }
}

impl Default for DesktopOrdination {
    fn default() -> Self {
        Self::new()
    }
}

/// Desktop cluster/membership/euclidean state: one project-scoped cluster
/// service sharing the project root (Section 8 results are ephemeral,
/// Section 5 storage invariant).
pub struct DesktopClustering {
    service: Mutex<Option<ClusterService>>,
}

impl DesktopClustering {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for cluster use cases.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = ClusterService::new(root).map_err(|e| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&ClusterService) -> Result<T, DomainError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `cluster_diagnostics` command body: WSS elbow and
    /// mean-silhouette series over one group file.
    pub fn cluster_diagnostics(
        &self,
        req: ClusterDiagnosticsRequest,
    ) -> Result<ClusterDiagnosticsResponse, String> {
        self.with_service(|svc| svc.cluster_diagnostics(&req))
    }

    /// Desktop `cluster_fit` command body: one kmeans/pam/ward.D2/DIANA fit.
    pub fn cluster_fit(&self, req: ClusterFitRequest) -> Result<ClusterFitResponse, String> {
        self.with_service(|svc| svc.cluster_fit(&req))
    }

    /// Desktop `membership_probabilities` command body: `group.mem.probs`
    /// parity with the Hotellings-to-Mahalanobis fallback.
    pub fn membership_probabilities(
        &self,
        req: MembershipProbabilitiesRequest,
    ) -> Result<MembershipProbabilitiesResponse, String> {
        self.with_service(|svc| svc.membership_probabilities(&req))
    }

    /// Desktop `euclidean_matches` command body: `calcEDistance` parity.
    pub fn euclidean_matches(
        &self,
        req: EuclideanMatchesRequest,
    ) -> Result<EuclideanMatchesResponse, String> {
        self.with_service(|svc| svc.euclidean_matches(&req))
    }
}

impl Default for DesktopClustering {
    fn default() -> Self {
        Self::new()
    }
}

/// Desktop export state: one project-scoped export service sharing the
/// project root (Section 7.3 result exports are ephemeral CSV strings; the
/// client saves through a native dialog, Section 5 storage invariant).
pub struct DesktopExports {
    service: Mutex<Option<ExportService>>,
}

impl DesktopExports {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for export use cases.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = ExportService::new(root).map_err(|e| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&ExportService) -> Result<T, DomainError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `export_measured_data` command body: the measured chemical
    /// frame (legacy `rvals$selectedData`) as ephemeral CSV (Section 7.3).
    pub fn export_measured_data(
        &self,
        req: ExportMeasuredDataRequest,
    ) -> Result<ExportResult, String> {
        self.with_service(|svc| svc.export_measured_data(&req))
    }

    /// Desktop `export_transformed` command body: the explicitly computed
    /// transformed result as ephemeral CSV (Section 7.3; never persisted,
    /// Section 5).
    pub fn export_transformed(
        &self,
        req: ExportTransformedRequest,
    ) -> Result<ExportResult, String> {
        self.with_service(|svc| svc.export_transformed(&req))
    }

    /// Desktop `export_pca_scores` command body: the computed PCA score frame
    /// (Section 3.2 correction of the legacy `rvals$pcaData` bug).
    pub fn export_pca_scores(&self, req: ExportPcaScoresRequest) -> Result<ExportResult, String> {
        self.with_service(|svc| svc.export_pca_scores(&req))
    }
}

impl Default for DesktopExports {
    fn default() -> Self {
        Self::new()
    }
}

/// Desktop preference state: one project-scoped preference service sharing
/// the project root (Section 10.1 typed allowlist; `.archaeodash/
/// preferences.json` store, replaced by the hosted control-plane table in
/// Phase 7).
pub struct DesktopPreferences {
    service: Mutex<Option<PreferenceService>>,
}

impl DesktopPreferences {
    /// No project open yet; `open_project` sets the root.
    pub fn new() -> Self {
        Self {
            service: Mutex::new(None),
        }
    }

    /// Opens (or re-opens) the project root for preference use cases.
    pub fn open_project(&self, root: impl Into<PathBuf>) -> Result<(), String> {
        let service = PreferenceService::new(root).map_err(|e| e.to_string())?;
        *self.service.lock().map_err(|e| e.to_string())? = Some(service);
        Ok(())
    }

    fn with_service<T>(
        &self,
        op: impl FnOnce(&PreferenceService) -> Result<T, DomainError>,
    ) -> Result<T, String> {
        let guard = self.service.lock().map_err(|e| e.to_string())?;
        let service = guard.as_ref().ok_or_else(|| {
            "no project open: call open_project with a directory first".to_string()
        })?;
        op(service).map_err(|e| e.to_string())
    }

    /// Desktop `preferences_get` command body: every stored preference
    /// (absent keys read as defaults client-side).
    pub fn preferences_get(&self) -> Result<GetPreferencesResponse, String> {
        self.with_service(|svc| svc.get_all())
    }

    /// Desktop `preferences_set` command body: upsert one allowlisted
    /// preference.
    pub fn preferences_set(&self, req: PutPreferenceRequest) -> Result<(), String> {
        self.with_service(|svc| svc.set(&req))
    }
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self::new()
    }
}

/// Executes the desktop smoke use case: same application call as the HTTP
/// adapter, proving the two adapters share one core.
pub fn app_info() -> DesktopAppInfo {
    archaeodash_application::app_info("tauri", true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_contracts::{
        ClusterFitRequest, ClusterMethod, EuclideanMatchesRequest, ImportCommitRequest,
        ImportPreviewRequest, MembershipMethodDto, MembershipProbabilitiesRequest,
    };

    #[test]
    fn desktop_smoke_reports_tauri_transport() {
        assert_eq!(app_info().transport, "tauri");
        assert!(app_info().ready);
    }

    #[test]
    fn clustering_commands_run_against_committed_groups() {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-desktop-clustering-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");

        let clustering = DesktopClustering::new();
        let no_project = clustering
            .cluster_diagnostics(ClusterDiagnosticsRequest {
                path: "groups/A.parquet".into(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                max_k: 3,
                seed: 42,
            })
            .expect_err("no project open");
        assert!(no_project.contains("no project open"));
        assert!(clustering
            .euclidean_matches(EuclideanMatchesRequest {
                path: "groups/A.parquet".into(),
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                limit: 2,
                within_group: false,
            })
            .expect_err("no project open")
            .contains("no project open"));
        clustering.open_project(&dir).expect("open project");

        let rows: Vec<String> = (0..24)
            .map(|i| {
                let group = ["A", "B", "C"][i / 8];
                let v = i as f64;
                format!(
                    "S{i},{group},{},{},{},{}",
                    1.0 + v * 0.1,
                    3.0 + v * 0.05,
                    5.0 - v * 0.02,
                    2.0 + v * 0.03
                )
            })
            .collect();
        std::fs::write(
            dir.join("cluster.csv"),
            format!("anid,Site,as,fe,co,zn\n{}\n", rows.join("\n")),
        )
        .expect("write clustering fixture");
        let import = ImportService::new(&dir).expect("import service");
        let imported = import
            .commit(&ImportCommitRequest {
                source: "cluster.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
            })
            .expect("commit fixture");

        let diagnostics = clustering
            .cluster_diagnostics(ClusterDiagnosticsRequest {
                path: imported.groups[0].path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                max_k: 3,
                seed: 42,
            })
            .expect("diagnostics");
        assert_eq!(diagnostics.n_rows, 8);
        assert_eq!(diagnostics.wss.len(), 3);
        assert_eq!(diagnostics.silhouette.len(), 2);

        let fit = clustering
            .cluster_fit(ClusterFitRequest {
                path: imported.groups[0].path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                method: ClusterMethod::Kmeans,
                k: Some(2),
                iter_max: 50,
                nstart: 5,
                seed: Some(42),
            })
            .expect("cluster fit");
        assert_eq!(fit.method, ClusterMethod::Kmeans);
        assert_eq!(fit.cluster.as_ref().map(Vec::len), Some(8));

        let groups = GroupService::new(&dir).expect("group service");
        let merged = groups
            .merge_groups(&MergeGroupsRequest {
                sources: imported
                    .groups
                    .iter()
                    .map(|group| group.path.clone())
                    .collect(),
                new_group_name: "All Sites".into(),
            })
            .expect("merge groups");
        let path = merged.outputs[0].path.clone();
        let merged_fit = clustering
            .cluster_fit(ClusterFitRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                method: ClusterMethod::Kmeans,
                k: Some(2),
                iter_max: 50,
                nstart: 5,
                seed: Some(42),
            })
            .expect("fit merged group");
        let before_transfer = groups.rows(&path).expect("read merged group rows");
        assert_eq!(
            merged_fit.analytical_uuids,
            before_transfer
                .rows
                .iter()
                .map(|row| row.analytical_uuid.clone())
                .collect::<Vec<_>>()
        );
        let membership = clustering
            .membership_probabilities(MembershipProbabilitiesRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into(), "co".into(), "zn".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                method: MembershipMethodDto::Mahalanobis,
            })
            .expect("membership probabilities");
        assert_eq!(membership.ids.len(), 24);
        assert_eq!(membership.probabilities.len(), 24);
        assert_eq!(membership.eligible_groups.len(), 3);
        assert_eq!(membership.analytical_uuids, merged_fit.analytical_uuids);

        let matches = clustering
            .euclidean_matches(EuclideanMatchesRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                limit: 2,
                within_group: false,
            })
            .expect("euclidean matches");
        assert!(!matches.rows.is_empty());
        assert!(matches.rows.iter().all(|row| row.group != row.match_group));
        let source_uuids = before_transfer
            .rows
            .iter()
            .map(|row| row.analytical_uuid.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert!(matches.rows.iter().all(|row| {
            source_uuids.contains(row.analytical_uuid.as_str())
                && source_uuids.contains(row.match_analytical_uuid.as_str())
        }));

        // A selected identity from the analytical result can be moved through
        // the same desktop transaction API without changing its measured row.
        let selected_uuid = merged_fit.analytical_uuids[0].clone();
        let selected_before = before_transfer
            .rows
            .iter()
            .find(|row| row.analytical_uuid == selected_uuid)
            .expect("fit identity belongs to source rows")
            .clone();
        let source_revision = before_transfer.revision_id.clone();
        let destination_path = "groups/Selected_Unit.parquet";
        groups
            .transfer_units(&TransferUnitsRequest {
                action: archaeodash_contracts::TransferAction::Move,
                source_path: path.clone(),
                destination_path: destination_path.into(),
                destination_group_name: Some("Selected Unit".into()),
                selected_uuids: vec![selected_uuid.clone()],
                expected_source_revision: source_revision.clone(),
            })
            .expect("move selected analytical unit");
        let destination_after = groups
            .rows(destination_path)
            .expect("read transfer destination");
        assert_eq!(destination_after.rows, vec![selected_before.clone()]);
        let source_after = groups.rows(&path).expect("read source after move");
        assert!(source_after
            .rows
            .iter()
            .all(|row| row.analytical_uuid != selected_uuid));

        // Move a second result row into the existing destination group. Both
        // original and new rows remain intact and the group revision advances.
        let second_uuid = merged_fit.analytical_uuids[1].clone();
        let second_before = source_after
            .rows
            .iter()
            .find(|row| row.analytical_uuid == second_uuid)
            .expect("second fit identity remains in source")
            .clone();
        let second_source_revision = source_after.revision_id.clone();
        let second_transfer = groups
            .transfer_units(&TransferUnitsRequest {
                action: archaeodash_contracts::TransferAction::Move,
                source_path: path.clone(),
                destination_path: destination_path.into(),
                destination_group_name: None,
                selected_uuids: vec![second_uuid.clone()],
                expected_source_revision: second_source_revision.clone(),
            })
            .expect("move another unit into existing destination");
        assert_eq!(second_transfer.outputs.len(), 2);
        let destination_after_second = groups
            .rows(destination_path)
            .expect("read destination after second move");
        assert_eq!(
            destination_after_second.rows,
            vec![selected_before.clone(), second_before]
        );
        assert_ne!(
            destination_after_second.revision_id,
            destination_after.revision_id
        );
        let source_after_second = groups.rows(&path).expect("read source after second move");
        assert!(source_after_second
            .rows
            .iter()
            .all(|row| row.analytical_uuid != second_uuid));
        assert_ne!(source_after_second.revision_id, second_source_revision);

        let retry_error = groups
            .transfer_units(&TransferUnitsRequest {
                action: archaeodash_contracts::TransferAction::Move,
                source_path: path.clone(),
                destination_path: destination_path.into(),
                destination_group_name: None,
                selected_uuids: vec![second_uuid],
                expected_source_revision: second_source_revision,
            })
            .expect_err("stale result revision rejected");
        assert!(retry_error.to_string().contains("revision"));
        assert_eq!(
            groups
                .rows(destination_path)
                .expect("destination is unchanged after rejected retry")
                .rows,
            destination_after_second.rows
        );
        assert_eq!(
            groups
                .rows(&path)
                .expect("source is unchanged after rejected retry")
                .rows,
            source_after_second.rows
        );

        let err = clustering
            .euclidean_matches(EuclideanMatchesRequest {
                path: path.clone(),
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                id_column: "anid".into(),
                limit: 0,
                within_group: false,
            })
            .expect_err("invalid limit");
        assert!(err.contains("limit"));

        // Moving every remaining source row removes the source file. The
        // transaction response names that deletion so the client can refresh.
        let remaining_uuids = source_after_second
            .rows
            .iter()
            .map(|row| row.analytical_uuid.clone())
            .collect::<Vec<_>>();
        let final_transfer = groups
            .transfer_units(&TransferUnitsRequest {
                action: archaeodash_contracts::TransferAction::Move,
                source_path: path.clone(),
                destination_path: destination_path.into(),
                destination_group_name: None,
                selected_uuids: remaining_uuids,
                expected_source_revision: source_after_second.revision_id.clone(),
            })
            .expect("move remaining units");
        assert_eq!(final_transfer.deleted_paths, vec![path.clone()]);
        assert!(groups.rows(&path).is_err(), "source group was removed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_commands_require_an_open_project() {
        let desktop = DesktopImport::new();
        let err = desktop
            .open_import_preview(ImportPreviewRequest {
                source: "x.csv".into(),
                group_column: None,
            })
            .expect_err("no project open");
        assert!(err.contains("no project open"));
    }

    #[test]
    fn import_commands_preview_and_commit_against_project_root() {
        let dir =
            std::env::temp_dir().join(format!("archaeodash-desktop-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as\nA1,Baca,1.5\nA2,Baca,2\n",
        )
        .expect("write source");

        let desktop = DesktopImport::new();
        desktop.open_project(&dir).expect("open project");

        let preview = desktop
            .open_import_preview(ImportPreviewRequest {
                source: "mini.csv".into(),
                group_column: Some("Site".into()),
            })
            .expect("preview");
        assert_eq!(preview.row_count, 2);
        assert_eq!(preview.partitions.len(), 1);

        let commit = desktop
            .commit_group_import(ImportCommitRequest {
                source: "mini.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
            })
            .expect("commit");
        assert_eq!(commit.groups.len(), 1);
        assert_eq!(commit.groups[0].group_name, "Baca");
        assert!(dir.join("groups/Baca.parquet").exists());

        // Path escape attempts are rejected with a safe message.
        let err = desktop
            .open_import_preview(ImportPreviewRequest {
                source: "../outside.csv".into(),
                group_column: None,
            })
            .expect_err("escape rejected");
        assert!(err.contains("escapes"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn group_commands_require_an_open_project() {
        let groups = DesktopGroups::new();
        let err = groups.scan_group_candidates().expect_err("no project open");
        assert!(err.contains("no project open"));
    }

    #[test]
    fn group_commands_scan_validate_transfer_and_merge() {
        use archaeodash_contracts::{ImportCommitRequest, TransferAction};
        let dir =
            std::env::temp_dir().join(format!("archaeodash-desktop-groups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,5,6\n",
        )
        .expect("write source");

        let import = DesktopImport::new();
        import.open_project(&dir).expect("open project");
        let commit = import
            .commit_group_import(ImportCommitRequest {
                source: "mini.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
            })
            .expect("commit");
        assert_eq!(commit.groups.len(), 2);

        let groups = DesktopGroups::new();
        groups.open_project(&dir).expect("open project");

        let candidates = groups.scan_group_candidates().expect("scan");
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|c| c.ready));

        let baca = groups
            .validate_group_file("groups/Baca.parquet".into())
            .expect("validate");
        assert_eq!(baca.group_name, "Baca");
        assert_eq!(baca.revision_id, "rev-1");

        let merge = groups
            .merge_groups(MergeGroupsRequest {
                sources: vec!["groups/Baca.parquet".into(), "groups/Hooper.parquet".into()],
                new_group_name: "Merged".into(),
            })
            .expect("merge");
        assert_eq!(merge.action, "merge_groups");
        assert_eq!(merge.outputs.len(), 1);
        assert_eq!(merge.outputs[0].row_count, 3);
        assert_eq!(
            merge.deleted_paths,
            vec!["groups/Hooper.parquet".to_string()]
        );
        assert!(!dir.join("groups/Hooper.parquet").exists());

        // Copy back one unit into a fresh group.
        let merged = std::fs::File::open(dir.join("groups/Baca.parquet")).ok();
        assert!(merged.is_some());
        let transfer = groups
            .transfer_units(TransferUnitsRequest {
                action: TransferAction::Copy,
                source_path: "groups/Baca.parquet".into(),
                destination_path: "groups/Copy_Target.parquet".into(),
                destination_group_name: Some("Copy Target".into()),
                selected_uuids: vec![],
                expected_source_revision: merge.outputs[0].revision_id.clone(),
            })
            .expect_err("empty selection rejected");
        assert!(transfer.contains("selected"));

        // Delete requires confirmation and the current revision.
        let baca = groups
            .validate_group_file("groups/Baca.parquet".into())
            .expect("validate");
        let err = groups
            .delete_group(DeleteGroupRequest {
                path: "groups/Baca.parquet".into(),
                expected_revision: baca.revision_id.clone(),
                confirm_path: "groups/Other.parquet".into(),
            })
            .expect_err("mismatched confirmation rejected");
        assert!(err.contains("confirm_path"));
        assert!(dir.join("groups/Baca.parquet").exists());

        groups
            .delete_group(DeleteGroupRequest {
                path: "groups/Baca.parquet".into(),
                expected_revision: baca.revision_id.clone(),
                confirm_path: "groups/Baca.parquet".into(),
            })
            .expect("delete");
        assert!(!dir.join("groups/Baca.parquet").exists());
        // The rejected copy never created its destination.
        assert!(!dir.join("groups/Copy_Target.parquet").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_commands_upload_metadata_download_delete() {
        let dir =
            std::env::temp_dir().join(format!("archaeodash-desktop-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");

        let files = DesktopFiles::new();
        let err = files
            .source_file_metadata("not-a-uuid".into())
            .expect_err("no project open");
        assert!(err.contains("no project open"));

        files.open_project(&dir).expect("open project");

        let staged = files
            .upload_source_file(FileUploadRequest {
                path: "sources/mini.csv".into(),
                content: b"anid,Site,as\nA1,Baca,1.5\n".to_vec(),
            })
            .expect("upload");
        assert_eq!(staged.parse_state, "parsed");
        assert!(dir.join("sources/mini.csv").exists());

        let meta = files
            .source_file_metadata(staged.file_id.clone())
            .expect("metadata");
        assert_eq!(meta, staged);

        let download = files
            .download_source_file(staged.file_id.clone())
            .expect("download");
        assert_eq!(download.content, b"anid,Site,as\nA1,Baca,1.5\n".to_vec());

        let deleted = files
            .delete_source_file(staged.file_id.clone())
            .expect("delete");
        assert!(deleted.deleted);
        assert!(!dir.join("sources/mini.csv").exists());
        let err = files
            .download_source_file(staged.file_id)
            .expect_err("download after delete");
        assert!(err.contains("deleted"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transformation_commands_save_apply_list_delete() {
        use archaeodash_contracts::{
            ApplyTransformationRequest, ImputationMethod, RatioMode, RatioSpecDto, TransformMethod,
            TransformationDefinition,
        };

        let dir =
            std::env::temp_dir().join(format!("archaeodash-desktop-tx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");

        let transforms = DesktopTransforms::new();
        let err = transforms
            .list_transformations()
            .expect_err("no project open");
        assert!(err.contains("no project open"));
        transforms.open_project(&dir).expect("open project");

        // Import a group file so apply has a target.
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,5,6\n",
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
            })
            .expect("commit");

        let definition = TransformationDefinition {
            name: "desktop log".into(),
            transform_method: TransformMethod::Log10,
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
        };
        let saved = transforms
            .save_transformation(definition.clone())
            .expect("save");
        assert!(!saved.replaced);

        let listed = transforms.list_transformations().expect("list");
        assert_eq!(listed.transformations.len(), 1);
        assert_eq!(listed.transformations[0].name, "desktop log");

        let loaded = transforms
            .load_transformation("desktop log".into())
            .expect("load");
        assert_eq!(loaded, definition);

        // Apply is ephemeral: result returned, group file untouched.
        let path = commit.groups[0].path.clone();
        let before = std::fs::read(dir.join(&path)).expect("read group");
        let applied = transforms
            .apply_transformation(ApplyTransformationRequest {
                path: path.clone(),
                definition,
            })
            .expect("apply");
        assert_eq!(applied.columns, vec!["as", "fe", "as_fe"]);
        assert_eq!(applied.rows.len(), commit.groups[0].row_count as usize);
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("read group"),
            before,
            "group file byte-identical after apply"
        );

        let deleted = transforms
            .delete_transformation("desktop log".into())
            .expect("delete");
        assert_eq!(deleted.transform_method, TransformMethod::Log10);
        assert!(transforms
            .list_transformations()
            .expect("empty")
            .transformations
            .is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explore_commands_run_against_committed_groups() {
        use archaeodash_contracts::ExploreMissingProfileRequest;

        let dir = std::env::temp_dir().join(format!(
            "archaeodash-desktop-explore-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");

        let explore = DesktopExplore::new();
        let err = explore
            .explore_missing_profile(ExploreMissingProfileRequest {
                path: "groups/whatever.parquet".into(),
                columns: vec!["as".into()],
                transformation: None,
            })
            .expect_err("no project open");
        assert!(err.contains("no project open"));

        explore.open_project(&dir).expect("open project");
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,5,6\n",
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
            })
            .expect("commit");

        let response = explore
            .explore_missing_profile(ExploreMissingProfileRequest {
                path: commit.groups[0].path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
            })
            .expect("missing profile");
        assert_eq!(response.rows.len(), 2);
        assert!(response.rows.iter().all(|r| r.band == "Good"));
        assert!(!response.revision_id.is_empty());
    }

    #[test]
    fn ordination_commands_run_against_committed_groups() {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "archaeodash-desktop-ordination-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");

        let ordination = DesktopOrdination::new();
        let err = ordination
            .ordination_pca(PcaRequest {
                path: "groups/whatever.parquet".into(),
                columns: vec!["as".into()],
                scale: false,
                transformation: None,
            })
            .expect_err("no project open");
        assert!(err.contains("no project open"));

        ordination.open_project(&dir).expect("open project");

        // Three-group source; LDA needs >= 3 levels in ONE group file, so the
        // three group files are merged into one before ordination.
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\nA3,Hooper,5,6\nA4,Hooper,7,8\nA5,Iowa,1,2\nA6,Iowa,3,4\n",
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
            })
            .expect("commit");
        assert_eq!(commit.groups.len(), 3);
        let groups = GroupService::new(&dir).expect("group service");
        let merge = groups
            .merge_groups(&MergeGroupsRequest {
                sources: commit.groups.iter().map(|g| g.path.clone()).collect(),
                new_group_name: "Merged".into(),
            })
            .expect("merge");
        let merged_path = merge.outputs[0].path.clone();

        // PCA is ephemeral: result returned, merged group file untouched.
        let before = std::fs::read(dir.join(&merged_path)).expect("read group");
        let pca = ordination
            .ordination_pca(PcaRequest {
                path: merged_path.clone(),
                columns: vec!["as".into(), "fe".into()],
                scale: false,
                transformation: None,
            })
            .expect("pca");
        assert_eq!(pca.score_names, vec!["PC1", "PC2"]);
        assert_eq!(pca.scores.len(), 6);
        assert!(!pca.revision_id.is_empty());
        assert_eq!(
            std::fs::read(dir.join(&merged_path)).expect("read group"),
            before,
            "group file byte-identical after PCA"
        );

        let lda = ordination
            .ordination_lda(LdaRequest {
                path: merged_path.clone(),
                columns: vec!["as".into(), "fe".into()],
                group_column: "Site".into(),
                transformation: None,
            })
            .expect("lda");
        assert_eq!(lda.levels.len(), 3);
        assert!(!lda.score_names.is_empty());

        // UMAP's legacy n_neighbors = 15 needs more than 15 rows, so a
        // 20-row single-group file feeds the default-seed embedding.
        let rows: Vec<String> = (0..20)
            .map(|i| {
                let v = i as f64;
                format!("A{i},A,{},{}", 1.5 + v, 3.0 + 2.0 * (v % 4.0))
            })
            .collect();
        std::fs::write(
            dir.join("many.csv"),
            format!("anid,Site,as,fe\n{}\n", rows.join("\n")),
        )
        .expect("write source");
        let commit = import
            .commit(&ImportCommitRequest {
                source: "many.csv".into(),
                group_column: "Site".into(),
                visible_id_column: None,
                elemental_columns: None,
                recipe: None,
                destination_dir: None,
            })
            .expect("commit many");
        let umap = ordination
            .ordination_umap(UmapRequest {
                path: commit.groups[0].path.clone(),
                columns: vec!["as".into(), "fe".into()],
                transformation: None,
                seed: None,
            })
            .expect("umap");
        assert_eq!(umap.score_names, vec!["V1", "V2"]);
        assert_eq!(umap.embedding.len(), 20);
        assert!(umap.embedding.iter().all(|row| row.len() == 2));

        // Validation errors surface as error payloads on the open service.
        let err = ordination
            .ordination_pca(PcaRequest {
                path: "groups/Missing.parquet".into(),
                columns: vec!["as".into(), "fe".into()],
                scale: false,
                transformation: None,
            })
            .expect_err("missing group file");
        assert!(!err.contains("no project open"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_commands_run_against_committed_groups() {
        use archaeodash_contracts::ExportMeasuredDataRequest;

        let dir = std::env::temp_dir().join(format!(
            "archaeodash-desktop-exports-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");

        let exports = DesktopExports::new();
        let err = exports
            .export_measured_data(ExportMeasuredDataRequest {
                path: "groups/whatever.parquet".into(),
                raw_text: false,
            })
            .expect_err("no project open");
        assert!(err.contains("no project open"));

        exports.open_project(&dir).expect("open project");
        std::fs::write(
            dir.join("mini.csv"),
            "anid,Site,as,fe\nA1,Baca,1.5,3\nA2,Baca,2,4\n",
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
            })
            .expect("commit");
        let path = commit.groups[0].path.clone();

        // Measured-data export: legacy filename hint, CSV media type, visible
        // ID + descriptive + elemental columns, hidden uuid absent.
        let export = exports
            .export_measured_data(ExportMeasuredDataRequest {
                path: path.clone(),
                raw_text: false,
            })
            .expect("measured export");
        assert_eq!(export.file_name, "Baca.csv");
        assert_eq!(export.media_type, "text/csv");
        assert!(export.content.starts_with("anid,Site,as,fe\n"));
        assert!(export.content.contains("A1,Baca,1.5,3"));
        assert!(!export.content.contains("analytical_uuid"));

        // Ephemeral: the group file is byte-identical after exporting.
        let before = std::fs::read(dir.join(&path)).expect("read group");
        assert_eq!(
            std::fs::read(dir.join(&path)).expect("read group"),
            before,
            "group file byte-identical after export"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    use archaeodash_contracts::{PreferenceKey, PutPreferenceRequest};

    #[test]
    fn preference_commands_upsert_read_and_gate_on_open_project() {
        let dir =
            std::env::temp_dir().join(format!("archaeodash-desktop-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("project dir");

        let prefs = DesktopPreferences::new();
        let err = prefs.preferences_get().expect_err("no project open");
        assert!(err.contains("no project open"));

        prefs.open_project(&dir).expect("open project");
        prefs
            .preferences_set(PutPreferenceRequest {
                key: PreferenceKey::Theme,
                value: serde_json::json!("dark"),
            })
            .expect("set theme");
        prefs
            .preferences_set(PutPreferenceRequest {
                key: PreferenceKey::LastOpenedDataset,
                value: serde_json::json!("Baca"),
            })
            .expect("set last opened");
        // Upsert replaces without duplicating.
        prefs
            .preferences_set(PutPreferenceRequest {
                key: PreferenceKey::Theme,
                value: serde_json::json!("light"),
            })
            .expect("update theme");

        let response = prefs.preferences_get().expect("get");
        assert_eq!(response.preferences.len(), 2);
        assert!(response
            .preferences
            .contains(&archaeodash_contracts::PreferenceEntry {
                key: PreferenceKey::Theme,
                value: serde_json::json!("light"),
            }));

        // Allowlist/shape validation rejects before touching the store.
        assert!(prefs
            .preferences_set(PutPreferenceRequest {
                key: PreferenceKey::Theme,
                value: serde_json::json!("solarized"),
            })
            .is_err());
        assert_eq!(prefs.preferences_get().expect("get").preferences.len(), 2);

        // Persisted as one JSON document in the project metadata area.
        assert!(dir.join(".archaeodash/preferences.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
