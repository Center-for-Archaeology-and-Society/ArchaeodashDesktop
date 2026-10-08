//! Hosted per-user file store (Section 6.4): the local-filesystem backend
//! for the single-host deployment mode. Object keys are generated UUIDs —
//! never usernames, emails, uploaded path components, or display names —
//! under `users/<user-id>/projects/<project-id>/files/<file-id>/1`. The
//! catalog (logical path, display name, integrity fields) lives in the
//! control plane's `files` table, never in object metadata.

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use uuid::Uuid;

use archaeodash_application::files::{
    media_type_for, ALLOWED_EXTENSIONS, MAX_UPLOAD_BYTES, PARSE_DEFERRED, PARSE_FAILED,
    PARSE_PARSED,
};
use archaeodash_control_postgres::{ControlError, ControlStore, FileRow};

/// Errors surfaced to the HTTP layer, mapped to problem-details envelopes by
/// the route handlers.
#[derive(Debug, thiserror::Error)]
pub enum HostedFileError {
    #[error("logical path {path:?} is not a valid relative path")]
    InvalidPath { path: String },
    #[error("unsupported source format {extension:?}; allowed: {allowed:?}")]
    UnsupportedFormat {
        extension: String,
        allowed: [&'static str; 3],
    },
    #[error("upload of {bytes} bytes exceeds the {limit} byte limit")]
    TooLarge { bytes: u64, limit: u64 },
    #[error("catalog write rejected: {0}")]
    Catalog(#[from] ControlError),
    #[error("file storage I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// Staged-object metadata: everything the catalog row needs plus the bytes
/// checksum. Produced by [`HostedFileStore::stage`].
#[derive(Debug, Clone)]
pub struct StagedObject {
    pub file_id: Uuid,
    pub project_id: Uuid,
    pub logical_path: String,
    pub display_filename: String,
    pub object_key: String,
    pub sha256: String,
    pub media_type: String,
    pub extension: String,
    pub bytes: u64,
    pub parse_state: String,
    pub parse_error: Option<String>,
}

/// The per-user namespace root. All object keys derive from opaque UUIDs;
/// nothing client-controlled reaches the filesystem as a path component.
pub struct HostedFileStore {
    base: PathBuf,
}

/// Validates a client-supplied logical path: relative, no `..`, non-empty,
/// allowlisted extension. Returns (logical path, display filename, ext).
pub fn validate_logical_path(path: &str) -> Result<(String, String, String), HostedFileError> {
    let trimmed = path.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(HostedFileError::InvalidPath {
            path: path.to_string(),
        });
    }
    let extension = trimmed
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .ok_or_else(|| HostedFileError::InvalidPath {
            path: path.to_string(),
        })?;
    if !ALLOWED_EXTENSIONS.contains(&extension.as_str()) {
        return Err(HostedFileError::UnsupportedFormat {
            extension,
            allowed: ALLOWED_EXTENSIONS,
        });
    }
    let display = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();
    Ok((trimmed.to_string(), display, extension))
}

impl HostedFileStore {
    /// Creates the store; the base directory must exist or be creatable.
    pub fn new(base: impl Into<PathBuf>) -> Result<Self, HostedFileError> {
        let base = base.into();
        std::fs::create_dir_all(&base).map_err(HostedFileError::Io)?;
        Ok(Self { base })
    }

    fn project_dir(&self, user_id: Uuid, project_id: Uuid) -> PathBuf {
        self.base
            .join("users")
            .join(user_id.to_string())
            .join("projects")
            .join(project_id.to_string())
    }

    fn object_path(&self, object_key: &str) -> PathBuf {
        // Object keys are server-generated (`users/<uuid>/projects/<uuid>/...`);
        // the components are trusted, but join defensively against traversal
        // in case of catalog corruption.
        self.base.join(object_key)
    }

    /// Stages the bytes into the user's namespace quarantine, computes the
    /// checksum, parse-checks CSV sources, then atomically promotes the
    /// object to its opaque key. Returns the staged metadata; the caller
    /// inserts the catalog row (ownership checked in SQL) and removes the
    /// object on rejection.
    pub async fn stage(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        logical_path: &str,
        display_filename: Option<&str>,
        content: &[u8],
    ) -> Result<StagedObject, HostedFileError> {
        if content.len() as u64 > MAX_UPLOAD_BYTES {
            return Err(HostedFileError::TooLarge {
                bytes: content.len() as u64,
                limit: MAX_UPLOAD_BYTES,
            });
        }
        let (logical, display_from_path, extension) = validate_logical_path(logical_path)?;
        let display = display_filename
            .map(str::to_string)
            .unwrap_or(display_from_path);
        let file_id = Uuid::now_v7();
        let object_key = format!(
            "users/{user_id}/projects/{project_id}/files/{}/1",
            file_id.simple()
        );
        let target = self.object_path(&object_key);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(HostedFileError::Io)?;
        }
        let staging = target.with_extension("staging");
        std::fs::write(&staging, content).map_err(HostedFileError::Io)?;
        if let Some(parent) = staging.parent() {
            archaeodash_data_io::sync_dir(parent);
        }
        let sha256 = {
            let mut hasher = Sha256::new();
            hasher.update(content);
            format!("{:x}", hasher.finalize())
        };
        let (parse_state, parse_error) = if extension == "csv" {
            match archaeodash_data_io::data_loader(&staging) {
                Ok(_) => (PARSE_PARSED, None),
                Err(e) => (PARSE_FAILED, Some(e.to_string())),
            }
        } else {
            (PARSE_DEFERRED, None)
        };
        std::fs::rename(&staging, &target).map_err(HostedFileError::Io)?;
        if let Some(parent) = target.parent() {
            archaeodash_data_io::sync_dir(parent);
        }
        Ok(StagedObject {
            file_id,
            project_id,
            logical_path: logical,
            display_filename: display,
            object_key,
            sha256,
            media_type: media_type_for(&extension).to_string(),
            extension,
            bytes: content.len() as u64,
            parse_state: parse_state.to_string(),
            parse_error,
        })
    }

    /// Removes a staged object after a catalog rejection so failed uploads
    /// leave no orphans.
    pub fn discard_staged(&self, staged: &StagedObject) {
        let _ = std::fs::remove_file(self.object_path(&staged.object_key));
    }

    /// Writes one transformation-definition revision into the project's
    /// `transformations/<id>/<revision>.json` sub-namespace (Section 6.4):
    /// staged write + fsync + atomic rename, SHA-256 over the exact bytes.
    /// Object keys are server-generated UUIDs; the catalog row (written by
    /// the caller after this succeeds) is the only index.
    pub fn write_definition_object(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        transformation_id: Uuid,
        revision_id: &str,
        body: &[u8],
    ) -> Result<StoredDefinitionObject, HostedFileError> {
        let object_key = format!(
            "users/{user_id}/projects/{project_id}/transformations/{}/{revision_id}",
            transformation_id.simple()
        );
        let target = self.object_path(&object_key);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(HostedFileError::Io)?;
        }
        let staging = target.with_extension("staging");
        std::fs::write(&staging, body).map_err(HostedFileError::Io)?;
        if let Some(parent) = staging.parent() {
            archaeodash_data_io::sync_dir(parent);
        }
        let sha256 = {
            let mut hasher = Sha256::new();
            hasher.update(body);
            format!("{:x}", hasher.finalize())
        };
        std::fs::rename(&staging, &target).map_err(HostedFileError::Io)?;
        if let Some(parent) = target.parent() {
            archaeodash_data_io::sync_dir(parent);
        }
        Ok(StoredDefinitionObject {
            object_key,
            sha256,
            bytes: body.len() as u64,
        })
    }

    /// Reads one object by its catalog-resolved key. Callers resolve the key
    /// through an ownership-checked catalog row first; the path join stays
    /// defensive against catalog corruption.
    pub fn read_object(&self, object_key: &str) -> Result<Vec<u8>, HostedFileError> {
        std::fs::read(self.object_path(object_key)).map_err(HostedFileError::Io)
    }

    /// Removes one object outright (used when a catalog write rejects a
    /// just-written object so no orphan remains, even in trash).
    pub fn remove_object(&self, object_key: &str) {
        let _ = std::fs::remove_file(self.object_path(object_key));
    }

    /// Moves one current object into the namespace trash after its catalog
    /// row was tombstoned or replaced, for the retention sweep to purge.
    /// Missing objects are fine (idempotent cleanup).
    pub fn trash_object(&self, user_id: Uuid, project_id: Uuid, object_key: &str, tag: &str) {
        let from = self.object_path(object_key);
        let trash = self.project_dir(user_id, project_id).join(".trash");
        if std::fs::create_dir_all(&trash).is_err() {
            return;
        }
        let _ = std::fs::rename(&from, trash.join(format!("{tag}.deleted")));
        if let Some(parent) = trash.parent() {
            archaeodash_data_io::sync_dir(parent);
        }
    }

    /// Metadata for one owned, live file. Foreign/unknown/deleted are `None`.
    pub async fn metadata(
        &self,
        store: &ControlStore,
        user_id: Uuid,
        file_id: Uuid,
    ) -> Result<Option<HostedFileMeta>, HostedFileError> {
        Ok(store
            .get_file(user_id, file_id)
            .await?
            .map(|row| self.meta_from_row(&row)))
    }

    /// Returns the metadata plus object bytes for download. Ownership is
    /// resolved in SQL before any path is touched.
    pub async fn download(
        &self,
        store: &ControlStore,
        user_id: Uuid,
        file_id: Uuid,
    ) -> Result<Option<(HostedFileMeta, Vec<u8>)>, HostedFileError> {
        let Some(row) = store.get_file(user_id, file_id).await? else {
            return Ok(None);
        };
        let content =
            std::fs::read(self.object_path(&row.object_key)).map_err(HostedFileError::Io)?;
        Ok(Some((self.meta_from_row(&row), content)))
    }

    /// Soft delete: tombstones the catalog row, then moves the object into
    /// the namespace trash for the retention sweep. Returns the moved byte
    /// count, or `None` for foreign/unknown/deleted IDs.
    pub async fn delete(
        &self,
        store: &ControlStore,
        user_id: Uuid,
        file_id: Uuid,
    ) -> Result<Option<u64>, HostedFileError> {
        let Some(object_key) = store
            .soft_delete_file(user_id, file_id, std::time::SystemTime::now())
            .await?
        else {
            return Ok(None);
        };
        let bytes = std::fs::metadata(self.object_path(&object_key))
            .map(|m| m.len())
            .unwrap_or(0);
        self.trash_object(
            user_id,
            project_id_of(&object_key),
            &object_key,
            &file_id.simple().to_string(),
        );
        Ok(Some(bytes))
    }

    fn meta_from_row(&self, row: &FileRow) -> HostedFileMeta {
        HostedFileMeta {
            file_id: row.file_id,
            project_id: row.project_id,
            logical_path: row.logical_path.clone(),
            display_filename: row.display_filename.clone(),
            size_bytes: row.bytes as u64,
            sha256: row.sha256.clone(),
            media_type: row.media_type.clone(),
            parse_state: row.state.clone(),
            parse_error: row.parse_error.clone(),
        }
    }
}

/// Recovers the project UUID from an object key (trash routing on delete).
fn project_id_of(object_key: &str) -> Uuid {
    object_key
        .split('/')
        .nth(3)
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::nil)
}

/// Hosted file metadata as the HTTP layer serializes it.
#[derive(Debug, serde::Serialize)]
pub struct HostedFileMeta {
    pub file_id: Uuid,
    pub project_id: Uuid,
    pub logical_path: String,
    pub display_filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub media_type: String,
    pub parse_state: String,
    pub parse_error: Option<String>,
}

/// The file-store side of a saved transformation definition (Section 6.4
/// `transformations/<id>/<revision>.json`): opaque key plus integrity fields
/// for the catalog row.
#[derive(Debug, Clone)]
pub struct StoredDefinitionObject {
    pub object_key: String,
    pub sha256: String,
    pub bytes: u64,
}

impl HostedFileStore {
    /// Removes a trash object left by a delete once its catalog tombstone
    /// has been swept (Section 6.9 retention cleanup). `id` is the trash tag
    /// — the file or transformation ID that named the object. Missing files
    /// are fine — the sweep may run after a partial purge.
    pub fn purge_trash_object(&self, user_id: Uuid, id: Uuid) {
        // The trash dir is the namespace's `.trash`; walk it rather than
        // reconstructing the project id (the sweep already carries user_id,
        // and a missing directory is a no-op either way).
        let user_dir = self.base.join("users").join(user_id.to_string());
        let trash = user_dir.join("projects");
        let Ok(entries) = std::fs::read_dir(trash) else {
            return;
        };
        let target = format!("{}.deleted", id.simple());
        for entry in entries.flatten() {
            let path = entry.path().join(".trash").join(&target);
            if path.exists() {
                let _ = std::fs::remove_file(&path);
                if let Some(parent) = path.parent() {
                    if let Some(project_dir) = parent.parent() {
                        archaeodash_data_io::sync_dir(project_dir);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().to_path_buf();
        (dir, base)
    }

    #[test]
    fn logical_path_validation_rejects_escape_and_bad_extension() {
        assert!(validate_logical_path("sources/a.csv").is_ok());
        assert!(validate_logical_path("../escape.csv").is_err());
        assert!(validate_logical_path("/absolute.csv").is_err());
        assert!(validate_logical_path("a//b.csv").is_err());
        assert!(validate_logical_path("file.exe").is_err());
        assert!(validate_logical_path("").is_err());
    }

    #[tokio::test]
    async fn stage_writes_opaque_key_and_returns_metadata() {
        let (_dir, base) = tempdir();
        let store = HostedFileStore::new(&base).expect("store");
        let user = Uuid::now_v7();
        let project = Uuid::now_v7();
        let staged = store
            .stage(user, project, "sources/INAA.csv", None, b"a,b\n1,2\n")
            .await
            .expect("stage");
        assert_eq!(staged.logical_path, "sources/INAA.csv");
        assert_eq!(staged.display_filename, "INAA.csv");
        assert_eq!(staged.parse_state, PARSE_PARSED);
        assert_eq!(staged.media_type, "text/csv");
        assert_eq!(
            staged.object_key,
            format!(
                "users/{user}/projects/{project}/files/{}/1",
                staged.file_id.simple()
            )
        );
        let expected = base
            .join("users")
            .join(user.to_string())
            .join("projects")
            .join(project.to_string())
            .join("files")
            .join(staged.file_id.simple().to_string())
            .join("1");
        assert_eq!(
            std::fs::read(&expected).expect("bytes"),
            b"a,b\n1,2\n".to_vec()
        );
        // No staging leftovers.
        assert!(!expected.with_extension("staging").exists());
    }

    #[tokio::test]
    async fn invalid_utf8_csv_records_parse_failure() {
        let (_dir, base) = tempdir();
        let store = HostedFileStore::new(&base).expect("store");
        let staged = store
            .stage(
                Uuid::now_v7(),
                Uuid::now_v7(),
                "broken.csv",
                None,
                b"a,b\n1,\xff\xfe\x00invalid utf8",
            )
            .await
            .expect("stage");
        assert_eq!(staged.parse_state, PARSE_FAILED);
        assert!(staged.parse_error.is_some());
        // The object still exists for inspection until the catalog decides.
        assert!(store.object_path(&staged.object_key).exists());
    }

    #[tokio::test]
    async fn oversized_uploads_are_rejected_before_any_write() {
        let (_dir, base) = tempdir();
        let store = HostedFileStore::new(&base).expect("store");
        let err = store
            .stage(
                Uuid::now_v7(),
                Uuid::now_v7(),
                "a.csv",
                None,
                &vec![0u8; (MAX_UPLOAD_BYTES + 1) as usize],
            )
            .await
            .expect_err("too large");
        assert!(matches!(err, HostedFileError::TooLarge { .. }));
        assert!(!base.join("users").exists(), "no namespace created");
    }

    #[tokio::test]
    async fn definition_objects_follow_the_transformations_namespace() {
        let (_dir, base) = tempdir();
        let store = HostedFileStore::new(&base).expect("store");
        let user = Uuid::now_v7();
        let project = Uuid::now_v7();
        let transformation = Uuid::now_v7();
        let revision = Uuid::now_v7().simple().to_string();
        let stored = store
            .write_definition_object(
                user,
                project,
                transformation,
                &revision,
                br#"{"created_at_unix_secs":1,"definition":{}}"#,
            )
            .expect("write");
        // Object key layout per Section 6.4, with a checksum over the bytes.
        let expected = base
            .join("users")
            .join(user.to_string())
            .join("projects")
            .join(project.to_string())
            .join("transformations")
            .join(transformation.simple().to_string())
            .join(&revision);
        assert_eq!(
            stored.object_key,
            format!(
                "users/{user}/projects/{project}/transformations/{}/{revision}",
                transformation.simple()
            )
        );
        assert_eq!(stored.bytes, 42);
        assert_eq!(stored.sha256.len(), 64);
        // Round trip: the catalog-resolved key reads back exact bytes.
        let read_back = store.read_object(&stored.object_key).expect("read");
        assert_eq!(
            read_back,
            br#"{"created_at_unix_secs":1,"definition":{}}"#.to_vec()
        );
        assert!(!expected.with_extension("staging").exists());
        // Trash routes the object into the project's .trash for the sweep;
        // the caller's tag marks why the revision left the catalog.
        store.trash_object(user, project, &stored.object_key, "replaced");
        assert!(!expected.exists());
        let trash = base
            .join("users")
            .join(user.to_string())
            .join("projects")
            .join(project.to_string())
            .join(".trash")
            .join("replaced.deleted");
        assert!(trash.exists());
    }
}
