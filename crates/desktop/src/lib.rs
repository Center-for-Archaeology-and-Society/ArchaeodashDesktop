//! Tauri command surface (desktop adapter) delegating to
//! `archaeodash-application`. The `#[tauri::command]` wrappers live in the
//! `apps/desktop/src-tauri` shell; this crate keeps the command payloads and
//! invocation logic testable without a webview runtime.

use archaeodash_application::{
    ExploreService, GroupService, ImportService, SourceFileService, TransformService,
};
use archaeodash_contracts::{
    AppInfo, AppliedTransformation, ApplyTransformationRequest, BatchRatioRequest,
    DeleteGroupRequest, DuplicateGroupRequest, ExploreCompositionalProfileRequest,
    ExploreCompositionalProfileResponse, ExploreCrosstabRequest, ExploreCrosstabResponse,
    ExploreHistogramRequest, ExploreHistogramResponse, ExploreMissingProfileRequest,
    ExploreMissingProfileResponse, FileDownload, FileUploadRequest, GroupCandidate, GroupSummary,
    ImportCommitRequest, ImportCommitResponse, ImportPreviewRequest, ImportPreviewResponse,
    MergeGroupsRequest, PatchDescriptiveValuesRequest, RatioSpecDto, SaveTransformationResponse,
    StagedFile, TransactionResponse, TransferUnitsRequest, TransformationDefinition,
    TransformationListResponse,
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

/// Executes the desktop smoke use case: same application call as the HTTP
/// adapter, proving the two adapters share one core.
pub fn app_info() -> DesktopAppInfo {
    archaeodash_application::app_info("tauri", true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_contracts::{ImportCommitRequest, ImportPreviewRequest};

    #[test]
    fn desktop_smoke_reports_tauri_transport() {
        assert_eq!(app_info().transport, "tauri");
        assert!(app_info().ready);
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
}
