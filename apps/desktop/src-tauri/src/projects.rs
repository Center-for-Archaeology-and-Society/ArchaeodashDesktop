use archaeodash_contracts::{ProjectInfo, RecentProject};
use archaeodash_desktop::recent::{now_unix_secs, RecentProjects};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub(crate) fn current_project(
    state: tauri::State<'_, super::SharedState>,
) -> Result<Option<ProjectInfo>, String> {
    Ok(state.lock().map_err(|e| e.to_string())?.project.clone())
}

/// The recent-projects list, stored in the app's config directory.
fn recent_store(app: &tauri::AppHandle) -> Result<RecentProjects, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("no app config directory: {e}"))?;
    Ok(RecentProjects::new(dir))
}

/// Swaps in a freshly opened project and bumps the generation.
fn activate(
    state: &super::SharedState,
    mut candidate: super::DesktopState,
) -> Result<Option<ProjectInfo>, String> {
    let mut current = state.lock().map_err(|e| e.to_string())?;
    candidate.project_generation = current.project_generation.wrapping_add(1);
    if let Some(project) = candidate.project.as_mut() {
        project.generation = candidate.project_generation;
    }
    let info = candidate.project.clone();
    *current = Arc::new(candidate);
    Ok(info)
}

/// Puts an opened project at the top of the recent list (best effort: a
/// config-directory problem never blocks opening the project).
fn remember(app: &tauri::AppHandle, info: Option<&ProjectInfo>) {
    let Some(project) = info else { return };
    let saved = recent_store(app)
        .and_then(|store| store.record(&project.path, &project.name, now_unix_secs()));
    if let Err(error) = saved {
        tracing::warn!(%error, "could not update the recent projects list");
    }
}

/// Opens a native directory picker. JavaScript never supplies a filesystem path.
#[tauri::command]
pub(crate) async fn open_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, super::SharedState>,
) -> Result<Option<ProjectInfo>, String> {
    let picker = app.clone();
    let candidate = tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = picker.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|e| format!("invalid selected directory: {e}"))?;
        super::DesktopState::for_project(&path).map(Some)
    })
    .await
    .map_err(|e| format!("project picker failed: {e}"))??;
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let info = activate(&state, candidate)?;
    remember(&app, info.as_ref());
    Ok(info)
}

/// Recent projects, most recent first, with whether each folder still exists.
#[tauri::command]
pub(crate) fn recent_projects(app: tauri::AppHandle) -> Result<Vec<RecentProject>, String> {
    Ok(recent_store(&app)?.list())
}

/// Reopens a project from the recent list. Only paths already on the list
/// (added by the native picker) are accepted, so JavaScript still cannot
/// open an arbitrary directory.
#[tauri::command]
pub(crate) async fn open_recent_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, super::SharedState>,
    path: String,
) -> Result<Option<ProjectInfo>, String> {
    if !recent_store(&app)?.contains(&path) {
        return Err("that folder is not in the recent projects list".into());
    }
    let candidate = tauri::async_runtime::spawn_blocking(move || {
        super::DesktopState::for_project(std::path::Path::new(&path))
    })
    .await
    .map_err(|e| format!("opening the project failed: {e}"))??;
    let info = activate(&state, candidate)?;
    remember(&app, info.as_ref());
    Ok(info)
}

/// Removes one project from the recent list (the folder is untouched).
#[tauri::command]
pub(crate) fn remove_recent_project(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<RecentProject>, String> {
    recent_store(&app)?.remove(&path)
}

/// Empties the recent list.
#[tauri::command]
pub(crate) fn clear_recent_projects(app: tauri::AppHandle) -> Result<(), String> {
    recent_store(&app)?.clear()
}
