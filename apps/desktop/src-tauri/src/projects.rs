use archaeodash_contracts::ProjectInfo;
use std::sync::Mutex;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub(crate) fn current_project(
    state: tauri::State<'_, Mutex<super::DesktopState>>,
) -> Result<Option<ProjectInfo>, String> {
    Ok(state.lock().map_err(|e| e.to_string())?.project.clone())
}

/// Opens a native directory picker. JavaScript never supplies a filesystem path.
#[tauri::command]
pub(crate) async fn open_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<super::DesktopState>>,
) -> Result<Option<ProjectInfo>, String> {
    let app = app.clone();
    let candidate = tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|e| format!("invalid selected directory: {e}"))?;
        super::DesktopState::for_project(&path).map(Some)
    })
    .await
    .map_err(|e| format!("project picker failed: {e}"))??;
    let Some(mut candidate) = candidate else {
        return Ok(None);
    };
    let mut current = state.lock().map_err(|e| e.to_string())?;
    candidate.project_generation = current.project_generation.wrapping_add(1);
    if let Some(project) = candidate.project.as_mut() {
        project.generation = candidate.project_generation;
    }
    let info = candidate.project.clone();
    *current = candidate;
    Ok(info)
}
