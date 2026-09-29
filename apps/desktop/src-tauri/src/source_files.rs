use archaeodash_application::files::MAX_UPLOAD_BYTES;
use archaeodash_contracts::{FileUploadRequest, StagedFile};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri_plugin_dialog::DialogExt;

fn safe_source_name(path: &Path) -> Result<String, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| matches!(extension.as_str(), "csv" | "tsv" | "xlsx"))
        .ok_or_else(|| "choose a CSV, TSV, or XLSX source file".to_string())?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("source");
    let safe_stem: String = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let stem = safe_stem.trim_matches('_');
    let stem = if stem.is_empty() { "source" } else { stem };
    Ok(format!("{stem}.{extension}"))
}

/// Opens the native source picker at the active project's root on every open,
/// then stages the selected file into that project's bounded source catalog.
/// JavaScript supplies neither a host path nor source bytes.
#[tauri::command]
pub(crate) async fn pick_and_upload_source(
    app: tauri::AppHandle,
    state: tauri::State<'_, Mutex<super::DesktopState>>,
) -> Result<Option<StagedFile>, String> {
    let (project_root, project_generation) = {
        let current = state.lock().map_err(|error| error.to_string())?;
        let project = current
            .project
            .as_ref()
            .ok_or_else(|| "open a project before importing a source file".to_string())?;
        (PathBuf::from(&project.path), current.project_generation)
    };

    let selected = tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .set_directory(&project_root)
            .add_filter("Source data", &["csv", "tsv", "xlsx"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let source_path = selected
            .into_path()
            .map_err(|error| format!("invalid selected source path: {error}"))?;
        let source_name = safe_source_name(&source_path)?;
        let metadata = std::fs::metadata(&source_path)
            .map_err(|error| format!("could not read selected source metadata: {error}"))?;
        if !metadata.is_file() {
            return Err("the selected source is not a regular file".to_string());
        }
        if metadata.len() > MAX_UPLOAD_BYTES {
            return Err(format!(
                "source file exceeds the {} MiB import limit",
                MAX_UPLOAD_BYTES / (1024 * 1024)
            ));
        }
        let content = std::fs::read(&source_path)
            .map_err(|error| format!("could not read selected source: {error}"))?;
        Ok(Some((source_name, content)))
    })
    .await
    .map_err(|error| format!("source file picker failed: {error}"))??;

    let Some((source_name, content)) = selected else {
        return Ok(None);
    };

    let current = state.lock().map_err(|error| error.to_string())?;
    if current.project_generation != project_generation {
        return Err(
            "the active project changed while selecting a source; choose the file again".into(),
        );
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_nanos();
    let path = format!("sources/{nonce}-{source_name}");
    current
        .files
        .upload_source_file(FileUploadRequest { path, content })
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::safe_source_name;
    use std::path::Path;

    #[test]
    fn source_names_are_flat_and_keep_supported_extensions() {
        assert_eq!(
            safe_source_name(Path::new("/tmp/site data.CSV")).unwrap(),
            "site_data.csv"
        );
        assert_eq!(
            safe_source_name(Path::new("/tmp/study.tsv")).unwrap(),
            "study.tsv"
        );
        assert_eq!(
            safe_source_name(Path::new("/tmp/table.xlsx")).unwrap(),
            "table.xlsx"
        );
        assert!(safe_source_name(Path::new("/tmp/file.parquet")).is_err());
        assert!(safe_source_name(Path::new("/tmp/.csv")).is_err());
    }
}
