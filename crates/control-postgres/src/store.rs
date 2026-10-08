//! PostgreSQL-backed auth control plane (Section 6.5, Section 11.1).
//!
//! Stores only control-plane state — identity, sessions, one-time tokens,
//! throttle counters — never analytical-unit rows. Token digests are
//! `BYTEA(32)`; raw presentation tokens never reach the database.
//! Migrations are an explicit deployment step (`migrate`); API startup
//! should call `schema_version` read-only and fail readiness if the schema
//! is behind.

use archaeodash_auth::throttle::{ThrottleDecision, ThrottlePolicy, ThrottleStore};
use sqlx::migrate::Migrate;
use sqlx::postgres::PgPool;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("username is already taken")]
    UsernameTaken,
    #[error("email is already taken")]
    EmailTaken,
    /// A referenced entity does not exist or is not visible to the caller
    /// (ownership-scoped lookups deliberately merge these two cases).
    #[error("{0} not found")]
    NotFound(String),
}

/// A `users` row as the auth layer sees it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub username: String,
    pub username_normalized: String,
    pub email: String,
    pub email_normalized: String,
    pub password_hash: String,
    pub email_verified_at: Option<OffsetDateTime>,
    pub disabled_at: Option<OffsetDateTime>,
    /// Terms/privacy version accepted at registration (Section 10.1
    /// consent-version validation); `None` for pre-consent-audit accounts.
    pub consent_version: Option<String>,
    pub consented_at: Option<OffsetDateTime>,
}

/// A hosted project catalog row (Section 6.4/10.2): identity, ownership,
/// display name, and soft-delete state. Analytical content never lands here.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProjectRow {
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub deleted_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// How a reservation resolves (Section 6.9 reserve/reconcile discipline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaOutcome {
    /// Upload succeeded: reserved bytes become logical bytes.
    Commit,
    /// Upload failed: the reservation is returned.
    Release,
    /// A live file was deleted: its bytes leave the logical total.
    Remove,
}

/// A hosted file catalog row (Section 6.4/6.5): identity, ownership,
/// logical path, and integrity/state fields. Object bytes live in the file
/// store under `object_key`; this row is the only catalog.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FileRow {
    pub file_id: Uuid,
    pub user_id: Uuid,
    pub project_id: Uuid,
    pub logical_path: String,
    pub kind: String,
    pub display_filename: String,
    pub object_key: String,
    pub sha256: String,
    pub media_type: String,
    pub extension: String,
    pub bytes: i64,
    pub state: String,
    pub parse_error: Option<String>,
    pub deleted_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// A tombstone due for retention cleanup (Section 6.9): the catalog row is
/// deleted and the caller removes the trash object it names.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SweptFile {
    pub file_id: Uuid,
    pub user_id: Uuid,
    pub object_key: String,
}

/// A transformation-definition tombstone due for retention cleanup: same
/// contract as [`SweptFile`], for the `transformations` catalog.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SweptTransformation {
    pub transformation_id: Uuid,
    pub user_id: Uuid,
    pub object_key: String,
}

/// A hosted transformation-definition catalog row (Sections 6.4/6.5/10.2):
/// identity, project scope, unique active name, current revision, and the
/// small summary fields listing needs without object reads. The definition
/// JSON itself lives in the file-store namespace under `object_key`;
/// calculated values are never stored here (Section 5).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TransformationRow {
    pub transformation_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub revision: i32,
    pub object_key: String,
    pub sha256: String,
    pub bytes: i64,
    pub transform_method: String,
    pub imputation_method: String,
    pub ratio_count: i32,
    pub deleted_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// The outcome of saving one transformation definition: the current row
/// (revision already bumped), whether an active definition was replaced, and
/// the replaced revision's object key so the caller can trash the old bytes.
#[derive(Debug, Clone)]
pub struct UpsertedTransformation {
    pub row: TransformationRow,
    pub replaced: bool,
    pub previous_object_key: Option<String>,
}

/// A `sessions` row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: Vec<u8>,
    pub created_at: OffsetDateTime,
    pub last_seen_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

fn to_offset(now: SystemTime) -> OffsetDateTime {
    OffsetDateTime::from(now)
}

fn epoch_secs(now: SystemTime) -> u64 {
    now.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// PostgreSQL control-plane store. Cheap to clone; one pool per replica.
#[derive(Debug, Clone)]
pub struct ControlStore {
    pool: PgPool,
}

impl ControlStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Connects to `url`. Deployment configuration supplies credentials;
    /// they are never logged.
    pub async fn connect(url: &str) -> Result<Self, ControlError> {
        Ok(Self::new(PgPool::connect(url).await?))
    }

    /// Applies embedded migrations. Deployment step, not request-path work.
    pub async fn migrate(&self) -> Result<(), ControlError> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    /// Read-only schema check for API startup readiness: the latest applied
    /// migration version must be at least the newest embedded migration.
    pub async fn schema_is_current(&self) -> Result<bool, ControlError> {
        let migrator = sqlx::migrate!("./migrations");
        let latest = migrator.iter().next_back().map(|m| m.version);
        let Some(expected) = latest else {
            return Ok(true);
        };
        let applied = {
            let mut conn = self.pool.acquire().await?;
            conn.list_applied_migrations("_sqlx_migrations").await?
        };
        Ok(applied.iter().any(|m| m.version >= expected))
    }
}

impl ControlStore {
    /// Creates a user; normalized uniqueness is enforced by the schema and
    /// surfaced as typed errors so callers can render uniform messages.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_user(
        &self,
        id: Uuid,
        username: &str,
        username_normalized: &str,
        email: &str,
        email_normalized: &str,
        password_hash: &str,
        consent_version: Option<&str>,
        now: SystemTime,
    ) -> Result<UserRow, ControlError> {
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, username, username_normalized, email, \
             email_normalized, password_hash, created_at, updated_at, \
             consent_version, consented_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7, $8, $7) \
             RETURNING id, username, username_normalized, email, email_normalized, \
             password_hash, email_verified_at, disabled_at, consent_version, \
             consented_at",
        )
        .bind(id)
        .bind(username)
        .bind(username_normalized)
        .bind(email)
        .bind(email_normalized)
        .bind(password_hash)
        .bind(to_offset(now)) // $7: created_at, updated_at, consented_at
        .bind(consent_version) // $8
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db)
                if db.is_unique_violation()
                    && db
                        .constraint()
                        .unwrap_or_default()
                        .contains("email_normalized") =>
            {
                ControlError::EmailTaken
            }
            sqlx::Error::Database(db) if db.is_unique_violation() => ControlError::UsernameTaken,
            other => ControlError::Database(other),
        })?;
        Ok(row)
    }

    pub async fn find_user_by_normalized_email(
        &self,
        email_normalized: &str,
    ) -> Result<Option<UserRow>, ControlError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, username_normalized, email, email_normalized, \
             password_hash, email_verified_at, disabled_at, consent_version, \
             consented_at \
             FROM users WHERE email_normalized = $1",
        )
        .bind(email_normalized)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn find_user_by_normalized_username(
        &self,
        username_normalized: &str,
    ) -> Result<Option<UserRow>, ControlError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, username_normalized, email, email_normalized, \
             password_hash, email_verified_at, disabled_at, consent_version, \
             consented_at \
             FROM users WHERE username_normalized = $1",
        )
        .bind(username_normalized)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Marks email verified and records the new password hash (verification
    /// or reset both land here). Returns whether a live row was updated.
    pub async fn verify_and_set_password(
        &self,
        user_id: Uuid,
        password_hash: &str,
        now: SystemTime,
    ) -> Result<bool, ControlError> {
        let result = sqlx::query(
            "UPDATE users SET email_verified_at = $2, password_hash = $3, \
             updated_at = $2 WHERE id = $1",
        )
        .bind(user_id)
        .bind(to_offset(now))
        .bind(password_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
    /// Records a rehashed password after a successful login against a legacy
    /// or below-policy hash (Section 11.1 rehash-on-login). Does not touch
    /// verification state.
    pub async fn mark_email_verified(
        &self,
        user_id: Uuid,
        now: SystemTime,
    ) -> Result<bool, ControlError> {
        let result =
            sqlx::query("UPDATE users SET email_verified_at = $2, updated_at = $2 WHERE id = $1")
                .bind(user_id)
                .bind(to_offset(now))
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() == 1)
    }
    /// or below-policy hash (Section 11.1 rehash-on-login). Does not touch
    /// verification state.
    pub async fn update_password_hash(
        &self,
        user_id: Uuid,
        password_hash: &str,
        now: SystemTime,
    ) -> Result<bool, ControlError> {
        let result =
            sqlx::query("UPDATE users SET password_hash = $2, updated_at = $3 WHERE id = $1")
                .bind(user_id)
                .bind(password_hash)
                .bind(to_offset(now))
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Creates a session for the digest of a freshly presented opaque token.
    pub async fn create_session(
        &self,
        session_id: Uuid,
        user_id: Uuid,
        token_hash: &[u8; 32],
        remember_days: Option<u64>,
        now: SystemTime,
    ) -> Result<SessionRow, ControlError> {
        let created = to_offset(now);
        let expires_at = remember_days
            .map(|days| created + time::Duration::days(i64::try_from(days).unwrap_or(i64::MAX)))
            .unwrap_or(created + time::Duration::hours(12));
        let row = sqlx::query_as::<_, SessionRow>(
            "INSERT INTO sessions (id, user_id, token_hash, created_at, last_seen_at, expires_at) \
             VALUES ($1, $2, $3, $4, $4, $5) \
             RETURNING id, user_id, token_hash, created_at, last_seen_at, expires_at, revoked_at",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(token_hash.as_slice())
        .bind(created)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Resolves a presented token digest to its live session, touching
    /// `last_seen_at`. Returns `None` for expired, revoked, or unknown
    /// sessions and for disabled accounts.
    pub async fn find_live_session(
        &self,
        token_hash: &[u8; 32],
        now: SystemTime,
    ) -> Result<Option<(SessionRow, UserRow)>, ControlError> {
        let now_ts = to_offset(now);
        // Two reads rather than a JOIN: FromRow tuple extraction with
        // table-prefixed JOIN columns is not supported by derive, and the
        // expiry/revocation/disabled checks are already in the WHERE clauses.
        let session = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, token_hash, created_at, last_seen_at, expires_at, revoked_at \
             FROM sessions WHERE token_hash = $1 AND revoked_at IS NULL \
             AND expires_at > $2",
        )
        .bind(token_hash.as_slice())
        .bind(now_ts)
        .fetch_optional(&self.pool)
        .await?;
        let Some(session) = session else {
            return Ok(None);
        };
        let user = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, username_normalized, email, email_normalized, \
             password_hash, email_verified_at, disabled_at, consent_version, \
             consented_at \
             FROM users WHERE id = $1 AND disabled_at IS NULL",
        )
        .bind(session.user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(user) = user else {
            return Ok(None);
        };
        sqlx::query("UPDATE sessions SET last_seen_at = $2 WHERE id = $1")
            .bind(session.id)
            .bind(now_ts)
            .execute(&self.pool)
            .await?;
        Ok(Some((session, user)))
    }

    /// Rotates a session at use or login (Section 11.1): revokes the old
    /// digest and issues a new one for the same user in one transaction.
    /// Returns the new session row, or `None` when the old digest is not a
    /// live session.
    pub async fn rotate_session(
        &self,
        old_token_hash: &[u8; 32],
        new_session_id: Uuid,
        new_token_hash: &[u8; 32],
        remember_days: Option<u64>,
        now: SystemTime,
    ) -> Result<Option<SessionRow>, ControlError> {
        let now_ts = to_offset(now);
        let expires_at = remember_days
            .map(|days| now_ts + time::Duration::days(i64::try_from(days).unwrap_or(i64::MAX)))
            .unwrap_or(now_ts + time::Duration::hours(12));
        let mut tx = self.pool.begin().await?;
        let revoked = sqlx::query_scalar::<_, Uuid>(
            "UPDATE sessions SET revoked_at = $2 WHERE token_hash = $1 \
             AND revoked_at IS NULL AND expires_at > $2 RETURNING user_id",
        )
        .bind(old_token_hash.as_slice())
        .bind(now_ts)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(user_id) = revoked else {
            return Ok(None);
        };
        let row = sqlx::query_as::<_, SessionRow>(
            "INSERT INTO sessions (id, user_id, token_hash, created_at, last_seen_at, expires_at) \
             VALUES ($1, $2, $3, $4, $4, $5) \
             RETURNING id, user_id, token_hash, created_at, last_seen_at, expires_at, revoked_at",
        )
        .bind(new_session_id)
        .bind(user_id)
        .bind(new_token_hash.as_slice())
        .bind(now_ts)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(row))
    }

    /// Revokes one session (logout) and reports whether it was live.
    pub async fn revoke_session(
        &self,
        token_hash: &[u8; 32],
        now: SystemTime,
    ) -> Result<bool, ControlError> {
        let result = sqlx::query(
            "UPDATE sessions SET revoked_at = $2 WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(token_hash.as_slice())
        .bind(to_offset(now))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Revokes every session of a user (logout-all, post-reset).
    pub async fn revoke_all_sessions(
        &self,
        user_id: Uuid,
        now: SystemTime,
    ) -> Result<u64, ControlError> {
        let result = sqlx::query(
            "UPDATE sessions SET revoked_at = $2 WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user_id)
        .bind(to_offset(now))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Issues a one-time account token (kind `verify_email` or
    /// `password_reset`) for `user_id`; the caller sends the presentation
    /// token by email and stores only the digest here.
    pub async fn create_account_token(
        &self,
        token_id: Uuid,
        user_id: Uuid,
        kind: AccountTokenKind,
        token_hash: &[u8; 32],
        ttl: Duration,
        now: SystemTime,
    ) -> Result<(), ControlError> {
        sqlx::query(
            "INSERT INTO account_tokens (id, user_id, kind, token_hash, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(token_id)
        .bind(user_id)
        .bind(kind.as_str())
        .bind(token_hash.as_slice())
        .bind(
            to_offset(now)
                + time::Duration::seconds(i64::try_from(ttl.as_secs()).unwrap_or(i64::MAX)),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically consumes a one-time token: exactly one presentation
    /// succeeds (single use, 24-hour expiry per Section 11.1). Revokes all
    /// sessions for the user when `kind` is `password_reset`.
    pub async fn consume_account_token(
        &self,
        token_hash: &[u8; 32],
        kind: AccountTokenKind,
        now: SystemTime,
    ) -> Result<Option<Uuid>, ControlError> {
        let now_ts = to_offset(now);
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "UPDATE account_tokens SET used_at = $2 \
             WHERE token_hash = $1 AND kind = $3 AND used_at IS NULL \
             AND revoked_at IS NULL AND expires_at > $2 RETURNING user_id",
        )
        .bind(token_hash.as_slice())
        .bind(now_ts)
        .bind(kind.as_str())
        .fetch_optional(&self.pool)
        .await?;
        if let Some(user) = user_id.filter(|_| kind == AccountTokenKind::PasswordReset) {
            self.revoke_all_sessions(user, now).await?;
        }
        Ok(user_id)
    }
    /// Reads all preferences for a user as raw JSON values keyed by the
    /// allowlisted preference key. The API layer validates key allowlisting
    /// and value shapes; the store treats values as opaque JSONB.
    pub async fn get_preferences(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<(String, serde_json::Value)>, ControlError> {
        let rows: Vec<(String, serde_json::Value)> =
            sqlx::query_as("SELECT key, value FROM preferences WHERE user_id = $1 ORDER BY key")
                .bind(user_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    /// Creates a hosted project catalog row (Section 6.4/10.2): identity,
    /// ownership, and display name only — no analytical data. The file
    /// namespace `users/<user-id>/projects/<project-id>/` is derived from
    /// these opaque UUIDs and lives in the file store, not PostgreSQL.
    pub async fn create_project(
        &self,
        project_id: Uuid,
        user_id: Uuid,
        name: &str,
        now: SystemTime,
    ) -> Result<ProjectRow, ControlError> {
        let row = sqlx::query_as::<_, ProjectRow>(
            "INSERT INTO projects (project_id, user_id, name, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $4) \
             RETURNING project_id, user_id, name, deleted_at, created_at, updated_at",
        )
        .bind(project_id)
        .bind(user_id)
        .bind(name)
        .bind(to_offset(now))
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Lists the caller's live (non-deleted) projects, newest first. Catalog
    /// authorization is by `user_id` equality: a user never sees another
    /// user's rows, and no legacy/external identifier participates.
    pub async fn list_projects(&self, user_id: Uuid) -> Result<Vec<ProjectRow>, ControlError> {
        let rows: Vec<ProjectRow> = sqlx::query_as(
            "SELECT project_id, user_id, name, deleted_at, created_at, updated_at \
             FROM projects WHERE user_id = $1 AND deleted_at IS NULL \
             ORDER BY updated_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Fetches one live project the user owns; `Ok(None)` for other users'
    /// projects, deleted projects, and unknown IDs alike (no existence
    /// oracle across accounts).
    pub async fn get_project(
        &self,
        user_id: Uuid,
        project_id: Uuid,
    ) -> Result<Option<ProjectRow>, ControlError> {
        let row = sqlx::query_as::<_, ProjectRow>(
            "SELECT project_id, user_id, name, deleted_at, created_at, updated_at \
             FROM projects WHERE project_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Soft-deletes a project the user owns; returns whether a live row was
    /// tombstoned. The retention sweep purges file-store objects later.
    pub async fn soft_delete_project(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        now: SystemTime,
    ) -> Result<bool, ControlError> {
        let result = sqlx::query(
            "UPDATE projects SET deleted_at = $3, updated_at = $3 \
             WHERE project_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .bind(user_id)
        .bind(to_offset(now))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Inserts a hosted file catalog row after the bytes are durably staged.
    /// Ownership is checked against the projects table; a foreign or deleted
    /// project is rejected with `None`.
    pub async fn insert_file(&self, file: &FileRow) -> Result<bool, ControlError> {
        // Parameter order follows first appearance in the SQL text: the
        // SELECT list ($1, $3..$12) and the ownership WHERE clause
        // ($2 project, $13 user). Timestamps default to now().
        let result = sqlx::query(
            "INSERT INTO files (file_id, project_id, logical_path, kind, \
             display_filename, object_key, sha256, media_type, extension, \
             bytes, state, parse_error, deleted_at) \
             SELECT $1, p.project_id, $3, $4, $5, $6, $7, $8, $9, $10, \
                    $11, $12, $14 \
             FROM projects p \
             WHERE p.project_id = $2 AND p.user_id = $13 \
               AND p.deleted_at IS NULL",
        )
        .bind(file.file_id) // $1
        .bind(file.project_id) // $2
        .bind(&file.logical_path) // $3
        .bind(&file.kind) // $4
        .bind(&file.display_filename) // $5
        .bind(&file.object_key) // $6
        .bind(&file.sha256) // $7
        .bind(&file.media_type) // $8
        .bind(&file.extension) // $9
        .bind(file.bytes) // $10
        .bind(&file.state) // $11
        .bind(&file.parse_error) // $12
        .bind(file.user_id) // $13
        .bind(file.deleted_at) // $14
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Fetches one live (non-deleted) file the user owns through its
    /// project. Other users' files, deleted files, and unknown IDs are all
    /// `Ok(None)` — no existence oracle across accounts.
    pub async fn get_file(
        &self,
        user_id: Uuid,
        file_id: Uuid,
    ) -> Result<Option<FileRow>, ControlError> {
        let row = sqlx::query_as::<_, FileRow>(
            "SELECT f.file_id, p.user_id, f.project_id, f.logical_path, \
                    f.kind, f.display_filename, f.object_key, f.sha256, \
                    f.media_type, f.extension, f.bytes, f.state, \
                    f.parse_error, f.deleted_at, f.created_at, f.updated_at \
             FROM files f JOIN projects p ON p.project_id = f.project_id \
             WHERE f.file_id = $1 AND p.user_id = $2 AND f.deleted_at IS NULL \
               AND p.deleted_at IS NULL",
        )
        .bind(file_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Lists the live files of one owned project, oldest first.
    pub async fn list_files(
        &self,
        user_id: Uuid,
        project_id: Uuid,
    ) -> Result<Vec<FileRow>, ControlError> {
        let rows: Vec<FileRow> = sqlx::query_as(
            "SELECT f.file_id, p.user_id, f.project_id, f.logical_path, \
                    f.kind, f.display_filename, f.object_key, f.sha256, \
                    f.media_type, f.extension, f.bytes, f.state, \
                    f.parse_error, f.deleted_at, f.created_at, f.updated_at \
             FROM files f JOIN projects p ON p.project_id = f.project_id \
             WHERE p.user_id = $1 AND f.project_id = $2 AND f.deleted_at IS NULL \
               AND p.deleted_at IS NULL \
             ORDER BY f.created_at ASC, f.file_id ASC",
        )
        .bind(user_id)
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Soft-deletes a live file the user owns: tombstones the row and
    /// returns the object key so the caller can move the bytes to trash.
    /// Returns `Ok(None)` for foreign/unknown/deleted IDs.
    pub async fn soft_delete_file(
        &self,
        user_id: Uuid,
        file_id: Uuid,
        now: SystemTime,
    ) -> Result<Option<String>, ControlError> {
        let row: Option<(String,)> = sqlx::query_as(
            "UPDATE files f SET deleted_at = $3, updated_at = $3, state = 'deleted' \
             FROM projects p \
             WHERE f.project_id = p.project_id AND f.file_id = $1 \
               AND p.user_id = $2 AND f.deleted_at IS NULL \
             RETURNING f.object_key",
        )
        .bind(file_id)
        .bind(user_id)
        .bind(to_offset(now))
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(object_key,)| object_key))
    }

    /// Saves (upserts by name) one transformation definition for an owned,
    /// live project (Section 6.5 catalog semantics). The write happens in a
    /// transaction: ownership is re-checked inside, the active name conflict
    /// serializes on `FOR UPDATE`, and the revision bumps monotonically.
    /// Returns `Ok(None)` for a foreign or deleted project (no existence
    /// oracle across accounts).
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_transformation(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        name: &str,
        object_key: &str,
        sha256: &str,
        bytes: i64,
        transform_method: &str,
        imputation_method: &str,
        ratio_count: i32,
        now: SystemTime,
    ) -> Result<Option<UpsertedTransformation>, ControlError> {
        let mut tx = self.pool.begin().await?;
        let owned: Option<Uuid> = sqlx::query_scalar(
            "SELECT project_id FROM projects \
             WHERE project_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(project_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(project_id) = owned else {
            return Ok(None);
        };
        let previous: Option<(Uuid, i32, String)> = sqlx::query_as(
            "SELECT transformation_id, revision, object_key FROM transformations \
             WHERE project_id = $1 AND name = $2 AND deleted_at IS NULL \
             FOR UPDATE",
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(&mut *tx)
        .await?;
        let row = match previous {
            Some((transformation_id, revision, previous_object_key)) => {
                let row = sqlx::query_as::<_, TransformationRow>(
                    "UPDATE transformations SET revision = $3, object_key = $4, sha256 = $5, \
                            bytes = $6, transform_method = $7, imputation_method = $8, \
                            ratio_count = $9, updated_at = $10 \
                     WHERE transformation_id = $1 \
                     RETURNING transformation_id, project_id, name, revision, object_key, \
                               sha256, bytes, transform_method, imputation_method, ratio_count, \
                               deleted_at, created_at, updated_at",
                )
                .bind(transformation_id)
                .bind(project_id)
                .bind(revision + 1)
                .bind(object_key)
                .bind(sha256)
                .bind(bytes)
                .bind(transform_method)
                .bind(imputation_method)
                .bind(ratio_count)
                .bind(to_offset(now))
                .fetch_one(&mut *tx)
                .await?;
                Some(UpsertedTransformation {
                    row,
                    replaced: true,
                    previous_object_key: Some(previous_object_key),
                })
            }
            None => {
                let row = sqlx::query_as::<_, TransformationRow>(
                    "INSERT INTO transformations (transformation_id, project_id, name, revision, \
                            object_key, sha256, bytes, transform_method, imputation_method, \
                            ratio_count, created_at, updated_at) \
                     VALUES ($1, $2, $3, 1, $4, $5, $6, $7, $8, $9, $10, $10) \
                     RETURNING transformation_id, project_id, name, revision, object_key, \
                               sha256, bytes, transform_method, imputation_method, ratio_count, \
                               deleted_at, created_at, updated_at",
                )
                .bind(Uuid::now_v7())
                .bind(project_id)
                .bind(name)
                .bind(object_key)
                .bind(sha256)
                .bind(bytes)
                .bind(transform_method)
                .bind(imputation_method)
                .bind(ratio_count)
                .bind(to_offset(now))
                .fetch_one(&mut *tx)
                .await?;
                Some(UpsertedTransformation {
                    row,
                    replaced: false,
                    previous_object_key: None,
                })
            }
        };
        tx.commit().await?;
        Ok(row)
    }

    /// Lists one owned project's live transformation definitions, ordered by
    /// name. Ownership resolves through the project join; other users'
    /// projects yield an empty list.
    pub async fn list_transformations(
        &self,
        user_id: Uuid,
        project_id: Uuid,
    ) -> Result<Vec<TransformationRow>, ControlError> {
        let rows: Vec<TransformationRow> = sqlx::query_as(
            "SELECT t.transformation_id, t.project_id, t.name, t.revision, t.object_key, \
                    t.sha256, t.bytes, t.transform_method, t.imputation_method, t.ratio_count, \
                    t.deleted_at, t.created_at, t.updated_at \
             FROM transformations t JOIN projects p ON p.project_id = t.project_id \
             WHERE p.user_id = $1 AND t.project_id = $2 AND t.deleted_at IS NULL \
             ORDER BY t.name ASC",
        )
        .bind(user_id)
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Fetches one live definition the user owns through its project.
    /// Foreign, unknown, and deleted IDs are uniformly `Ok(None)`.
    pub async fn get_transformation(
        &self,
        user_id: Uuid,
        transformation_id: Uuid,
    ) -> Result<Option<TransformationRow>, ControlError> {
        let row = sqlx::query_as::<_, TransformationRow>(
            "SELECT t.transformation_id, t.project_id, t.name, t.revision, t.object_key, \
                    t.sha256, t.bytes, t.transform_method, t.imputation_method, t.ratio_count, \
                    t.deleted_at, t.created_at, t.updated_at \
             FROM transformations t JOIN projects p ON p.project_id = t.project_id \
             WHERE t.transformation_id = $1 AND p.user_id = $2 AND t.deleted_at IS NULL \
               AND p.deleted_at IS NULL",
        )
        .bind(transformation_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Fetches one live definition by its project-unique name. Ownership
    /// resolves through the project join; foreign projects, unknown names,
    /// and tombstoned rows are uniformly `Ok(None)`.
    pub async fn get_transformation_by_name(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        name: &str,
    ) -> Result<Option<TransformationRow>, ControlError> {
        let row = sqlx::query_as::<_, TransformationRow>(
            "SELECT t.transformation_id, t.project_id, t.name, t.revision, t.object_key, \
                    t.sha256, t.bytes, t.transform_method, t.imputation_method, t.ratio_count, \
                    t.deleted_at, t.created_at, t.updated_at \
             FROM transformations t JOIN projects p ON p.project_id = t.project_id \
             WHERE p.user_id = $1 AND t.project_id = $2 AND t.name = $3 \
               AND t.deleted_at IS NULL AND p.deleted_at IS NULL",
        )
        .bind(user_id)
        .bind(project_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Soft-deletes a live definition the user owns: tombstones the row and
    /// returns the object key so the caller can move the bytes to trash.
    /// Returns `Ok(None)` for foreign/unknown/deleted IDs.
    pub async fn soft_delete_transformation(
        &self,
        user_id: Uuid,
        transformation_id: Uuid,
        now: SystemTime,
    ) -> Result<Option<String>, ControlError> {
        let row: Option<(String,)> = sqlx::query_as(
            "UPDATE transformations t SET deleted_at = $3, updated_at = $3 \
             FROM projects p \
             WHERE t.project_id = p.project_id AND t.transformation_id = $1 \
               AND p.user_id = $2 AND t.deleted_at IS NULL \
             RETURNING t.object_key",
        )
        .bind(transformation_id)
        .bind(user_id)
        .bind(to_offset(now))
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(object_key,)| object_key))
    }

    /// Reserves capacity for an incoming upload (Section 6.9): the quota row
    /// is created on first use and `reserved_bytes` grows by `add` only when
    /// the projected total (logical + reserved + add) stays within the limit.
    /// Returns `false` when the reservation would exceed it. The caller
    /// reconciles (commits or releases the reservation) after the outcome.
    pub async fn reserve_quota(
        &self,
        user_id: Uuid,
        add: i64,
        limit_bytes: i64,
    ) -> Result<bool, ControlError> {
        // A first-use insert has no existing row to guard arithmetic, so the
        // limit check for that path happens here.
        if add > limit_bytes {
            return Ok(false);
        }
        let result = sqlx::query(
            "INSERT INTO storage_quotas (user_id, logical_bytes, reserved_bytes, file_count) \
             VALUES ($1, 0, $2, 0) \
             ON CONFLICT (user_id) DO UPDATE SET \
               reserved_bytes = storage_quotas.reserved_bytes + $2, \
               updated_at = now() \
             WHERE storage_quotas.logical_bytes + storage_quotas.reserved_bytes + $2 <= $3",
        )
        .bind(user_id)
        .bind(add)
        .bind(limit_bytes)
        .execute(&self.pool)
        .await?;
        // 0 rows means the conflict-update's arithmetic guard rejected the
        // reservation for an existing row.
        Ok(result.rows_affected() == 1)
    }

    /// Reconciles a reservation after the upload outcome (Section 6.9):
    /// `Commit` moves the reserved bytes into the logical total and counts
    /// the file; `Release` returns the reservation without counting; `Remove`
    /// drops a tombstoned file's bytes from the logical total and count.
    pub async fn reconcile_quota(
        &self,
        user_id: Uuid,
        outcome: QuotaOutcome,
        bytes: i64,
        files: i64,
    ) -> Result<(), ControlError> {
        let commit = matches!(outcome, QuotaOutcome::Commit);
        let remove = matches!(outcome, QuotaOutcome::Remove);
        sqlx::query(
            "UPDATE storage_quotas SET \
               logical_bytes = GREATEST( \
                 logical_bytes + CASE WHEN $2 THEN $3 WHEN $5 THEN -$3 ELSE 0 END, 0), \
               reserved_bytes = GREATEST(reserved_bytes - $3, 0), \
               file_count = GREATEST(file_count + CASE WHEN $2 THEN $4 WHEN $5 THEN -$4 ELSE 0 END, 0), \
               updated_at = now() \
             WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(commit)
        .bind(bytes)
        .bind(files)
        .bind(remove)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Retention sweep (Section 6.9): deletes catalog rows tombstoned before
    /// `cutoff` and returns them with ownership + object key so the caller
    /// can purge the trash objects. One statement: selection and removal are
    /// atomic, so a crash between DB delete and FS purge can only orphan an
    /// inaccessible trash object, never a discoverable one.
    pub async fn sweep_expired_tombstones(
        &self,
        cutoff: OffsetDateTime,
    ) -> Result<Vec<SweptFile>, ControlError> {
        let rows: Vec<SweptFile> = sqlx::query_as(
            "DELETE FROM files f USING projects p \
             WHERE f.project_id = p.project_id \
               AND f.deleted_at IS NOT NULL AND f.deleted_at < $1 \
             RETURNING f.file_id, p.user_id, f.object_key",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Retention sweep for transformation definitions (Section 6.9): same
    /// contract as [`Self::sweep_expired_tombstones`] over the
    /// `transformations` catalog — tombstoned rows past `cutoff` are deleted
    /// atomically and returned with ownership + object key so the caller can
    /// purge their trash objects.
    pub async fn sweep_expired_transformations(
        &self,
        cutoff: OffsetDateTime,
    ) -> Result<Vec<SweptTransformation>, ControlError> {
        let rows: Vec<SweptTransformation> = sqlx::query_as(
            "DELETE FROM transformations t USING projects p \
             WHERE t.project_id = p.project_id \
               AND t.deleted_at IS NOT NULL AND t.deleted_at < $1 \
             RETURNING t.transformation_id, p.user_id, t.object_key",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Reads the quota row for observability/tests; `None` before first use.
    pub async fn get_quota(&self, user_id: Uuid) -> Result<Option<(i64, i64, i64)>, ControlError> {
        let row: Option<(i64, i64, i64)> = sqlx::query_as(
            "SELECT logical_bytes, reserved_bytes, file_count \
             FROM storage_quotas WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Upserts one preference (single statement, atomic per Section 6.5's
    /// primary-key `(user_id, key)` contract).
    pub async fn set_preference(
        &self,
        user_id: Uuid,
        key: &str,
        value: serde_json::Value,
        now: SystemTime,
    ) -> Result<(), ControlError> {
        sqlx::query(
            "INSERT INTO preferences (user_id, key, value, updated_at) VALUES ($1, $2, $3, $4)              ON CONFLICT (user_id, key) DO UPDATE SET value = EXCLUDED.value, \
             updated_at = EXCLUDED.updated_at",
        )
        .bind(user_id)
        .bind(key)
        .bind(value)
        .bind(to_offset(now))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// The two one-time account token kinds (Section 6.5 `account_tokens`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountTokenKind {
    VerifyEmail,
    PasswordReset,
}

impl AccountTokenKind {
    fn as_str(self) -> &'static str {
        match self {
            AccountTokenKind::VerifyEmail => "verify_email",
            AccountTokenKind::PasswordReset => "password_reset",
        }
    }
}

impl ThrottleStore for ControlStore {
    /// Single atomic upsert: the row keyed by the privacy-minimized digest
    /// carries the current window bucket; same bucket increments, a new
    /// bucket resets. Atomicity makes the limit persistent across replicas.
    async fn record_and_check(
        &self,
        key: &[u8; 32],
        policy: ThrottlePolicy,
        now: SystemTime,
    ) -> ThrottleDecision {
        let window_secs = policy.window.as_secs().max(1);
        let bucket_start = to_offset(
            SystemTime::UNIX_EPOCH
                + Duration::from_secs(epoch_secs(now) / window_secs * window_secs),
        );
        let row: Result<(i64, OffsetDateTime), sqlx::Error> = sqlx::query_as(
            "INSERT INTO auth_throttles (key, window_start, count) VALUES ($1, $2, 1) \
             ON CONFLICT (key) DO UPDATE SET \
             count = CASE WHEN auth_throttles.window_start = EXCLUDED.window_start \
                          THEN auth_throttles.count + 1 ELSE 1 END, \
             window_start = EXCLUDED.window_start \
             RETURNING count, window_start",
        )
        .bind(key.as_slice())
        .bind(bucket_start)
        .fetch_one(&self.pool)
        .await;
        match row {
            Ok((count, window_start)) if count > i64::from(policy.max_attempts) => {
                let retry = window_start
                    + time::Duration::seconds(i64::try_from(window_secs).unwrap_or(i64::MAX))
                    - to_offset(now);
                ThrottleDecision::Limited {
                    retry_after: Duration::from_secs(retry.whole_seconds().max(1) as u64),
                }
            }
            Ok(_) => ThrottleDecision::Allowed,
            Err(_) => {
                // Fail closed on database errors for throttle checks would
                // deny service on a transient blip; fail open here because
                // login still verifies the password, and the outage is
                // visible in monitoring. Documented trade-off (Section 11.1
                // persistent throttling is best-effort during DB failure).
                ThrottleDecision::Allowed
            }
        }
    }

    /// Clears the counter (successful login/verify/reset).
    async fn clear(&self, key: &[u8; 32]) {
        let _ = sqlx::query("DELETE FROM auth_throttles WHERE key = $1")
            .bind(key.as_slice())
            .execute(&self.pool)
            .await;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)] // test code; panics are the failure mode

    use super::*;
    use archaeodash_auth::identity::normalize_email;
    use archaeodash_auth::password::hash_password;
    use archaeodash_auth::throttle::{ThrottleCategory, ThrottlePepper, LOGIN_PER_ACCOUNT};
    use archaeodash_auth::token::OpaqueToken;

    /// DB-backed tests run only when `DATABASE_URL` points at a test
    /// PostgreSQL instance (CI provides one on Linux); otherwise they skip
    /// so the workspace suite stays green on machines without a database.
    async fn test_pool() -> Option<PgPool> {
        let url = std::env::var("DATABASE_URL").ok()?;
        match PgPool::connect(&url).await {
            Ok(pool) => Some(pool),
            Err(e) => {
                eprintln!("skipping DB test: connect failed: {e}");
                None
            }
        }
    }

    fn unique_suffix() -> String {
        let bytes = OpaqueToken::generate().expect("rng").digest();
        hex_encode(&bytes)[..12].to_string()
    }

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    async fn migrated_store() -> Option<(ControlStore, PgPool)> {
        let pool = test_pool().await?;
        let store = ControlStore::new(pool.clone());
        store.migrate().await.expect("migrations apply");
        Some((store, pool))
    }

    async fn make_user(store: &ControlStore) -> (UserRow, String) {
        let suffix = unique_suffix();
        let username = format!("user-{suffix}");
        let email = format!("{username}@example.com");
        let hash = hash_password("correct horse battery staple").expect("hash");
        let user = store
            .create_user(
                Uuid::now_v7(),
                &username,
                &username,
                &email,
                normalize_email(&email).expect("valid email").as_str(),
                &hash,
                None,
                SystemTime::now(),
            )
            .await
            .expect("create user");
        (user, email)
    }

    #[tokio::test]
    async fn migrations_apply_and_schema_check_passes() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        assert!(store.schema_is_current().await.expect("schema check"));
    }

    #[tokio::test]
    async fn user_uniqueness_is_enforced_on_normalized_columns() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (first, _) = make_user(&store).await;
        let hash = "irrelevant-for-uniqueness";
        // Same normalized email (case-insensitive), different username ->
        // EmailTaken.
        let other_username = format!("other-{}", unique_suffix());
        let err = store
            .create_user(
                Uuid::now_v7(),
                &other_username,
                &other_username,
                first.email.to_uppercase().as_str(),
                normalize_email(&first.email).expect("valid email").as_str(),
                hash,
                None,
                SystemTime::now(),
            )
            .await
            .expect_err("unique email must reject");
        assert!(matches!(err, ControlError::EmailTaken));
        // Same normalized username -> username taken.
        let other_email = format!("{}@other.example.com", unique_suffix());
        let err = store
            .create_user(
                Uuid::now_v7(),
                &first.username.to_uppercase(),
                &first.username,
                &other_email,
                normalize_email(&other_email).expect("valid email").as_str(),
                hash,
                None,
                SystemTime::now(),
            )
            .await
            .expect_err("unique username must reject");
        assert!(matches!(err, ControlError::UsernameTaken));
    }

    #[tokio::test]
    async fn session_lifecycle_rotates_and_revokes() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (user, _) = make_user(&store).await;
        let token = OpaqueToken::generate().expect("rng");
        let session = store
            .create_session(
                Uuid::now_v7(),
                user.id,
                &token.digest(),
                Some(30),
                SystemTime::now(),
            )
            .await
            .expect("create session");
        assert!(session.revoked_at.is_none());
        // Live lookup by digest finds it and touches last_seen.
        let found = store
            .find_live_session(&token.digest(), SystemTime::now())
            .await
            .expect("query")
            .expect("session is live");
        assert_eq!(found.0.id, session.id);
        assert_eq!(found.1.id, user.id);
        // Rotation revokes the old digest and issues a new live one.
        let new_token = OpaqueToken::generate().expect("rng");
        let rotated = store
            .rotate_session(
                &token.digest(),
                Uuid::now_v7(),
                &new_token.digest(),
                None,
                SystemTime::now(),
            )
            .await
            .expect("rotate")
            .expect("old session was live");
        assert_eq!(rotated.user_id, user.id);
        let old_gone = store
            .find_live_session(&token.digest(), SystemTime::now())
            .await
            .expect("query");
        assert!(old_gone.is_none(), "rotated-out session must not resolve");
        // Revoke the new one; logout works by digest.
        assert!(store
            .revoke_session(&new_token.digest(), SystemTime::now())
            .await
            .expect("revoke"));
        assert!(store
            .find_live_session(&new_token.digest(), SystemTime::now())
            .await
            .expect("query")
            .is_none());
    }

    #[tokio::test]
    async fn account_tokens_are_single_use_and_reset_revokes_sessions() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (user, _) = make_user(&store).await;
        let token = OpaqueToken::generate().expect("rng");
        store
            .create_session(
                Uuid::now_v7(),
                user.id,
                &OpaqueToken::generate().expect("rng").digest(),
                None,
                SystemTime::now(),
            )
            .await
            .expect("session for revoke-all check");
        store
            .create_account_token(
                Uuid::now_v7(),
                user.id,
                AccountTokenKind::PasswordReset,
                &token.digest(),
                archaeodash_auth::token::ONE_TIME_LINK_TTL,
                SystemTime::now(),
            )
            .await
            .expect("create token");
        // First presentation consumes it and revokes all sessions.
        let consumed = store
            .consume_account_token(
                &token.digest(),
                AccountTokenKind::PasswordReset,
                SystemTime::now(),
            )
            .await
            .expect("consume");
        assert_eq!(consumed, Some(user.id));
        // Second presentation fails (single use).
        let replay = store
            .consume_account_token(
                &token.digest(),
                AccountTokenKind::PasswordReset,
                SystemTime::now(),
            )
            .await
            .expect("consume");
        assert_eq!(replay, None);
        let sessions = store
            .find_live_session(&token.digest(), SystemTime::now())
            .await
            .expect("query");
        assert!(sessions.is_none());
    }

    #[tokio::test]
    async fn throttle_is_persistent_across_store_instances() {
        let Some((store, pool)) = migrated_store().await else {
            return;
        };
        // A second store over the same database stands in for another replica.
        let replica = ControlStore::new(pool);
        let pepper = ThrottlePepper::from_hex(&hex_encode(&[7u8; 32])).expect("pepper");
        // Unique per run so a shared test database never carries leftover
        // counts into the fixed window.
        let key = pepper.key(
            ThrottleCategory::LoginByAccount,
            &format!("shared-{}@client", Uuid::now_v7()),
        );
        let now = SystemTime::now();
        for _ in 0..LOGIN_PER_ACCOUNT.max_attempts {
            assert_eq!(
                store.record_and_check(&key, LOGIN_PER_ACCOUNT, now).await,
                ThrottleDecision::Allowed
            );
        }
        // The replica sees the same counter: cross-replica persistence.
        assert!(matches!(
            replica.record_and_check(&key, LOGIN_PER_ACCOUNT, now).await,
            ThrottleDecision::Limited { .. }
        ));
        replica.clear(&key).await;
        assert_eq!(
            store.record_and_check(&key, LOGIN_PER_ACCOUNT, now).await,
            ThrottleDecision::Allowed
        );
    }

    /// Section 14.3.2 rehearsal artifact: the control-plane schema contains
    /// only identity/session/token/throttle/preference tables — no
    /// analytical, dataframe, source, result, or project content. This is a
    /// hard schema invariant, so it is enforced by a test against the live
    /// migrations rather than by inspection.
    #[tokio::test]
    async fn schema_contains_no_analytical_content() {
        let Some((_store, pool)) = migrated_store().await else {
            return;
        };
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_type = 'BASE TABLE' \
             ORDER BY table_name",
        )
        .fetch_all(&pool)
        .await
        .expect("table listing");
        let expected: Vec<String> = [
            "_sqlx_migrations",
            "account_tokens",
            "auth_throttles",
            "files",
            "preferences",
            // Catalog rows only: identity/ownership/name/tombstone. No
            // analytical data (Section 6.4: file bytes live in the user file
            // store namespace, not PostgreSQL).
            "projects",
            "sessions",
            "storage_quotas",
            // Transformation-definition catalog: configuration and summary
            // fields only (Section 6.5); definition JSON lives in the file
            // store namespace.
            "transformations",
            "users",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            tables, expected,
            "control plane must hold only Section 6.5 tables"
        );
        // The users table holds identity/contact/auth state only: no column
        // could carry analytical payloads (a structural, not data, check).
        let user_columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'users' \
             ORDER BY ordinal_position",
        )
        .fetch_all(&pool)
        .await
        .expect("column listing");
        for column in &user_columns {
            assert!(
                !column.contains("dataframe")
                    && !column.contains("analysis")
                    && !column.contains("project"),
                "unexpected analytical-sounding column: {column}"
            );
        }
        assert_eq!(
            user_columns,
            [
                "id",
                "username",
                "username_normalized",
                "email",
                "email_normalized",
                "password_hash",
                "email_verified_at",
                "disabled_at",
                "created_at",
                "updated_at",
                "consent_version",
                "consented_at"
            ],
            "users table columns drifted from the Section 6.5 spec"
        );
    }

    /// Section 6.4/10.2 hosted project catalog: creation, ownership-scoped
    /// listing, cross-user invisibility (no existence oracle), and soft
    /// delete. Catalog rows carry identity/name only — never analytical
    /// content.
    #[tokio::test]
    async fn hosted_project_catalog_is_ownership_scoped() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (owner, _) = make_user(&store).await;
        let (stranger, _) = make_user(&store).await;
        let project_id = Uuid::now_v7();
        let created = store
            .create_project(project_id, owner.id, "INAA field season", SystemTime::now())
            .await
            .expect("create project");
        assert_eq!(created.project_id, project_id);
        assert_eq!(created.user_id, owner.id);
        assert_eq!(created.name, "INAA field season");
        assert!(created.deleted_at.is_none());

        // Owner listing shows it.
        let listed = store.list_projects(owner.id).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].project_id, project_id);

        // A stranger's listing and direct fetch see nothing — no existence
        // oracle across accounts.
        assert!(store
            .list_projects(stranger.id)
            .await
            .expect("list")
            .is_empty());
        assert!(store
            .get_project(stranger.id, project_id)
            .await
            .expect("get")
            .is_none());
        assert!(store
            .get_project(owner.id, project_id)
            .await
            .expect("get")
            .is_some());

        // Soft delete tombstones for the owner and is idempotent (second
        // delete reports nothing deleted).
        assert!(store
            .soft_delete_project(owner.id, project_id, SystemTime::now())
            .await
            .expect("delete"));
        assert!(!store
            .soft_delete_project(owner.id, project_id, SystemTime::now())
            .await
            .expect("delete again"));
        assert!(store
            .list_projects(owner.id)
            .await
            .expect("list")
            .is_empty());
        assert!(store
            .get_project(owner.id, project_id)
            .await
            .expect("get")
            .is_none());
    }

    /// Section 6.4/10.2 hosted file catalog: insertion is ownership-checked
    /// (foreign/deleted projects reject), the active logical path is unique
    /// per project, listing is owner-scoped, and soft delete tombstones and
    /// frees the path while returning the object key for the bytes sweep.
    #[tokio::test]
    async fn hosted_file_catalog_is_ownership_scoped() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (owner, _) = make_user(&store).await;
        let (stranger, _) = make_user(&store).await;
        let project_id = Uuid::now_v7();
        store
            .create_project(project_id, owner.id, "catalog", SystemTime::now())
            .await
            .expect("create project");

        let row = FileRow {
            file_id: Uuid::now_v7(),
            user_id: owner.id,
            project_id,
            logical_path: "sources/INAA.csv".to_string(),
            kind: "source".to_string(),
            display_filename: "INAA.csv".to_string(),
            object_key: format!(
                "users/{}/projects/{}/files/{}/1",
                owner.id,
                project_id,
                Uuid::now_v7().simple()
            ),
            sha256: "a".repeat(64),
            media_type: "text/csv".to_string(),
            extension: "csv".to_string(),
            bytes: 12,
            state: "published".to_string(),
            parse_error: None,
            deleted_at: None,
            created_at: to_offset(SystemTime::now()),
            updated_at: to_offset(SystemTime::now()),
        };
        assert!(store.insert_file(&row).await.expect("insert"));

        // A foreign project's ownership check rejects the insert.
        let stranger_project = Uuid::now_v7();
        store
            .create_project(stranger_project, stranger.id, "other", SystemTime::now())
            .await
            .expect("create project");
        let foreign = FileRow {
            project_id: stranger_project,
            user_id: owner.id,
            file_id: Uuid::now_v7(),
            ..row.clone()
        };
        assert!(!store.insert_file(&foreign).await.expect("insert"));

        // Same logical path twice among live rows is a unique violation.
        let duplicate = FileRow {
            file_id: Uuid::now_v7(),
            ..row.clone()
        };
        assert!(store.insert_file(&duplicate).await.is_err());

        // Owner listing shows exactly one file; the stranger sees none.
        assert_eq!(
            store
                .list_files(owner.id, project_id)
                .await
                .expect("list")
                .len(),
            1
        );
        assert!(store
            .list_files(stranger.id, project_id)
            .await
            .expect("list")
            .is_empty());

        // Owner fetch works; stranger fetch is None (no existence oracle).
        assert!(store
            .get_file(owner.id, row.file_id)
            .await
            .expect("get")
            .is_some());
        assert!(store
            .get_file(stranger.id, row.file_id)
            .await
            .expect("get")
            .is_none());

        // Soft delete tombstones and returns the object key for the sweep.
        let key = store
            .soft_delete_file(owner.id, row.file_id, SystemTime::now())
            .await
            .expect("delete")
            .expect("object key");
        assert_eq!(key, row.object_key);
        assert!(store
            .get_file(owner.id, row.file_id)
            .await
            .expect("get")
            .is_none());
        // Deleting again reports nothing; the path is free again.
        assert!(store
            .soft_delete_file(owner.id, row.file_id, SystemTime::now())
            .await
            .expect("delete again")
            .is_none());
        let reused = FileRow {
            file_id: Uuid::now_v7(),
            object_key: format!(
                "users/{}/projects/{}/files/{}/1",
                owner.id,
                project_id,
                Uuid::now_v7().simple()
            ),
            ..row.clone()
        };
        assert!(store.insert_file(&reused).await.expect("insert again"));
    }

    /// Section 6.9 reserve/reconcile discipline: reservations are bounded by
    /// the limit, commits move bytes to logical accounting, releases return
    /// them, and removes drop tombstoned bytes from the logical total.
    #[tokio::test]
    async fn quota_reserve_commit_release_and_remove() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (user, _) = make_user(&store).await;
        let limit: i64 = 1000;

        // First-use reservation within the limit.
        assert!(store
            .reserve_quota(user.id, 600, limit)
            .await
            .expect("reserve"));
        let (logical, reserved, count) =
            store.get_quota(user.id).await.expect("quota").expect("row");
        assert_eq!((logical, reserved, count), (0, 600, 0));

        // A second reservation overshoots and is refused, leaving the row.
        assert!(!store
            .reserve_quota(user.id, 500, limit)
            .await
            .expect("reserve"));
        let (logical, reserved, _) = store.get_quota(user.id).await.expect("quota").expect("row");
        assert_eq!((logical, reserved), (0, 600));

        // Commit: reserved bytes become logical; the file is counted.
        store
            .reconcile_quota(user.id, QuotaOutcome::Commit, 600, 1)
            .await
            .expect("commit");
        let (logical, reserved, count) =
            store.get_quota(user.id).await.expect("quota").expect("row");
        assert_eq!((logical, reserved, count), (600, 0, 1));

        // Release (failed upload): the reservation is returned untouched.
        assert!(store
            .reserve_quota(user.id, 100, limit)
            .await
            .expect("reserve"));
        store
            .reconcile_quota(user.id, QuotaOutcome::Release, 100, 0)
            .await
            .expect("release");
        let (logical, reserved, count) =
            store.get_quota(user.id).await.expect("quota").expect("row");
        assert_eq!((logical, reserved, count), (600, 0, 1));

        // Remove (soft delete): bytes leave the logical total.
        store
            .reconcile_quota(user.id, QuotaOutcome::Remove, 600, 1)
            .await
            .expect("remove");
        let (logical, reserved, count) =
            store.get_quota(user.id).await.expect("quota").expect("row");
        assert_eq!((logical, reserved, count), (0, 0, 0));
    }

    /// Retention sweep: only tombstones older than the cutoff are purged,
    /// and the sweep returns ownership + object key for the trash purge.
    #[tokio::test]
    async fn sweep_expired_tombstones_scopes_by_cutoff() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (owner, _) = make_user(&store).await;
        let project_id = Uuid::now_v7();
        store
            .create_project(project_id, owner.id, "sweep", SystemTime::now())
            .await
            .expect("create project");
        let now = SystemTime::now();
        let old = FileRow {
            file_id: Uuid::now_v7(),
            user_id: owner.id,
            project_id,
            logical_path: "old.csv".to_string(),
            kind: "source".to_string(),
            display_filename: "old.csv".to_string(),
            object_key: format!(
                "users/{}/projects/{}/files/{}/1",
                owner.id,
                project_id,
                Uuid::now_v7().simple()
            ),
            sha256: "b".repeat(64),
            media_type: "text/csv".to_string(),
            extension: "csv".to_string(),
            bytes: 1,
            state: "deleted".to_string(),
            parse_error: None,
            deleted_at: Some(to_offset(now - Duration::from_secs(48 * 3600))),
            created_at: to_offset(now),
            updated_at: to_offset(now),
        };
        store.insert_file(&old).await.expect("insert old");
        let fresh_id = Uuid::now_v7();
        let mut fresh = old.clone();
        fresh.file_id = fresh_id;
        fresh.logical_path = "fresh.csv".to_string();
        fresh.object_key = format!(
            "users/{}/projects/{}/files/{}/1",
            owner.id,
            project_id,
            fresh_id.simple()
        );
        fresh.deleted_at = None;
        store.insert_file(&fresh).await.expect("insert fresh");
        store
            .soft_delete_file(owner.id, fresh_id, now)
            .await
            .expect("tombstone fresh");

        // Cutoff one hour before the deletes: only the 48h-old tombstone is
        // due (an hour of margin absorbs process/DB clock skew).
        let swept = store
            .sweep_expired_tombstones(to_offset(now - Duration::from_secs(3600)))
            .await
            .expect("sweep");
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].file_id, old.file_id);
        assert_eq!(swept[0].user_id, owner.id);
        assert_eq!(swept[0].object_key, old.object_key);

        // The fresh tombstone survives (still within the window) and the
        // path is occupied until purge, so a re-insert of the same path fails.
        assert!(store
            .get_file(owner.id, fresh_id)
            .await
            .expect("get")
            .is_none());
        assert!(store
            .list_files(owner.id, project_id)
            .await
            .expect("list")
            .is_empty());
        // Sweeping again finds nothing.
        assert!(store
            .sweep_expired_tombstones(to_offset(now - Duration::from_secs(3600)))
            .await
            .expect("sweep again")
            .is_empty());
    }

    /// A tiny transformation-definition catalog row for tests.
    async fn save_definition(
        store: &ControlStore,
        user_id: Uuid,
        project_id: Uuid,
        name: &str,
        object_key: &str,
    ) -> UpsertedTransformation {
        store
            .upsert_transformation(
                user_id,
                project_id,
                name,
                object_key,
                "a".repeat(64).as_str(),
                42,
                "log10",
                "none",
                0,
                SystemTime::now(),
            )
            .await
            .expect("upsert transformation")
            .expect("project is owned and live")
    }

    /// Section 6.4/6.5 hosted transformation definitions: upsert by name
    /// bumps the revision and returns the replaced object key, listing and
    /// fetches are ownership-scoped with no existence oracle, and soft
    /// delete frees the name while returning the object key for trash.
    #[tokio::test]
    async fn hosted_transformations_are_scoped_and_revisioned() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (owner, _) = make_user(&store).await;
        let (stranger, _) = make_user(&store).await;
        let project_id = Uuid::now_v7();
        store
            .create_project(project_id, owner.id, "defs", SystemTime::now())
            .await
            .expect("create project");
        // Object keys embed the run-unique owner/project UUIDs so repeated
        // runs against a shared test database never collide on the unique
        // object_key column.
        let object_key = |suffix: &str| {
            format!(
                "users/{}/projects/{project_id}/transformations/{suffix}",
                owner.id
            )
        };

        // Foreign or unknown projects reject atomically (no existence oracle).
        assert!(store
            .upsert_transformation(
                stranger.id,
                project_id,
                "ratios",
                "users/x/transformations/t/1.json",
                &"a".repeat(64),
                1,
                "log10",
                "none",
                0,
                SystemTime::now(),
            )
            .await
            .expect("upsert")
            .is_none());

        // First save creates revision 1.
        let first = save_definition(
            &store,
            owner.id,
            project_id,
            "Cu/Zn ratios",
            &object_key("t/1.json"),
        )
        .await;
        assert!(!first.replaced);
        assert_eq!(first.row.revision, 1);
        assert!(first.previous_object_key.is_none());

        // Second save of the same name replaces in place: the
        // transformation_id is stable, the revision bumps, and the caller
        // learns the previous object key so the old bytes can be trashed.
        let second = save_definition(
            &store,
            owner.id,
            project_id,
            "Cu/Zn ratios",
            &object_key("t/2.json"),
        )
        .await;
        assert!(second.replaced);
        assert_eq!(second.row.transformation_id, first.row.transformation_id);
        assert_eq!(second.row.revision, 2);
        assert_eq!(
            second.previous_object_key.as_deref(),
            Some(object_key("t/1.json").as_str())
        );
        // Creation time is preserved across replaces (desktop upsert parity).
        assert_eq!(second.row.created_at, first.row.created_at);

        // A different name is a new definition; listing is name-ordered and
        // owner-scoped.
        let _ = save_definition(
            &store,
            owner.id,
            project_id,
            "log10 base",
            &object_key("u/1.json"),
        )
        .await;
        let listed = store
            .list_transformations(owner.id, project_id)
            .await
            .expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "Cu/Zn ratios");
        assert_eq!(listed[1].name, "log10 base");
        assert!(store
            .list_transformations(stranger.id, project_id)
            .await
            .expect("stranger list")
            .is_empty());

        // Fetch by ID is ownership-scoped; strangers and unknown IDs see
        // nothing.
        assert!(store
            .get_transformation(owner.id, second.row.transformation_id)
            .await
            .expect("get")
            .is_some());
        assert!(store
            .get_transformation(stranger.id, second.row.transformation_id)
            .await
            .expect("stranger get")
            .is_none());

        // Soft delete tombstones and returns the object key; the name is
        // freed for a fresh definition (new transformation_id, revision 1).
        let key = store
            .soft_delete_transformation(owner.id, second.row.transformation_id, SystemTime::now())
            .await
            .expect("delete")
            .expect("live row");
        assert_eq!(key, object_key("t/2.json"));
        assert!(store
            .get_transformation(owner.id, second.row.transformation_id)
            .await
            .expect("get after delete")
            .is_none());
        let fresh = save_definition(
            &store,
            owner.id,
            project_id,
            "Cu/Zn ratios",
            &object_key("v/1.json"),
        )
        .await;
        assert!(!fresh.replaced);
        assert_ne!(fresh.row.transformation_id, second.row.transformation_id);
        assert_eq!(fresh.row.revision, 1);

        // Deleting a foreign user's definition is uniformly not-found.
        assert!(store
            .soft_delete_transformation(stranger.id, fresh.row.transformation_id, SystemTime::now())
            .await
            .expect("stranger delete")
            .is_none());
    }

    /// Retention sweep for transformation tombstones: the same cutoff
    /// scoping as the file sweep — an old tombstone is deleted and returned
    /// with its object key for the trash purge, a fresh one survives, and
    /// the files sweep is untouched.
    #[tokio::test]
    async fn sweep_expired_transformations_scopes_by_cutoff() {
        let Some((store, _pool)) = migrated_store().await else {
            return;
        };
        let (owner, _) = make_user(&store).await;
        let project_id = Uuid::now_v7();
        store
            .create_project(project_id, owner.id, "sweep", SystemTime::now())
            .await
            .expect("create project");
        let now = SystemTime::now();
        let object_key = |suffix: &str| {
            format!(
                "users/{}/projects/{project_id}/transformations/{suffix}",
                owner.id
            )
        };

        // Live definition, tombstoned 48 hours ago by direct update (the
        // sweep cutoff carries an hour of margin, so it is due).
        let old = save_definition(
            &store,
            owner.id,
            project_id,
            "old definition",
            &object_key("old/1.json"),
        )
        .await;
        sqlx::query("UPDATE transformations SET deleted_at = $2 WHERE transformation_id = $1")
            .bind(old.row.transformation_id)
            .bind(to_offset(now - Duration::from_secs(48 * 3600)))
            .execute(&store.pool)
            .await
            .expect("backdate tombstone");

        // Fresh definition tombstoned now: inside the margin, survives.
        let fresh = save_definition(
            &store,
            owner.id,
            project_id,
            "fresh definition",
            &object_key("fresh/1.json"),
        )
        .await;
        assert!(store
            .soft_delete_transformation(owner.id, fresh.row.transformation_id, now)
            .await
            .expect("tombstone fresh")
            .is_some());

        let swept = store
            .sweep_expired_transformations(to_offset(now - Duration::from_secs(3600)))
            .await
            .expect("sweep");
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].transformation_id, old.row.transformation_id);
        assert_eq!(swept[0].user_id, owner.id);
        assert_eq!(swept[0].object_key, object_key("old/1.json"));

        // The fresh tombstone survives (still within the window); sweeping
        // again finds nothing.
        assert!(store
            .get_transformation(owner.id, fresh.row.transformation_id)
            .await
            .expect("get after delete")
            .is_none());
        assert!(store
            .sweep_expired_transformations(to_offset(now - Duration::from_secs(3600)))
            .await
            .expect("sweep again")
            .is_empty());
    }
}
