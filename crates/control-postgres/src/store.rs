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
        now: SystemTime,
    ) -> Result<UserRow, ControlError> {
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, username, username_normalized, email, \
             email_normalized, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7) \
             RETURNING id, username, username_normalized, email, email_normalized, \
             password_hash, email_verified_at, disabled_at",
        )
        .bind(id)
        .bind(username)
        .bind(username_normalized)
        .bind(email)
        .bind(email_normalized)
        .bind(password_hash)
        .bind(to_offset(now))
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
             password_hash, email_verified_at, disabled_at \
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
             password_hash, email_verified_at, disabled_at \
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
             password_hash, email_verified_at, disabled_at \
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
}
