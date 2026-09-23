//! Tauri 2 desktop shell. Commands delegate to the `archaeodash-desktop`
//! crate (crates/desktop), which calls the shared application use cases.

use archaeodash_contracts::{
    ApplyTransformationRequest, BatchRatioRequest, DeleteGroupRequest, DuplicateGroupRequest,
    ExploreCompositionalProfileRequest, ExploreCrosstabRequest, ExploreHistogramRequest,
    ExploreMissingProfileRequest, ExportMeasuredDataRequest, ExportPcaScoresRequest,
    ExportTransformedRequest, GetPreferencesResponse, ImportCommitRequest, ImportPreviewRequest,
    LdaRequest, MergeGroupsRequest, PatchDescriptiveValuesRequest, PcaRequest,
    PutPreferenceRequest, TransferUnitsRequest, UmapRequest,
};
use archaeodash_desktop::{
    DesktopAppInfo, DesktopExplore, DesktopExports, DesktopFiles, DesktopGroups, DesktopImport,
    DesktopOrdination, DesktopPreferences, DesktopTransforms,
};
use std::sync::Mutex;

/// Project-scoped state shared by the import, group, file, transformation,
/// explore, ordination, export, and preference commands.
struct DesktopState {
    import: DesktopImport,
    groups: DesktopGroups,
    files: DesktopFiles,
    transforms: DesktopTransforms,
    explore: DesktopExplore,
    ordination: DesktopOrdination,
    exports: DesktopExports,
    preferences: DesktopPreferences,
}

/// Smoke command exposed to the React client over Tauri IPC.
#[tauri::command]
fn app_info() -> DesktopAppInfo {
    archaeodash_desktop::app_info()
}

/// Parse-only source preview (`open_import_preview`, Section 10.4).
#[tauri::command]
fn open_import_preview(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ImportPreviewRequest,
) -> Result<archaeodash_contracts::ImportPreviewResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .import
        .open_import_preview(request)
}

/// Publish one validated group Parquet file per group (`commit_group_import`).
#[tauri::command]
fn commit_group_import(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ImportCommitRequest,
) -> Result<archaeodash_contracts::ImportCommitResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .import
        .commit_group_import(request)
}

/// Recursive candidate discovery (`scan_group_candidates`, Section 10.4).
#[tauri::command]
fn scan_group_candidates(
    state: tauri::State<'_, Mutex<DesktopState>>,
) -> Result<Vec<archaeodash_contracts::GroupCandidate>, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .groups
        .scan_group_candidates()
}

/// Full validation-on-add (`validate_group_file`).
#[tauri::command]
fn validate_group_file(
    state: tauri::State<'_, Mutex<DesktopState>>,
    path: String,
) -> Result<archaeodash_contracts::GroupSummary, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .groups
        .validate_group_file(path)
}

/// Full row data of one group file for the client dataset table
/// (`group_rows`; hidden UUIDs address edits, Section 9.4).
#[tauri::command]
fn group_rows(
    state: tauri::State<'_, Mutex<DesktopState>>,
    path: String,
) -> Result<archaeodash_contracts::GroupRowsResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .groups
        .group_rows(path)
}

/// Move/copy analytical units by hidden UUID (`move_analytical_units` /
/// `copy_analytical_units`).
#[tauri::command]
fn transfer_units(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: TransferUnitsRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .groups
        .transfer_units(request)
}

/// Merge whole groups into one target (`merge_groups`).
#[tauri::command]
fn merge_groups(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: MergeGroupsRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .groups
        .merge_groups(request)
}

/// Delete one group file after exact-path confirmation and revision check
/// (`delete_group`, Section 10.4: destructive commands are never implied).
#[tauri::command]
fn delete_group(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: DeleteGroupRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .groups
        .delete_group(request)
}

/// Batch hidden-UUID-addressed descriptive edits (`patch_descriptive_values`).
#[tauri::command]
fn patch_descriptive_values(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: PatchDescriptiveValuesRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .groups
        .patch_descriptive_values(request)
}

/// Duplicate one whole group (`duplicate_group`), preserving UUIDs and
/// lineage by default.
#[tauri::command]
fn duplicate_group(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: DuplicateGroupRequest,
) -> Result<archaeodash_contracts::TransactionResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .groups
        .duplicate_group(request)
}

/// Stage-upload a source file through the bounded quarantine and promote it
/// to the requested in-project logical path (`upload_source_file`).
#[tauri::command]
fn upload_source_file(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: archaeodash_contracts::FileUploadRequest,
) -> Result<archaeodash_contracts::StagedFile, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .files
        .upload_source_file(request)
}

/// Quarantine-record metadata for one uploaded source file
/// (`source_file_metadata`).
#[tauri::command]
fn source_file_metadata(
    state: tauri::State<'_, Mutex<DesktopState>>,
    file_id: String,
) -> Result<archaeodash_contracts::StagedFile, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .files
        .source_file_metadata(file_id)
}

/// Raw bytes plus metadata for one uploaded source file
/// (`download_source_file`).
#[tauri::command]
fn download_source_file(
    state: tauri::State<'_, Mutex<DesktopState>>,
    file_id: String,
) -> Result<archaeodash_contracts::FileDownload, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .files
        .download_source_file(file_id)
}

/// Soft-delete one uploaded source file (`delete_source_file`).
#[tauri::command]
fn delete_source_file(
    state: tauri::State<'_, Mutex<DesktopState>>,
    file_id: String,
) -> Result<archaeodash_contracts::StagedFile, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .files
        .delete_source_file(file_id)
}

/// Save (upsert by name) one transformation definition (`save_transformation`).
#[tauri::command]
fn save_transformation(
    state: tauri::State<'_, Mutex<DesktopState>>,
    definition: archaeodash_contracts::TransformationDefinition,
) -> Result<archaeodash_contracts::SaveTransformationResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .transforms
        .save_transformation(definition)
}

/// List saved transformation summaries (`list_transformations`).
#[tauri::command]
fn list_transformations(
    state: tauri::State<'_, Mutex<DesktopState>>,
) -> Result<archaeodash_contracts::TransformationListResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .transforms
        .list_transformations()
}

/// Load one saved transformation definition (`load_transformation`).
#[tauri::command]
fn load_transformation(
    state: tauri::State<'_, Mutex<DesktopState>>,
    name: String,
) -> Result<archaeodash_contracts::TransformationDefinition, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .transforms
        .load_transformation(name)
}

/// Delete one saved transformation definition (`delete_transformation`).
#[tauri::command]
fn delete_transformation(
    state: tauri::State<'_, Mutex<DesktopState>>,
    name: String,
) -> Result<archaeodash_contracts::TransformationDefinition, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .transforms
        .delete_transformation(name)
}

/// One-to-one or Cartesian batch ratio-spec generation (`batch_ratio_specs`).
#[tauri::command]
fn batch_ratio_specs(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: BatchRatioRequest,
) -> Result<Vec<archaeodash_contracts::RatioSpecDto>, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .transforms
        .batch_ratio_specs(request)
}

/// Ephemeral on-demand transformation application (`apply_transformation`);
/// calculated values are never persisted (Section 5 storage invariant).
#[tauri::command]
fn apply_transformation(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ApplyTransformationRequest,
) -> Result<archaeodash_contracts::AppliedTransformation, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .transforms
        .apply_transformation(request)
}

/// `explore_missing_profile`: `profile_missing` band summary (Section 8.12).
#[tauri::command]
fn explore_missing_profile(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ExploreMissingProfileRequest,
) -> Result<archaeodash_contracts::ExploreMissingProfileResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .explore
        .explore_missing_profile(request)
}

/// `explore_histogram`: `hist.default` breakpoints and counts.
#[tauri::command]
fn explore_histogram(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ExploreHistogramRequest,
) -> Result<archaeodash_contracts::ExploreHistogramResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .explore
        .explore_histogram(request)
}

/// `explore_crosstab`: legacy `compute_crosstab_summary`.
#[tauri::command]
fn explore_crosstab(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ExploreCrosstabRequest,
) -> Result<archaeodash_contracts::ExploreCrosstabResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .explore
        .explore_crosstab(request)
}

/// `explore_compositional_profile`: the `pivot_longer` long table.
#[tauri::command]
fn explore_compositional_profile(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ExploreCompositionalProfileRequest,
) -> Result<archaeodash_contracts::ExploreCompositionalProfileResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .explore
        .explore_compositional_profile(request)
}

/// `ordination_pca`: prcomp-parity PCA; results are ephemeral and never
/// persisted (Section 5 storage invariant).
#[tauri::command]
fn ordination_pca(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: PcaRequest,
) -> Result<archaeodash_contracts::PcaResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .ordination
        .ordination_pca(request)
}

/// `ordination_lda`: `MASS::lda` moment-method parity with the legacy
/// three-group minimum; results are ephemeral and never persisted (Section 5).
#[tauri::command]
fn ordination_lda(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: LdaRequest,
) -> Result<archaeodash_contracts::LdaResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .ordination
        .ordination_lda(request)
}

/// `ordination_umap`: legacy naive UMAP parity with the deterministic seed;
/// results are ephemeral and never persisted (Section 5 storage invariant).
#[tauri::command]
fn ordination_umap(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: UmapRequest,
) -> Result<archaeodash_contracts::UmapResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .ordination
        .ordination_umap(request)
}

/// `export_measured_data`: measured chemical frame as ephemeral CSV
/// (Section 7.3; legacy `rvals$selectedData`).
#[tauri::command]
fn export_measured_data(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ExportMeasuredDataRequest,
) -> Result<archaeodash_contracts::ExportResult, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .exports
        .export_measured_data(request)
}

/// `export_transformed`: explicitly computed transformed result as ephemeral
/// CSV (Section 7.3; never persisted, Section 5 storage invariant).
#[tauri::command]
fn export_transformed(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ExportTransformedRequest,
) -> Result<archaeodash_contracts::ExportResult, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .exports
        .export_transformed(request)
}

/// `export_pca_scores`: computed PCA score frame as ephemeral CSV (Section 3.2
/// correction of the legacy `rvals$pcaData` bug).
#[tauri::command]
fn export_pca_scores(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: ExportPcaScoresRequest,
) -> Result<archaeodash_contracts::ExportResult, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .exports
        .export_pca_scores(request)
}

/// `preferences_get`: every stored allowlisted preference (Section 10.1).
#[tauri::command]
fn preferences_get(
    state: tauri::State<'_, Mutex<DesktopState>>,
) -> Result<GetPreferencesResponse, String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .preferences
        .preferences_get()
}

/// `preferences_set`: upsert one allowlisted, shape-validated preference.
#[tauri::command]
fn preferences_set(
    state: tauri::State<'_, Mutex<DesktopState>>,
    request: PutPreferenceRequest,
) -> Result<(), String> {
    state
        .lock()
        .map_err(|e| e.to_string())?
        .preferences
        .preferences_set(request)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::expect_used)] // app entry point: a failed runtime start must abort startup
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(DesktopState {
            import: DesktopImport::new(),
            groups: DesktopGroups::new(),
            files: DesktopFiles::new(),
            transforms: DesktopTransforms::new(),
            explore: DesktopExplore::new(),
            ordination: DesktopOrdination::new(),
            exports: DesktopExports::new(),
            preferences: DesktopPreferences::new(),
        }))
        .invoke_handler(tauri::generate_handler![
            app_info,
            open_import_preview,
            commit_group_import,
            scan_group_candidates,
            validate_group_file,
            transfer_units,
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
            export_measured_data,
            export_transformed,
            export_pca_scores,
            preferences_get,
            preferences_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
