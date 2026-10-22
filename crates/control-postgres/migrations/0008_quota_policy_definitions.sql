-- Quota policy revision 2 (Section 6.9): transformation-definition bytes
-- count toward the owner's logical byte total alongside uploaded files.
-- Definitions saved before this revision were never charged, so backfill
-- each owner's live definition bytes once and mark the row as revision 2.
-- `file_count` is unchanged: it counts files only.

-- Owners with live definitions but no quota row yet (never uploaded).
INSERT INTO storage_quotas (user_id, logical_bytes, reserved_bytes, file_count, policy_revision)
SELECT DISTINCT p.user_id, 0, 0, 0, 1
FROM transformations t
JOIN projects p ON p.project_id = t.project_id
WHERE t.deleted_at IS NULL
ON CONFLICT (user_id) DO NOTHING;

-- Charge live definition bytes once. The revision guard keeps the backfill
-- from double-counting if this statement were ever replayed.
UPDATE storage_quotas q
SET logical_bytes = q.logical_bytes + s.total_bytes,
    policy_revision = 2,
    updated_at = now()
FROM (
    SELECT p.user_id, SUM(t.bytes)::BIGINT AS total_bytes
    FROM transformations t
    JOIN projects p ON p.project_id = t.project_id
    WHERE t.deleted_at IS NULL
    GROUP BY p.user_id
) s
WHERE q.user_id = s.user_id AND q.policy_revision < 2;

-- Every remaining row (no definitions) is already correct under revision 2.
UPDATE storage_quotas SET policy_revision = 2 WHERE policy_revision < 2;
ALTER TABLE storage_quotas ALTER COLUMN policy_revision SET DEFAULT 2;
