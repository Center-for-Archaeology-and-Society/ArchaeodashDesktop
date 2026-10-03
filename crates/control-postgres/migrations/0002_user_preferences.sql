-- User-scoped hosted preferences (Section 6.5): typed JSON values keyed by
-- an allowlisted key, one row per (user_id, key). No analytical-unit rows.

CREATE TABLE IF NOT EXISTS preferences (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    key TEXT NOT NULL CHECK (key <> ''),
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, key)
);
