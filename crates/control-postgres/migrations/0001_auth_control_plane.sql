-- Phase 7 auth control-plane schema (Section 6.5, Section 11.1).
-- Control-plane state only: identity, sessions, one-time tokens, throttles.
-- No analytical-unit rows are ever stored here.

CREATE TABLE users (
    id UUID PRIMARY KEY,
    username TEXT NOT NULL CHECK (char_length(username) BETWEEN 3 AND 40),
    username_normalized TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    email_normalized TEXT NOT NULL UNIQUE,
    -- PHC string format (Argon2id); legacy libsodium hashes are rehashed on
    -- successful login during the Phase 8 migration and never inserted new.
    password_hash TEXT NOT NULL,
    email_verified_at TIMESTAMPTZ,
    disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- SHA-256 digest of the opaque presentation token; the raw token is
    -- never stored (Section 11.1).
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    device_label TEXT
);

CREATE INDEX idx_sessions_user_id ON sessions (user_id);

CREATE TABLE account_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 'verify_email' or 'password_reset' (Section 10 auth endpoints).
    kind TEXT NOT NULL CHECK (kind IN ('verify_email', 'password_reset')),
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    -- Single use: consume sets used_at atomically; used/expired/revoked
    -- tokens can never be presented again.
    used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);

CREATE INDEX idx_account_tokens_user_kind ON account_tokens (user_id, kind);

-- Privacy-minimized throttle counters (Section 11.1): `key` is the
-- HMAC-SHA256 digest of category || identifier under the server pepper;
-- raw emails, usernames, and IP addresses are never stored. One row per
-- fixed-window bucket; the upsert in the store is atomic across replicas.
CREATE TABLE auth_throttles (
    key BYTEA PRIMARY KEY CHECK (octet_length(key) = 32),
    window_start TIMESTAMPTZ NOT NULL,
    count BIGINT NOT NULL DEFAULT 0 CHECK (count >= 0)
);
