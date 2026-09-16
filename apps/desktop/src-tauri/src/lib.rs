//! Tauri 2 desktop shell. Commands delegate to the `archaeodash-desktop`
//! crate (crates/desktop), which calls the shared application use cases.

use archaeodash_contracts::{ImportCommitRequest, ImportPreviewRequest};
use archaeodash_desktop::{DesktopAppInfo, DesktopImport};
use std::sync::Mutex;

/// Project-scoped import state shared by the import commands.
struct DesktopState {
    import: DesktopImport,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::expect_used)] // app entry point: a failed runtime start must abort startup
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(DesktopState {
            import: DesktopImport::new(),
        }))
        .invoke_handler(tauri::generate_handler![
            app_info,
            open_import_preview,
            commit_group_import
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
