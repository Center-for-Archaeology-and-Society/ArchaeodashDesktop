-- Per-owner storage quotas (Section 6.9): reserve capacity before an
-- upload/rewrite, reconcile against the catalog after completion. Logical
-- bytes are the authoritative accounting basis (sum over live file rows);
-- `reserved_bytes` covers in-flight uploads so concurrent requests cannot
-- overshoot the limit. `policy_revision` lets operators bump limits without
-- migrating again.

CREATE TABLE IF NOT EXISTS storage_quotas (
    user_id UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    logical_bytes BIGINT NOT NULL DEFAULT 0 CHECK (logical_bytes >= 0),
    reserved_bytes BIGINT NOT NULL DEFAULT 0 CHECK (reserved_bytes >= 0),
    file_count BIGINT NOT NULL DEFAULT 0 CHECK (file_count >= 0),
    policy_revision INTEGER NOT NULL DEFAULT 1,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
