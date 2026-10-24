//! Tauri 2 desktop shell. Commands delegate to the `archaeodash-desktop`
//! crate (crates/desktop), which calls the shared application use cases.

use tauri::Manager;

use archaeodash_contracts::{
    ApplyTransformationRequest, BatchRatioRequest, BatchTransferUnitsRequest,
    ClusterDiagnosticsRequest, ClusterFitRequest, DeleteGroupRequest, DuplicateGroupRequest,
    EuclideanMatchesRequest, ExploreCompositionalProfileRequest, ExploreCrosstabRequest,
    ExploreHistogramRequest, ExploreMissingProfileRequest, ExportMeasuredDataRequest,
    ExportPcaScoresRequest, ExportTransformedRequest, GetPreferencesResponse, ImportCommitRequest,
    ImportPreviewRequest, LdaRequest, MembershipProbabilitiesRequest, MergeGroupsRequest,
    PatchDescriptiveValuesRequest, PcaRequest, PutPreferenceRequest, TransferUnitsRequest,
    UmapRequest,
};
use archaeodash_desktop::{
    DesktopAppInfo, DesktopClustering, DesktopExplore, DesktopExports, DesktopFiles, DesktopGroups,
    DesktopImport, DesktopOrdination, DesktopPreferences, DesktopTransforms,
};
use std::sync::{Arc, Mutex};

mod export_files;
mod job_events;
mod logging;
mod projects;
mod source_files;
#[cfg(windows)]
mod windows_icon;

/// Project-scoped state shared by the import, group, file, transformation,
/// explore, ordination, export, and preference commands.
pub(crate) struct DesktopState {
    import: DesktopImport,
    groups: DesktopGroups,
    files: DesktopFiles,
    transforms: DesktopTransforms,
    explore: DesktopExplore,
    ordination: DesktopOrdination,
    pub(crate) clustering: DesktopClustering,
    exports: DesktopExports,
    preferences: DesktopPreferences,
    project: Option<archaeodash_contracts::ProjectInfo>,
    project_generation: u64,
}

/// Managed app state: the open project's services behind an `Arc`.
///
/// Commands lock the mutex only long enough to clone the `Arc`, then do their
/// work on Tauri's blocking thread pool. Synchronous Tauri commands run on the
/// main thread one at a time, so a slow request (a UMAP precompute, a large
/// group read) used to freeze the window and hold up every other request.
/// The services are stateless wrappers around the project root and are
/// already shared concurrently by the HTTP server, so parallel calls are safe;
/// file-level locks coordinate writers.
pub(crate) type SharedState = Mutex<Arc<DesktopState>>;

/// The current project's services (cheap `Arc` clone).
pub(crate) fn snapshot(state: &SharedState) -> Result<Arc<DesktopState>, String> {
    state
        .lock()
        .map(|current| Arc::clone(&current))
        .map_err(|e| e.to_string())
}

/// Runs a command body off the main thread against the current project.
async fn run_blocking<T, F>(state: &SharedState, op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&DesktopState) -> Result<T, String> + Send + 'static,
{
    let current = snapshot(state)?;
    tauri::async_runtime::spawn_blocking(move || op(&current))
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

impl DesktopState {
    fn empty() -> Self {
        Self {
            import: DesktopImport::new(),
            groups: DesktopGroups::new(),
            files: DesktopFiles::new(),
            transforms: DesktopTransforms::new(),
            explore: DesktopExplore::new(),
            ordination: DesktopOrdination::new(),
            clustering: DesktopClustering::new(),
            exports: DesktopExports::new(),
            preferences: DesktopPreferences::new(),
            project: None,
            project_generation: 0,
        }
    }

    pub(crate) fn for_project(path: &std::path::Path) -> Result<Self, String> {
        let root = path
            .canonicalize()
            .map_err(|e| format!("could not open project directory: {e}"))?;
        if !root.is_dir() {
            return Err("selected project path is not a directory".into());
        }
        let mut state = Self::empty();
        state.import.open_project(root.clone())?;
        state.groups.open_project(root.clone())?;
        state.files.open_project(root.clone())?;
        state.transforms.open_project(root.clone())?;
        state.explore.open_project(root.clone())?;
        state.ordination.open_project(root.clone())?;
        state.clustering.open_project(root.clone())?;
        state.exports.open_project(root.clone())?;
        state.preferences.open_project(root.clone())?;
        let name = root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("Project")
            .to_owned();
        state.project = Some(archaeodash_contracts::ProjectInfo {
            path: root.to_string_lossy().into_owned(),
            name,
            generation: 0,
        });
        Ok(state)
    }
}

#[tauri::command]
async fn submit_analysis_job(
    state: tauri::State<'_, SharedState>,
    request: archaeodash_contracts::SubmitAnalysisJobRequest,
) -> Result<archaeodash_contracts::AnalysisJobSnapshot, String> {
    run_blocking(&state, move |s| s.clustering.submit_analysis_job(request)).await
}

#[tauri::command]
async fn get_analysis_job(
    state: tauri::State<'_, SharedState>,
    id: String,
) -> Result<archaeodash_contracts::AnalysisJobSnapshot, String> {
    run_blocking(&state, move |s| s.clustering.get_analysis_job(&id)).await
}

#[tauri::command]
async fn cancel_analysis_job(
    state: tauri::State<'_, SharedState>,
    id: String,
) -> Result<archaeodash_contracts::AnalysisJobSnapshot, String> {
    run_blocking(&state, move |s| s.clustering.cancel_analysis_job(&id)).await
}

/// Smoke command exposed to the React client over Tauri IPC.
#[tauri::command]
fn app_info() -> DesktopAppInfo {
    archaeodash_desktop::app_info()
}

/// Parse-only source preview (`open_import_preview`, Section 10.4).
#[tauri::command]
async fn open_import_preview(
    state: tauri::State<'_, SharedState>,
    request: ImportPreviewRequest,
) -> Result<archaeodash_contracts::ImportPreviewResponse, String> {
    run_blocking(&state, move |s| s.import.open_import_preview(request)).await
}

/// Publish one validated group Parquet file per group (`commit_group_import`).
#[tauri::command]
async fn commit_group_import(
    state: tauri::State<'_, SharedState>,
    request: ImportCommitRequest,
) -> Result<archaeodash_contracts::ImportCommitResponse, String> {
    run_blocking(&state, move |s| s.import.commit_group_import(request)).await
}

/// Recursive candidate discovery (`scan_group_candidates`, Section 10.4).
#[tauri::command]
async fn scan_group_candidates(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<archaeodash_contracts::GroupCandidate>, String> {
    run_blocking(&state, move |s| s.groups.scan_group_candidates()).await
}

/// Full validation-on-add (`validate_group_file`).
#[tauri::command]
async fn validate_group_file(
    state: tauri::State<'_, SharedState>,
    path: String,
) -> Result<archaeodash_contracts::GroupSummary, String> {
    run_blocking(&state, move |s| s.groups.validate_group_file(path)).await
}

/// Full row data of one group file for the client dataset table
/// (`group_rows`; hidden UUIDs address edits, Section 9.4).
#[tauri::command]
async fn group_rows(
    state: tauri::State<'_, SharedState>,
    path: String,
) -> Result<archaeodash_contracts::GroupRowsResponse, String> {
    run_blocking(&state, move |s| s.groups.group_rows(path)).await
}

/// Move/copy analytical units by hidden UUID (`move_analytical_units` /
/// `copy_analytical_units`).
#[tauri::command]
async fn transfer_units(
    state: tauri::State<'_, SharedState>,
    request: TransferUnitsRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    run_blocking(&state, move |s| s.groups.transfer_units(request)).await
}

/// Move selected analytical units to multiple existing groups atomically.
#[tauri::command]
async fn batch_transfer_units(
    state: tauri::State<'_, SharedState>,
    request: BatchTransferUnitsRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    run_blocking(&state, move |s| s.groups.batch_transfer_units(request)).await
}

/// Merge whole groups into one target (`merge_groups`).
#[tauri::command]
async fn merge_groups(
    state: tauri::State<'_, SharedState>,
    request: MergeGroupsRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    run_blocking(&state, move |s| s.groups.merge_groups(request)).await
}

/// Delete one group file after exact-path confirmation and revision check
/// (`delete_group`, Section 10.4: destructive commands are never implied).
#[tauri::command]
async fn delete_group(
    state: tauri::State<'_, SharedState>,
    request: DeleteGroupRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    run_blocking(&state, move |s| s.groups.delete_group(request)).await
}

/// Batch hidden-UUID-addressed descriptive edits (`patch_descriptive_values`).
#[tauri::command]
async fn patch_descriptive_values(
    state: tauri::State<'_, SharedState>,
    request: PatchDescriptiveValuesRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    run_blocking(&state, move |s| s.groups.patch_descriptive_values(request)).await
}

/// Duplicate one whole group (`duplicate_group`), preserving UUIDs and
/// lineage by default.
#[tauri::command]
async fn duplicate_group(
    state: tauri::State<'_, SharedState>,
    request: DuplicateGroupRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    run_blocking(&state, move |s| s.groups.duplicate_group(request)).await
}

/// Stage-upload a source file through the bounded quarantine and promote it
/// to the requested in-project logical path (`upload_source_file`).
#[tauri::command]
async fn upload_source_file(
    state: tauri::State<'_, SharedState>,
    request: archaeodash_contracts::FileUploadRequest,
) -> Result<archaeodash_contracts::StagedFile, String> {
    run_blocking(&state, move |s| s.files.upload_source_file(request)).await
}

/// Quarantine-record metadata for one uploaded source file
/// (`source_file_metadata`).
#[tauri::command]
async fn source_file_metadata(
    state: tauri::State<'_, SharedState>,
    file_id: String,
) -> Result<archaeodash_contracts::StagedFile, String> {
    run_blocking(&state, move |s| s.files.source_file_metadata(file_id)).await
}

/// Raw bytes plus metadata for one uploaded source file
/// (`download_source_file`).
#[tauri::command]
async fn download_source_file(
    state: tauri::State<'_, SharedState>,
    file_id: String,
) -> Result<archaeodash_contracts::FileDownload, String> {
    run_blocking(&state, move |s| s.files.download_source_file(file_id)).await
}

/// Soft-delete one uploaded source file (`delete_source_file`).
#[tauri::command]
async fn delete_source_file(
    state: tauri::State<'_, SharedState>,
    file_id: String,
) -> Result<archaeodash_contracts::StagedFile, String> {
    run_blocking(&state, move |s| s.files.delete_source_file(file_id)).await
}

/// Save (upsert by name) one transformation definition (`save_transformation`).
#[tauri::command]
async fn save_transformation(
    state: tauri::State<'_, SharedState>,
    definition: archaeodash_contracts::TransformationDefinition,
) -> Result<archaeodash_contracts::SaveTransformationResponse, String> {
    run_blocking(&state, move |s| {
        s.transforms.save_transformation(definition)
    })
    .await
}

/// List saved transformation summaries (`list_transformations`).
#[tauri::command]
async fn list_transformations(
    state: tauri::State<'_, SharedState>,
) -> Result<archaeodash_contracts::TransformationListResponse, String> {
    run_blocking(&state, move |s| s.transforms.list_transformations()).await
}

/// Load one saved transformation definition (`load_transformation`).
#[tauri::command]
async fn load_transformation(
    state: tauri::State<'_, SharedState>,
    name: String,
) -> Result<archaeodash_contracts::TransformationDefinition, String> {
    run_blocking(&state, move |s| s.transforms.load_transformation(name)).await
}

/// Delete one saved transformation definition (`delete_transformation`).
#[tauri::command]
async fn delete_transformation(
    state: tauri::State<'_, SharedState>,
    name: String,
) -> Result<archaeodash_contracts::TransformationDefinition, String> {
    run_blocking(&state, move |s| s.transforms.delete_transformation(name)).await
}

/// One-to-one or Cartesian batch ratio-spec generation (`batch_ratio_specs`).
#[tauri::command]
async fn batch_ratio_specs(
    state: tauri::State<'_, SharedState>,
    request: BatchRatioRequest,
) -> Result<Vec<archaeodash_contracts::RatioSpecDto>, String> {
    run_blocking(&state, move |s| s.transforms.batch_ratio_specs(request)).await
}

/// Ephemeral on-demand transformation application (`apply_transformation`);
/// calculated values are never persisted (Section 5 storage invariant).
#[tauri::command]
async fn apply_transformation(
    state: tauri::State<'_, SharedState>,
    request: ApplyTransformationRequest,
) -> Result<archaeodash_contracts::AppliedTransformation, String> {
    run_blocking(&state, move |s| s.transforms.apply_transformation(request)).await
}

/// `explore_missing_profile`: `profile_missing` band summary (Section 8.12).
#[tauri::command]
async fn explore_missing_profile(
    state: tauri::State<'_, SharedState>,
    request: ExploreMissingProfileRequest,
) -> Result<archaeodash_contracts::ExploreMissingProfileResponse, String> {
    run_blocking(&state, move |s| s.explore.explore_missing_profile(request)).await
}

/// `explore_histogram`: `hist.default` breakpoints and counts.
#[tauri::command]
async fn explore_histogram(
    state: tauri::State<'_, SharedState>,
    request: ExploreHistogramRequest,
) -> Result<archaeodash_contracts::ExploreHistogramResponse, String> {
    run_blocking(&state, move |s| s.explore.explore_histogram(request)).await
}

/// `explore_crosstab`: legacy `compute_crosstab_summary`.
#[tauri::command]
async fn explore_crosstab(
    state: tauri::State<'_, SharedState>,
    request: ExploreCrosstabRequest,
) -> Result<archaeodash_contracts::ExploreCrosstabResponse, String> {
    run_blocking(&state, move |s| s.explore.explore_crosstab(request)).await
}

/// `explore_compositional_profile`: the `pivot_longer` long table.
#[tauri::command]
async fn explore_compositional_profile(
    state: tauri::State<'_, SharedState>,
    request: ExploreCompositionalProfileRequest,
) -> Result<archaeodash_contracts::ExploreCompositionalProfileResponse, String> {
    run_blocking(&state, move |s| {
        s.explore.explore_compositional_profile(request)
    })
    .await
}

/// `ordination_pca`: prcomp-parity PCA; results are ephemeral and never
/// persisted (Section 5 storage invariant).
#[tauri::command]
async fn ordination_pca(
    state: tauri::State<'_, SharedState>,
    request: PcaRequest,
) -> Result<archaeodash_contracts::PcaResponse, String> {
    run_blocking(&state, move |s| s.ordination.ordination_pca(request)).await
}

/// `ordination_lda`: `MASS::lda` moment-method parity with the legacy
/// three-group minimum; results are ephemeral and never persisted (Section 5).
#[tauri::command]
async fn ordination_lda(
    state: tauri::State<'_, SharedState>,
    request: LdaRequest,
) -> Result<archaeodash_contracts::LdaResponse, String> {
    run_blocking(&state, move |s| s.ordination.ordination_lda(request)).await
}

/// `ordination_umap`: legacy naive UMAP parity with the deterministic seed;
/// results are ephemeral and never persisted (Section 5 storage invariant).
/// `ordination_umap`: legacy naive UMAP parity; results are ephemeral and
/// never persisted (Section 5).
#[tauri::command]
async fn ordination_umap(
    state: tauri::State<'_, SharedState>,
    request: UmapRequest,
) -> Result<archaeodash_contracts::UmapResponse, String> {
    run_blocking(&state, move |s| s.ordination.ordination_umap(request)).await
}

/// `cluster_diagnostics`: WSS elbow + mean-silhouette series; results are
/// ephemeral and never persisted (Section 5).
#[tauri::command]
async fn cluster_diagnostics(
    state: tauri::State<'_, SharedState>,
    request: ClusterDiagnosticsRequest,
) -> Result<archaeodash_contracts::ClusterDiagnosticsResponse, String> {
    run_blocking(&state, move |s| s.clustering.cluster_diagnostics(request)).await
}

/// `cluster_fit`: one kmeans/pam/ward.D2/DIANA fit; results are ephemeral
/// and never persisted (Section 5).
#[tauri::command]
async fn cluster_fit(
    state: tauri::State<'_, SharedState>,
    request: ClusterFitRequest,
) -> Result<archaeodash_contracts::ClusterFitResponse, String> {
    run_blocking(&state, move |s| s.clustering.cluster_fit(request)).await
}

/// `membership_probabilities`: `group.mem.probs` parity; results are
/// ephemeral and never persisted (Section 5).
#[tauri::command]
async fn membership_probabilities(
    state: tauri::State<'_, SharedState>,
    request: MembershipProbabilitiesRequest,
) -> Result<archaeodash_contracts::MembershipProbabilitiesResponse, String> {
    run_blocking(&state, move |s| {
        s.clustering.membership_probabilities(request)
    })
    .await
}

/// `euclidean_matches`: `calcEDistance` parity; results are ephemeral and
/// never persisted (Section 5).
#[tauri::command]
async fn euclidean_matches(
    state: tauri::State<'_, SharedState>,
    request: EuclideanMatchesRequest,
) -> Result<archaeodash_contracts::EuclideanMatchesResponse, String> {
    run_blocking(&state, move |s| s.clustering.euclidean_matches(request)).await
}

/// `export_measured_data`: measured chemical frame as ephemeral CSV
/// (Section 7.3; legacy `rvals$selectedData`).
#[tauri::command]
async fn export_measured_data(
    state: tauri::State<'_, SharedState>,
    request: ExportMeasuredDataRequest,
) -> Result<archaeodash_contracts::ExportResult, String> {
    run_blocking(&state, move |s| s.exports.export_measured_data(request)).await
}

/// `export_transformed`: explicitly computed transformed result as ephemeral
/// CSV (Section 7.3; never persisted, Section 5 storage invariant).
#[tauri::command]
async fn export_transformed(
    state: tauri::State<'_, SharedState>,
    request: ExportTransformedRequest,
) -> Result<archaeodash_contracts::ExportResult, String> {
    run_blocking(&state, move |s| s.exports.export_transformed(request)).await
}

/// `export_pca_scores`: computed PCA score frame as ephemeral CSV (Section 3.2
/// correction of the legacy `rvals$pcaData` bug).
#[tauri::command]
async fn export_pca_scores(
    state: tauri::State<'_, SharedState>,
    request: ExportPcaScoresRequest,
) -> Result<archaeodash_contracts::ExportResult, String> {
    run_blocking(&state, move |s| s.exports.export_pca_scores(request)).await
}

/// `preferences_get`: every stored allowlisted preference (Section 10.1).
#[tauri::command]
async fn preferences_get(
    state: tauri::State<'_, SharedState>,
) -> Result<GetPreferencesResponse, String> {
    run_blocking(&state, move |s| s.preferences.preferences_get()).await
}

/// `preferences_set`: upsert one allowlisted, shape-validated preference.
#[tauri::command]
async fn preferences_set(
    state: tauri::State<'_, SharedState>,
    request: PutPreferenceRequest,
) -> Result<(), String> {
    run_blocking(&state, move |s| s.preferences.preferences_set(request)).await
}

#[tauri::command]
async fn list_group_folders(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<archaeodash_contracts::GroupFolder>, String> {
    run_blocking(&state, move |s| s.groups.list_folders()).await
}

#[tauri::command]
async fn create_group_folder(
    state: tauri::State<'_, SharedState>,
    request: archaeodash_contracts::CreateFolderRequest,
) -> Result<archaeodash_contracts::GroupFolder, String> {
    run_blocking(&state, move |s| s.groups.create_folder(request)).await
}

#[tauri::command]
async fn rename_group_folder(
    state: tauri::State<'_, SharedState>,
    request: archaeodash_contracts::RenameFolderRequest,
) -> Result<archaeodash_contracts::GroupFolder, String> {
    run_blocking(&state, move |s| s.groups.rename_folder(request)).await
}

#[tauri::command]
async fn delete_group_folder(
    state: tauri::State<'_, SharedState>,
    request: archaeodash_contracts::DeleteFolderRequest,
) -> Result<(), String> {
    run_blocking(&state, move |s| s.groups.delete_folder(request)).await
}

/// Copy or move whole groups into a folder (`copy_groups`).
#[tauri::command]
async fn copy_groups(
    state: tauri::State<'_, SharedState>,
    request: archaeodash_contracts::CopyGroupsRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    run_blocking(&state, move |s| s.groups.copy_groups(request)).await
}

/// `export_table`: any result table as CSV or XLSX (no project access).
#[tauri::command]
async fn export_table(
    request: archaeodash_contracts::ExportTableRequest,
) -> Result<archaeodash_contracts::ExportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        archaeodash_application::export_table(&request).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::expect_used)] // app entry point: a failed runtime start must abort startup
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Rotating desktop logs (Section 12). The worker guard must
            // outlive the process for flush-on-drop; the app runs until
            // exit, so forgetting it is the documented pattern.
            let log_dir = app.path().app_log_dir().ok();
            if let Some(guard) = logging::init(log_dir.as_deref()) {
                std::mem::forget(guard);
            }
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "desktop app starting");
            // Sharp taskbar/title-bar icons from the exe's multi-size icon.
            #[cfg(windows)]
            for window in app.webview_windows().values() {
                if let Ok(hwnd) = window.hwnd() {
                    windows_icon::apply(hwnd.0);
                }
            }
            Ok(())
        })
        .on_window_event(|_window, _event| {
            // Icon sizes depend on the monitor's scale.
            #[cfg(windows)]
            if let tauri::WindowEvent::ScaleFactorChanged { .. } = _event {
                if let Ok(hwnd) = _window.hwnd() {
                    windows_icon::apply(hwnd.0);
                }
            }
        })
        .manage::<SharedState>(Mutex::new(Arc::new(DesktopState::empty())))
        .invoke_handler(tauri::generate_handler![
            app_info,
            projects::open_project,
            projects::current_project,
            projects::recent_projects,
            projects::open_recent_project,
            projects::remove_recent_project,
            projects::clear_recent_projects,
            source_files::pick_and_upload_source,
            submit_analysis_job,
            get_analysis_job,
            cancel_analysis_job,
            job_events::watch_analysis_job,
            job_events::stop_analysis_job_watch,
            open_import_preview,
            commit_group_import,
            scan_group_candidates,
            validate_group_file,
            transfer_units,
            batch_transfer_units,
            merge_groups,
            delete_group,
            patch_descriptive_values,
            duplicate_group,
            upload_source_file,
            source_file_metadata,
            download_source_file,
            delete_source_file,
            group_rows,
            save_transformation,
            list_transformations,
            load_transformation,
            delete_transformation,
            batch_ratio_specs,
            apply_transformation,
            explore_missing_profile,
            explore_histogram,
            explore_crosstab,
            explore_compositional_profile,
            ordination_pca,
            ordination_lda,
            ordination_umap,
            cluster_diagnostics,
            cluster_fit,
            membership_probabilities,
            euclidean_matches,
            export_measured_data,
            export_transformed,
            export_pca_scores,
            export_files::save_export_file,
            export_table,
            projects::reveal_project,
            list_group_folders,
            create_group_folder,
            rename_group_folder,
            delete_group_folder,
            copy_groups,
            preferences_get,
            preferences_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod project_state_tests {
    use super::DesktopState;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("archaeodash-{name}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn project_services_are_constructed_together_and_failed_candidate_preserves_current() {
        let first = temp_dir("project-one");
        let second = temp_dir("project-two");
        let current = DesktopState::for_project(&first).unwrap();
        assert_eq!(
            current.project.as_ref().unwrap().name,
            first.file_name().unwrap().to_string_lossy()
        );

        let replacement = DesktopState::for_project(&second).unwrap();
        assert_eq!(
            replacement.project.as_ref().unwrap().path,
            second.canonicalize().unwrap().to_string_lossy()
        );

        // The native command only assigns after candidate construction succeeds.
        let invalid = first.join("missing");
        assert!(DesktopState::for_project(&invalid).is_err());
        assert_eq!(
            current.project.as_ref().unwrap().path,
            first.canonicalize().unwrap().to_string_lossy()
        );

        let _ = std::fs::remove_dir_all(first);
        let _ = std::fs::remove_dir_all(second);
    }
}
