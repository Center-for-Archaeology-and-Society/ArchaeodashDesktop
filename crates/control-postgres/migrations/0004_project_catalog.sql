-- Hosted project catalog (Section 6.4/10.2): catalog rows only — project
-- identity, ownership, display name, and soft-delete state. No analytical
-- data: file/object bytes live in the user file store namespace
-- users/<user-id>/projects/<project-id>/ under the configured root; catalog
-- rows reference them by opaque UUIDs.

CREATE TABLE IF NOT EXISTS projects (
    project_id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 200),
    -- Soft delete (Section 17.1 item 10): rows are tombstoned, the file
    -- namespace is purged by the asynchronous retention sweep, and restores
    -- are possible within the published window.
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_projects_user_id ON projects (user_id);
