-- Hosted file catalog (Section 6.4/6.5/10.2): catalog rows only. Object
-- bytes live in the user file-store namespace under opaque keys
-- (users/<user-id>/projects/<project-id>/files/<file-id>/<version>); logical
-- paths, display names, and integrity fields live here. No analytical data.

CREATE TABLE IF NOT EXISTS files (
    file_id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects (project_id) ON DELETE CASCADE,
    logical_path TEXT NOT NULL CHECK (logical_path <> '' AND logical_path NOT LIKE '..%'),
    kind TEXT NOT NULL CHECK (kind IN ('group', 'source', 'export', 'internal')),
    display_filename TEXT NOT NULL,
    object_key TEXT NOT NULL UNIQUE,
    sha256 TEXT NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    media_type TEXT NOT NULL,
    extension TEXT NOT NULL,
    bytes BIGINT NOT NULL CHECK (bytes >= 0),
    state TEXT NOT NULL CHECK (state IN ('staged', 'published', 'deleted')),
    parse_error TEXT,
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Unique active logical path per project; tombstoned rows free the name.
CREATE UNIQUE INDEX IF NOT EXISTS idx_files_active_path
    ON files (project_id, logical_path) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_files_project ON files (project_id);
