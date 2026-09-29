use archaeodash_contracts::ExportResult;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri_plugin_dialog::DialogExt;

const MAX_EXPORT_BYTES: usize = 32 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn validate_file_name(file_name: &str) -> Result<(), String> {
    let mut components = Path::new(file_name).components();
    let is_one_normal_component =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if file_name.is_empty()
        || file_name.len() > 180
        || file_name.contains(['/', '\\'])
        || file_name.chars().any(char::is_control)
        || !is_one_normal_component
    {
        return Err("export filename must be a safe basename".into());
    }
    Ok(())
}

fn write_export(path: &Path, content: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| "selected output path has no filename".to_string())?;
    let mut temporary = None;
    for _ in 0..16 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.archaeodash-export-{}-{sequence}.tmp",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("could not prepare export file: {error}")),
        }
    }
    let (temporary_path, mut file) =
        temporary.ok_or_else(|| "could not allocate a temporary export file".to_string())?;

    let write_result = file
        .write_all(content)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not write export: {error}"));
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("could not publish export: {error}"));
    }
    Ok(())
}

fn validate_export(result: &ExportResult) -> Result<(), String> {
    validate_file_name(&result.file_name)?;
    if result.content.len() > MAX_EXPORT_BYTES {
        return Err(format!(
            "export exceeds the {} MiB save limit",
            MAX_EXPORT_BYTES / (1024 * 1024)
        ));
    }
    Ok(())
}

/// Opens a native save dialog; JavaScript supplies bytes and a suggested name,
/// never an output path. Returns false when the user cancels.
#[tauri::command]
pub(crate) async fn save_export_file(
    app: tauri::AppHandle,
    result: ExportResult,
) -> Result<bool, String> {
    validate_export(&result)?;

    let selected_path = tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_file_name(&result.file_name)
            .add_filter("CSV", &["csv"])
            .blocking_save_file();
        selected
            .map(|file| {
                file.into_path()
                    .map_err(|error| format!("invalid selected output path: {error}"))
            })
            .transpose()
    })
    .await
    .map_err(|error| format!("save dialog failed: {error}"))??;

    let Some(path) = selected_path else {
        return Ok(false);
    };
    let content = result.content.into_bytes();
    tauri::async_runtime::spawn_blocking(move || write_export(&path, &content))
        .await
        .map_err(|error| format!("export write failed: {error}"))??;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{validate_export, validate_file_name, write_export, MAX_EXPORT_BYTES};
    use archaeodash_contracts::ExportResult;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "archaeodash-export-file-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        path
    }

    #[test]
    fn accepts_a_basename_and_rejects_paths_and_controls() {
        assert!(validate_file_name("measured-data.csv").is_ok());
        assert!(validate_file_name("../measured-data.csv").is_err());
        assert!(validate_file_name("folder\\measured-data.csv").is_err());
        assert!(validate_file_name("bad\nname.csv").is_err());
        assert!(validate_file_name(&"x".repeat(181)).is_err());
    }

    #[test]
    fn rejects_oversized_export_before_opening_a_dialog() {
        let result = ExportResult {
            file_name: "large.csv".into(),
            media_type: "text/csv".into(),
            content: "x".repeat(MAX_EXPORT_BYTES + 1),
        };
        assert!(validate_export(&result).is_err());
    }

    #[test]
    fn write_saves_bytes_and_reports_path_errors_without_touching_directory_contents() {
        let root = temp_dir();
        let destination = root.join("export.csv");
        std::fs::write(&destination, b"previous contents").expect("write previous export");
        write_export(&destination, b"a,b\n1,2\n").expect("write export");
        assert_eq!(
            std::fs::read(&destination).expect("read export"),
            b"a,b\n1,2\n"
        );

        let protected_dir = root.join("protected");
        std::fs::create_dir_all(&protected_dir).expect("create protected directory");
        let marker = protected_dir.join("keep.txt");
        std::fs::write(&marker, b"keep").expect("write marker");
        assert!(write_export(&protected_dir, b"replacement").is_err());
        assert_eq!(std::fs::read(&marker).expect("read marker"), b"keep");
        assert!(std::fs::read_dir(&root)
            .expect("read test directory")
            .all(|entry| !entry
                .expect("read directory entry")
                .file_name()
                .to_string_lossy()
                .contains("archaeodash-export-")));
    }
}
