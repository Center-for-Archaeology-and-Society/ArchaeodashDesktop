//! Tauri command surface (desktop adapter) delegating to
//! `archaeodash-application`. The `#[tauri::command]` wrappers live in the
//! `apps/desktop/src-tauri` shell; this crate keeps the command payloads and
//! invocation logic testable without a webview runtime.

use archaeodash_contracts::AppInfo;

/// Payload for the desktop `app_info` command.
pub type DesktopAppInfo = AppInfo;

/// Executes the desktop smoke use case: same application call as the HTTP
/// adapter, proving the two adapters share one core.
pub fn app_info() -> DesktopAppInfo {
    archaeodash_application::app_info("tauri", true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_smoke_reports_tauri_transport() {
        assert_eq!(app_info().transport, "tauri");
        assert!(app_info().ready);
    }
}
