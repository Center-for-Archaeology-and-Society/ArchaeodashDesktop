//! ArchaeoDash shared domain model (IMPLEMENTATION.md Section 5).
//!
//! Opaque, sortable identifiers at every boundary; core entities and typed
//! errors. The physical Parquet column `analytical_uuid` is the hidden
//! immutable row identity and is never exposed as a user-facing identifier.

use std::fmt;

macro_rules! opaque_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a new identifier from an opaque string.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the opaque string representation.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque_id!(
    /// Authenticated user identity. Never derived from a table name.
    UserId
);
opaque_id!(
    /// Server session identity.
    SessionId
);
opaque_id!(
    /// Opened directory (desktop) or hosted namespace identity.
    ProjectId
);
opaque_id!(
    /// One logical group of analytical units; stable across revisions.
    GroupId
);
opaque_id!(
    /// Immutable published revision of a [`GroupId`].
    GroupRevisionId
);
opaque_id!(
    /// Named, persisted transformation definition.
    TransformationId
);
opaque_id!(
    /// Ephemeral or explicitly exported analysis result.
    AnalysisResultId
);
opaque_id!(
    /// Long-running job handle with progress/cancellation state.
    JobId
);

/// Hidden immutable row identity for one analytical unit.
///
/// Stored as the `analytical_uuid` Parquet column and rendered as the
/// canonical lowercase hyphenated UUID string in DTOs; never shown as a
/// normal column, picker option, label, or export identifier (Section 5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AnalyticalUuid(uuid::Uuid);

impl AnalyticalUuid {
    /// Generates a new UUIDv7 identity (time-ordered, collision-resistant).
    pub fn generate() -> Self {
        Self(uuid::Uuid::now_v7())
    }

    /// Parses the canonical hyphenated form.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        uuid::Uuid::parse_str(value)
            .map(Self)
            .map_err(|e| DomainError::InvalidIdentity {
                message: format!("invalid analytical_uuid: {e}"),
            })
    }

    /// Canonical lowercase hyphenated rendering.
    pub fn as_hyphenated(&self) -> String {
        self.0.hyphenated().to_string()
    }

    /// Underlying UUID for storage codecs.
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl fmt::Display for AnalyticalUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.hyphenated().to_string())
    }
}

impl serde::Serialize for AnalyticalUuid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.hyphenated().to_string())
    }
}

impl<'de> serde::Deserialize<'de> for AnalyticalUuid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Provenance of one loaded analytical unit, replacing the legacy
/// `currentDatasetRowMap` and temporary `.__source_*` columns (Section 5.1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceRef {
    /// Group file the unit was loaded from.
    pub group_id: GroupId,
    /// Immutable revision of that group file.
    pub group_revision_id: GroupRevisionId,
    /// Project-relative source path of the group file.
    pub source_path: String,
    /// Zero-based row position inside the group file.
    pub source_row: u64,
}

/// Typed domain error. Every failure carries safe user text plus structured
/// diagnostics instead of swallowed `try`/`quietly` errors (Section 3.2).
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    /// An identifier did not parse or failed an ownership check.
    #[error("{message}")]
    InvalidIdentity {
        /// Safe user-facing message.
        message: String,
    },
    /// A value or invariant violation with a machine-readable code.
    #[error("{code}: {message}")]
    Validation {
        /// Stable machine-readable error code.
        code: String,
        /// Safe user-facing message.
        message: String,
    },
    /// The requested entity does not exist or is not visible to the caller.
    #[error("not found: {0}")]
    NotFound(String),
    /// Storage, IO, or serialization failure; details stay in diagnostics.
    #[error("internal error")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl DomainError {
    /// Convenience constructor for [`DomainError::Validation`].
    pub fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Convenient result alias across the workspace.
pub type DomainResult<T> = Result<T, DomainError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytical_uuid_round_trips_canonical_form() {
        let id = AnalyticalUuid::generate();
        let text = id.as_hyphenated();
        assert_eq!(AnalyticalUuid::parse(&text).unwrap(), id);
        assert_eq!(text.to_string(), id.to_string());
        assert!(uuid::Uuid::parse_str(&text).unwrap().get_version_num() >= 7);
    }

    #[test]
    fn opaque_ids_are_opaque_and_sortable() {
        let mut ids = vec![GroupId::new("b"), GroupId::new("a")];
        ids.sort();
        assert_eq!(ids, vec![GroupId::new("a"), GroupId::new("b")]);
        assert_eq!(GroupId::new("g1").as_str(), "g1");
    }

    #[test]
    fn validation_error_has_code_and_safe_message() {
        let err = DomainError::validation("group_profile", "missing profile metadata");
        assert!(err.to_string().starts_with("group_profile:"));
    }

    #[test]
    fn source_ref_serializes_stably() {
        let r = SourceRef {
            group_id: GroupId::new("grp"),
            group_revision_id: GroupRevisionId::new("rev"),
            source_path: "groups/grp.parquet".into(),
            source_row: 7,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["source_row"], serde_json::json!(7));
    }
}
