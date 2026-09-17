//! Tauri 2 desktop shell. Commands delegate to the `archaeodash-desktop`
//! crate (crates/desktop), which calls the shared application use cases.

use archaeodash_contracts::{
    ImportCommitRequest, ImportPreviewRequest, MergeGroupsRequest, TransferUnitsRequest,
};
use archaeodash_desktop::{DesktopAppInfo, DesktopGroups, DesktopImport};
use std::sync::Mutex;

/// Project-scoped state shared by the import and group commands.
struct DesktopState {
    import: DesktopImport,
    groups: DesktopGroups,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::expect_used)] // app entry point: a failed runtime start must abort startup
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(DesktopState {
            import: DesktopImport::new(),
            groups: DesktopGroups::new(),
        }))
        .invoke_handler(tauri::generate_handler![
            app_info,
            open_import_preview,
            commit_group_import,
            scan_group_candidates,
            validate_group_file,
            transfer_units,
            merge_groups
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
