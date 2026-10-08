-- Hosted transformation-definition catalog (Sections 6.4/6.5/10.2): named
-- definitions are configuration only — column selections, method parameters,
-- and seeds — never calculated values (Section 5 storage invariant). The
-- definition JSON lives in the user file-store namespace under opaque keys
-- (users/<user-id>/projects/<project-id>/transformations/<transformation-id>/<revision>.json);
-- this row is the authorization, listing, and current-revision catalog.
-- Small summary fields are copied here so listing never reads objects
-- (Section 6.5: PostgreSQL may hold small summary fields for listing).

CREATE TABLE IF NOT EXISTS transformations (
    transformation_id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects (project_id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (name <> ''),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    object_key TEXT NOT NULL UNIQUE,
    sha256 TEXT NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    bytes BIGINT NOT NULL CHECK (bytes >= 0),
    transform_method TEXT NOT NULL,
    imputation_method TEXT NOT NULL,
    ratio_count INTEGER NOT NULL CHECK (ratio_count >= 0),
    -- Ordered input group checksums for the future hosted apply route;
    -- empty until definitions reference hosted group files.
    input_checksums JSONB NOT NULL DEFAULT '[]'::jsonb,
    deleted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Unique active definition name per project; tombstoned rows free the name
-- (same lifecycle as files.logical_path).
CREATE UNIQUE INDEX IF NOT EXISTS idx_transformations_active_name
    ON transformations (project_id, name) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_transformations_project
    ON transformations (project_id);
