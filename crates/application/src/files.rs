//! Source-file upload/catalog use cases shared by the HTTP and Tauri
//! adapters (Section 10.2 `POST /projects/{id}/files`, `GET /files/{id}`,
//! `GET /files/{id}/download`, `DELETE /files/{id}`).
//!
//! Local Phase-2 form: there is no hosted control plane yet, so the
//! quarantine record directory `.archaeodash/quarantine/` under the opened
//! project root is the file catalog. Uploads stage bytes into quarantine
//! under a bounded size, compute a SHA-256 checksum, parse-check CSV
//! sources, then promote atomically to the user-selected in-project logical
//! path. The hosted `UserFileStore`/catalog replaces this layout in Phase 7.

use std::path::PathBuf;

use archaeodash_contracts::{FileDownload, StagedFile};
use archaeodash_data_io::{data_loader, sync_dir, ImportError};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Upload size bound (Section 17.1.5: explicit limits with a clear error,
/// never OOM mid-job). 256 MiB is generous for INAA-scale spreadsheets.
pub const MAX_UPLOAD_BYTES: u64 = 256 * 1024 * 1024;

/// Format allowlist (Section 7.1): CSV/TSV import, generated XLSX import;
/// everything else is rejected at upload rather than failing later.
const ALLOWED_EXTENSIONS: [&str; 3] = ["csv", "tsv", "xlsx"];

/// Parse states reported in [`StagedFile::parse_state`].
pub const PARSE_PARSED: &str = "parsed";
pub const PARSE_FAILED: &str = "parse_failed";
pub const PARSE_DEFERRED: &str = "deferred";

fn io_err(e: std::io::Error) -> ImportError {
    ImportError::Io(e.to_string())
}

/// Internal quarantine record; serialized next to the staged bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct QuarantineRecord {
    file_id: String,
    path: String,
    size_bytes: u64,
    sha256: String,
    format: String,
    parse_state: String,
    parse_error: Option<String>,
    deleted: bool,
}

impl From<&QuarantineRecord> for StagedFile {
    fn from(r: &QuarantineRecord) -> Self {
        StagedFile {
            file_id: r.file_id.clone(),
            path: r.path.clone(),
            size_bytes: r.size_bytes,
            sha256: r.sha256.clone(),
            format: r.format.clone(),
            parse_state: r.parse_state.clone(),
            parse_error: r.parse_error.clone(),
            deleted: r.deleted,
        }
    }
}

/// Lexically contains `rel` under `root`: rejects absolute paths, `..`, and
/// root/prefix components (Section 6.3 path-containment rule).
fn resolve_contained(root: &std::path::Path, rel: &str) -> Result<PathBuf, ImportError> {
    let mut out = root.to_path_buf();
    for component in std::path::Path::new(rel).components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            other => {
                return Err(ImportError::Parse(format!(
                    "path {rel:?} escapes the project boundary: {other:?}"
                )))
            }
        }
    }
    Ok(out)
}

/// Source-file upload/catalog use cases rooted at one local project
/// directory. Shares the `.archaeodash` project area with the transaction
/// journal; candidate scanning skips dot-directories, so quarantined bytes
/// never appear as group candidates.
pub struct SourceFileService {
    root: PathBuf,
    max_upload_bytes: u64,
}

impl SourceFileService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ImportError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        Ok(Self {
            root,
            max_upload_bytes: MAX_UPLOAD_BYTES,
        })
    }

    /// Test seam: shrinks the upload bound so the limit path is testable.
    #[cfg(test)]
    fn with_max_upload_bytes(mut self, max: u64) -> Self {
        self.max_upload_bytes = max;
        self
    }

    fn quarantine_dir(&self) -> PathBuf {
        self.root.join(".archaeodash/quarantine")
    }

    fn trash_dir(&self) -> PathBuf {
        self.quarantine_dir().join("trash")
    }

    fn record_path(&self, file_id: &str) -> Result<PathBuf, ImportError> {
        // The ID names a quarantine file; only well-formed UUIDs are accepted
        // so no client-controlled string reaches the filesystem.
        Uuid::parse_str(file_id)
            .map(|id| self.quarantine_dir().join(format!("{id}.json")))
            .map_err(|_| ImportError::NotFound(format!("unknown file id {file_id:?}")))
    }

    fn read_record(&self, file_id: &str) -> Result<QuarantineRecord, ImportError> {
        let path = self.record_path(file_id)?;
        let text = std::fs::read_to_string(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                ImportError::NotFound(format!("unknown file id {file_id:?}"))
            }
            _ => io_err(e),
        })?;
        serde_json::from_str(&text)
            .map_err(|e| ImportError::Io(format!("quarantine record {file_id:?}: {e}")))
    }

    fn write_record(&self, record: &QuarantineRecord) -> Result<(), ImportError> {
        let path = self.record_path(&record.file_id)?;
        let body = serde_json::to_string_pretty(record)
            .map_err(|e| ImportError::Io(format!("record serialize: {e}")))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, body).map_err(io_err)?;
        std::fs::rename(&tmp, &path).map_err(io_err)?;
        sync_dir(&self.quarantine_dir());
        Ok(())
    }

    /// Stages uploaded bytes in quarantine, parse-checks CSV sources, then
    /// promotes atomically to the requested logical path. Rejects path
    /// escapes, non-allowlisted extensions, oversized payloads, and existing
    /// targets (delete first to replace). Returns the staged-file metadata.
    pub fn upload(&self, path: &str, content: &[u8]) -> Result<StagedFile, ImportError> {
        if content.len() as u64 > self.max_upload_bytes {
            return Err(ImportError::Limit(format!(
                "upload of {} bytes exceeds the {} byte limit",
                content.len(),
                self.max_upload_bytes
            )));
        }
        let target = resolve_contained(&self.root, path)?;
        if target.exists() {
            return Err(ImportError::Parse(format!(
                "target {path:?} already exists; delete it first to replace"
            )));
        }
        let format = target
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .ok_or_else(|| ImportError::Parse(format!("path {path:?} has no extension")))?;
        if !ALLOWED_EXTENSIONS.contains(&format.as_str()) {
            return Err(ImportError::Parse(format!(
                "unsupported source format {format:?}; allowed: {ALLOWED_EXTENSIONS:?}"
            )));
        }

        // Stage into quarantine: write, checksum, parse-check, record — all
        // before any byte reaches the user-selected path.
        std::fs::create_dir_all(self.quarantine_dir()).map_err(io_err)?;
        let file_id = Uuid::now_v7().simple().to_string();
        let staged = self.quarantine_dir().join(format!("{file_id}.bin"));
        std::fs::write(&staged, content).map_err(io_err)?;
        sync_dir(&self.quarantine_dir());

        let sha256 = {
            let mut hasher = Sha256::new();
            hasher.update(content);
            format!("{:x}", hasher.finalize())
        };
        // CSV is the only format with a tested parser in the Rust port so
        // far; TSV/XLSX stay deferred until their Section 7.1 adapters land.
        let (parse_state, parse_error) = if format == "csv" {
            match data_loader(&staged) {
                Ok(_) => (PARSE_PARSED, None),
                Err(e) => (PARSE_FAILED, Some(e.to_string())),
            }
        } else {
            (PARSE_DEFERRED, None)
        };
        let record = QuarantineRecord {
            file_id: file_id.clone(),
            path: path.to_string(),
            size_bytes: content.len() as u64,
            sha256,
            format,
            parse_state: parse_state.to_string(),
            parse_error: parse_error.map(|e| e.to_string()),
            deleted: false,
        };
        self.write_record(&record)?;

        // Promote: atomic rename from quarantine onto the logical path.
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
        std::fs::rename(&staged, &target).map_err(io_err)?;
        sync_dir(&self.root);
        Ok(StagedFile::from(&record))
    }

    /// Metadata for one staged file by ID; `deleted` files stay readable as
    /// tombstones until asynchronous retention cleanup (hosted form).
    pub fn metadata(&self, file_id: &str) -> Result<StagedFile, ImportError> {
        Ok(StagedFile::from(&self.read_record(file_id)?))
    }

    /// Returns the metadata plus the current bytes for `GET /files/{id}/download`.
    pub fn download(&self, file_id: &str) -> Result<FileDownload, ImportError> {
        let record = self.read_record(file_id)?;
        if record.deleted {
            return Err(ImportError::NotFound(format!(
                "file {file_id:?} is deleted"
            )));
        }
        let path = resolve_contained(&self.root, &record.path)?;
        let content = std::fs::read(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                ImportError::NotFound(format!("file bytes missing for {file_id:?}"))
            }
            _ => io_err(e),
        })?;
        Ok(FileDownload {
            metadata: StagedFile::from(&record),
            content,
        })
    }

    /// Soft delete: moves the bytes to quarantine trash and tombstones the
    /// record. Reference and expected-revision checks are hosted-catalog
    /// concerns (Section 10.2); local staged sources have no referents yet.
    pub fn delete(&self, file_id: &str) -> Result<StagedFile, ImportError> {
        let mut record = self.read_record(file_id)?;
        if record.deleted {
            return Err(ImportError::NotFound(format!(
                "file {file_id:?} is already deleted"
            )));
        }
        let target = resolve_contained(&self.root, &record.path)?;
        if target.exists() {
            std::fs::create_dir_all(self.trash_dir()).map_err(io_err)?;
            let trashed = self.trash_dir().join(format!("{}.bin", record.file_id));
            std::fs::rename(&target, &trashed).map_err(io_err)?;
            sync_dir(&self.trash_dir());
        }
        record.deleted = true;
        self.write_record(&record)?;
        Ok(StagedFile::from(&record))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;

    const MINI_CSV: &[u8] = b"anid,Site,as\nA1,Baca,1.5\nA2,Baca,2\n";

    fn service() -> (SourceFileService, tempdir::TempDir) {
        let dir = tempdir::tempdir().expect("tempdir");
        let service = SourceFileService::new(dir.path()).expect("service");
        (service, dir)
    }

    // Minimal tempdir shim so the dev-dependency list stays as-is.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        pub fn tempdir() -> Result<TempDir, std::io::Error> {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "archaeodash-files-test-{}-{}",
                std::process::id(),
                n
            ));
            std::fs::create_dir_all(&dir)?;
            Ok(TempDir(dir))
        }
    }

    #[test]
    fn upload_stages_parses_and_promotes() {
        let (service, dir) = service();
        let staged = service
            .upload("sources/mini.csv", MINI_CSV)
            .expect("upload");
        assert_eq!(staged.path, "sources/mini.csv");
        assert_eq!(staged.format, "csv");
        assert_eq!(staged.parse_state, PARSE_PARSED);
        assert_eq!(staged.parse_error, None);
        assert!(!staged.deleted);
        assert_eq!(staged.size_bytes, MINI_CSV.len() as u64);
        assert!(!staged.sha256.is_empty());
        // Promoted to the logical path; quarantine keeps only the record.
        assert!(dir.path().join("sources/mini.csv").exists());
        assert_eq!(
            std::fs::read(dir.path().join("sources/mini.csv")).expect("bytes"),
            MINI_CSV
        );
        assert!(!dir
            .path()
            .join(".archaeodash/quarantine")
            .join(format!("{}.bin", staged.file_id))
            .exists());

        let meta = service.metadata(&staged.file_id).expect("metadata");
        assert_eq!(meta, staged);
        let download = service.download(&staged.file_id).expect("download");
        assert_eq!(download.content, MINI_CSV);
        assert_eq!(download.metadata, staged);
    }

    #[test]
    fn upload_rejects_escape_existing_target_and_bad_format() {
        let (service, dir) = service();
        let err = service
            .upload("../outside.csv", MINI_CSV)
            .expect_err("escape rejected");
        assert!(matches!(err, ImportError::Parse(m) if m.contains("escapes")));

        service.upload("sources/mini.csv", MINI_CSV).expect("first");
        let err = service
            .upload("sources/mini.csv", MINI_CSV)
            .expect_err("existing target rejected");
        assert!(matches!(err, ImportError::Parse(m) if m.contains("already exists")));

        let err = service
            .upload("sources/thing.parquet", MINI_CSV)
            .expect_err("non-allowlisted format rejected");
        assert!(matches!(err, ImportError::Parse(m) if m.contains("unsupported source format")));
        let err = service
            .upload("sources/noext", MINI_CSV)
            .expect_err("extensionless path rejected");
        assert!(matches!(err, ImportError::Parse(m) if m.contains("no extension")));
        assert!(!dir.path().join("sources/thing.parquet").exists());
        // Nothing rejected reached the filesystem except the first upload.
        assert!(dir.path().join("sources/mini.csv").exists());
    }

    #[test]
    fn upload_enforces_size_bound_and_reports_parse_state() {
        let dir = tempdir::tempdir().expect("tempdir");
        let bounded = SourceFileService::new(dir.path())
            .expect("service")
            .with_max_upload_bytes(8);
        let err = bounded
            .upload("sources/mini.csv", MINI_CSV)
            .expect_err("oversize rejected");
        assert!(matches!(err, ImportError::Limit(_)));
        assert!(!dir.path().join("sources/mini.csv").exists());

        let (service, dir) = service();
        // Bad CSV stages but reports parse_failed with the message
        // (invalid UTF-8 fails the csv string-record reader).
        let staged = service
            .upload("sources/bad.csv", b"anid,Site\nA1,\xff\xfe\n".as_slice())
            .expect("upload succeeds; parse state is data");
        assert_eq!(staged.parse_state, PARSE_FAILED);
        assert!(staged.parse_error.is_some());
        assert!(dir.path().join("sources/bad.csv").exists());

        // XLSX defers parsing until the Section 7.1 adapter lands.
        let staged = service
            .upload("sources/book.xlsx", b"PK\x03\x04")
            .expect("xlsx upload");
        assert_eq!(staged.format, "xlsx");
        assert_eq!(staged.parse_state, PARSE_DEFERRED);
    }

    #[test]
    fn delete_is_soft_and_download_refuses_deleted() {
        let (service, dir) = service();
        let staged = service
            .upload("sources/mini.csv", MINI_CSV)
            .expect("upload");
        let deleted = service.delete(&staged.file_id).expect("delete");
        assert!(deleted.deleted);
        assert!(!dir.path().join("sources/mini.csv").exists());
        let trash: Vec<_> = std::fs::read_dir(dir.path().join(".archaeodash/quarantine/trash"))
            .expect("trash dir")
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(trash.len(), 1, "bytes preserved in trash");

        let meta = service.metadata(&staged.file_id).expect("tombstone");
        assert!(meta.deleted);
        let err = service.download(&staged.file_id).expect_err("download");
        assert!(matches!(err, ImportError::NotFound(_)));
        let err = service.delete(&staged.file_id).expect_err("second delete");
        assert!(matches!(err, ImportError::NotFound(m) if m.contains("already deleted")));
    }

    #[test]
    fn unknown_or_malformed_ids_are_not_found() {
        let (service, _dir) = service();
        let err = service.metadata("not-a-uuid").expect_err("malformed id");
        assert!(matches!(err, ImportError::NotFound(_)));
        let err = service
            .metadata("01900000-0000-7000-8000-00000000000f")
            .expect_err("unknown id");
        assert!(matches!(err, ImportError::NotFound(_)));
        // Path traversal through the ID never reaches the filesystem.
        let err = service.metadata("../../etc/passwd").expect_err("traversal");
        assert!(matches!(err, ImportError::NotFound(_)));
    }
}
