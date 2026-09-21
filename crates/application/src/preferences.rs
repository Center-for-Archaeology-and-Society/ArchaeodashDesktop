//! Preference use cases shared by the HTTP and Tauri adapters
//! (Section 10.1 `GET/PUT /preferences`; legacy `R/userPreferences.R`).
//!
//! Desktop Phase-4 form: preferences persist as one typed JSON document at
//! `.archaeodash/preferences.json` under the opened project root, written
//! atomically (temp + rename + fsync of the parent directory). The hosted
//! per-user control-plane table (Section 6.5) replaces this layout in
//! Phase 7; the allowlist and value shapes stay identical.
//!
//! Legacy parity notes: `read_user_preferences_safe` never throws — a
//! missing or corrupt store reads as empty defaults — and
//! `write_user_preference_safe` is an upsert. Only allowlisted keys with
//! per-key shape validation are accepted (Section 10.1).

use std::collections::BTreeMap;
use std::path::PathBuf;

use archaeodash_contracts::{
    GetPreferencesResponse, PreferenceEntry, PreferenceKey, PutPreferenceRequest,
};
use archaeodash_data_io::write_atomic;
use archaeodash_domain::DomainError;

/// Maximum accepted `lastOpenedDataset` length. Legacy table names are
/// capped at 32 characters (`app_table_name_max_len`); the new group names
/// are filesystem paths, so 255 (a common filename-component bound) is the
/// generous ceiling.
const MAX_LAST_OPENED_DATASET_LEN: usize = 255;

/// Theme allowlist: the legacy `shinyWidgets` choices (Section 9.3 keeps
/// Simple/Light/Dark).
const THEME_VALUES: [&str; 3] = ["simple", "light", "dark"];

fn io_err(e: std::io::Error) -> DomainError {
    DomainError::Internal(Box::new(e))
}

fn ser_err(e: serde_json::Error) -> DomainError {
    DomainError::Internal(Box::new(e))
}

/// Preference store rooted at one project directory. Shares the
/// `.archaeodash` project area with the quarantine and transaction journal;
/// candidate scanning and group operations skip dot-directories.
pub struct PreferenceService {
    root: PathBuf,
}

impl PreferenceService {
    /// Creates the service; the project root must exist or be creatable.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, DomainError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(io_err)?;
        Ok(Self { root })
    }

    fn store_path(&self) -> PathBuf {
        self.root.join(".archaeodash/preferences.json")
    }

    /// Reads every stored preference. A missing or corrupt document reads as
    /// empty (legacy `read_user_preferences_safe` never-throw semantics);
    /// unknown keys are dropped rather than erroring so older stores keep
    /// loading as the allowlist evolves.
    pub fn get_all(&self) -> Result<GetPreferencesResponse, DomainError> {
        let path = self.store_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(GetPreferencesResponse::default());
            }
            Err(e) => return Err(io_err(e)),
        };
        // A corrupt document reads as empty (legacy `read_user_preferences_safe`
        // never-throw semantics); the next successful upsert rewrites it.
        let map: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&text).unwrap_or_default();
        let preferences = map
            .into_iter()
            .filter_map(|(name, value)| {
                PreferenceKey::try_from(name.as_str())
                    .ok()
                    .map(|key| PreferenceEntry { key, value })
            })
            .collect();
        Ok(GetPreferencesResponse { preferences })
    }

    /// Upserts one allowlisted preference (legacy upsert semantics, typed).
    /// The value must match the key's documented shape; a wrong shape is a
    /// validation error, never a silent store corruption.
    pub fn set(&self, req: &PutPreferenceRequest) -> Result<(), DomainError> {
        validate_value(req.key, &req.value)?;
        let path = self.store_path();
        // A corrupt document reads as empty here too, so an upsert rewrites
        // it (legacy `write_user_preference_safe` last-resort rewrite;
        // `get_all` applies the same never-throw read semantics).
        let mut map: BTreeMap<String, serde_json::Value> = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(io_err(e)),
        };
        map.insert(req.key.as_str().to_string(), req.value.clone());
        let body = serde_json::to_string_pretty(&map).map_err(ser_err)?;
        write_atomic(&path, body.as_bytes()).map_err(|e| DomainError::Internal(Box::new(e)))?;
        Ok(())
    }
}

/// Per-key value-shape validation (Section 10.1 typed allowlist).
fn validate_value(key: PreferenceKey, value: &serde_json::Value) -> Result<(), DomainError> {
    let shape_error = || {
        DomainError::validation(
            "preference_value",
            format!("preference {} has an invalid value shape", key.as_str()),
        )
    };
    match key {
        PreferenceKey::Theme => {
            let text = value.as_str().ok_or_else(shape_error)?;
            if !THEME_VALUES.contains(&text) {
                return Err(DomainError::validation(
                    "preference_value",
                    format!("theme must be one of {}", THEME_VALUES.join(", ")),
                ));
            }
        }
        PreferenceKey::LastOpenedDataset => {
            let text = value.as_str().ok_or_else(shape_error)?;
            if text.trim().is_empty() || text.len() > MAX_LAST_OPENED_DATASET_LEN {
                return Err(shape_error());
            }
        }
        PreferenceKey::ColumnVisibility => {
            let map = value.as_object().ok_or_else(shape_error)?;
            if map.values().any(|v| !v.is_boolean()) {
                return Err(shape_error());
            }
        }
        PreferenceKey::CompactMode => {
            if !value.is_boolean() {
                return Err(shape_error());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archaeodash_contracts::PreferenceKey;

    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &PathBuf {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let base = std::env::temp_dir().join(format!(
            "archaeodash-pref-test-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&base).unwrap();
        TempDir(base)
    }

    #[test]
    fn missing_store_reads_as_empty_defaults() {
        let dir = tempdir();
        let svc = PreferenceService::new(dir.path()).unwrap();
        assert_eq!(svc.get_all().unwrap().preferences, vec![]);
    }

    #[test]
    fn corrupt_store_reads_as_empty_defaults() {
        let dir = tempdir();
        let svc = PreferenceService::new(dir.path()).unwrap();
        std::fs::create_dir_all(dir.path().join(".archaeodash")).unwrap();
        std::fs::write(dir.path().join(".archaeodash/preferences.json"), "not json").unwrap();
        assert_eq!(svc.get_all().unwrap().preferences, vec![]);
        // Legacy parity: an upsert rewrites the corrupt document instead of
        // failing forever (`write_user_preference_safe` last-resort rewrite).
        svc.set(&PutPreferenceRequest {
            key: PreferenceKey::Theme,
            value: serde_json::json!("dark"),
        })
        .unwrap();
        assert_eq!(svc.get_all().unwrap().preferences.len(), 1);
    }

    #[test]
    fn upsert_creates_then_updates_without_duplicates() {
        let dir = tempdir();
        let svc = PreferenceService::new(dir.path()).unwrap();
        svc.set(&PutPreferenceRequest {
            key: PreferenceKey::Theme,
            value: serde_json::json!("light"),
        })
        .unwrap();
        svc.set(&PutPreferenceRequest {
            key: PreferenceKey::LastOpenedDataset,
            value: serde_json::json!("Baca"),
        })
        .unwrap();
        // Upsert: same key replaces, no duplicate keys.
        svc.set(&PutPreferenceRequest {
            key: PreferenceKey::Theme,
            value: serde_json::json!("dark"),
        })
        .unwrap();
        let all = svc.get_all().unwrap().preferences;
        assert_eq!(all.len(), 2);
        assert!(all.contains(&PreferenceEntry {
            key: PreferenceKey::Theme,
            value: serde_json::json!("dark"),
        }));
        assert!(all.contains(&PreferenceEntry {
            key: PreferenceKey::LastOpenedDataset,
            value: serde_json::json!("Baca"),
        }));
        // The store survives as a JSON document next to the journal.
        assert!(dir.path().join(".archaeodash/preferences.json").exists());
    }

    #[test]
    fn unknown_keys_and_bad_shapes_are_rejected() {
        let dir = tempdir();
        let svc = PreferenceService::new(dir.path()).unwrap();
        // Theme outside the legacy allowlist.
        assert!(svc
            .set(&PutPreferenceRequest {
                key: PreferenceKey::Theme,
                value: serde_json::json!("solarized"),
            })
            .is_err());
        // Theme must be a string.
        assert!(svc
            .set(&PutPreferenceRequest {
                key: PreferenceKey::Theme,
                value: serde_json::json!(3),
            })
            .is_err());
        // lastOpenedDataset must be a non-empty bounded string.
        assert!(svc
            .set(&PutPreferenceRequest {
                key: PreferenceKey::LastOpenedDataset,
                value: serde_json::json!("   "),
            })
            .is_err());
        assert!(svc
            .set(&PutPreferenceRequest {
                key: PreferenceKey::LastOpenedDataset,
                value: serde_json::json!("x".repeat(256)),
            })
            .is_err());
        // columnVisibility must be an object of booleans.
        assert!(svc
            .set(&PutPreferenceRequest {
                key: PreferenceKey::ColumnVisibility,
                value: serde_json::json!({"as": "yes"}),
            })
            .is_err());
        // compactMode must be a boolean.
        assert!(svc
            .set(&PutPreferenceRequest {
                key: PreferenceKey::CompactMode,
                value: serde_json::json!("true"),
            })
            .is_err());
        // None of the rejected writes touched the store.
        assert_eq!(svc.get_all().unwrap().preferences, vec![]);
        // Valid shapes still land after rejections.
        svc.set(&PutPreferenceRequest {
            key: PreferenceKey::ColumnVisibility,
            value: serde_json::json!({"as": false, "fe": true}),
        })
        .unwrap();
        assert_eq!(svc.get_all().unwrap().preferences.len(), 1);
    }

    #[test]
    fn unknown_stored_keys_are_dropped_on_read() {
        let dir = tempdir();
        let svc = PreferenceService::new(dir.path()).unwrap();
        std::fs::create_dir_all(dir.path().join(".archaeodash")).unwrap();
        std::fs::write(
            dir.path().join(".archaeodash/preferences.json"),
            r#"{"theme":"dark","legacyField":"keep-going"}"#,
        )
        .unwrap();
        let all = svc.get_all().unwrap().preferences;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].key, PreferenceKey::Theme);
    }
}
