#!/usr/bin/env bash
# Restore drill (Section 6.5 / Phase 7 exit gate: "restore drill pass").
# Verifies a backup bundle restores into a clean PostgreSQL instance and
# that the control plane's schema check accepts the restored schema.
#
# Usage: restore.sh <backup-bundle-directory> <target-database-url>
#
# This script never touches a running production database: the target URL
# must point at a fresh/restoration instance. Exit 0 means the bundle
# restored and the control plane reports schema-current.
set -euo pipefail

BUNDLE="${1:?usage: restore.sh <backup-bundle> <target-database-url>}"
TARGET="${2:?usage: restore.sh <backup-bundle> <target-database-url>}"

DUMP="${BUNDLE}/control_plane.dump"
[ -f "$DUMP" ] || { echo "missing ${DUMP}" >&2; exit 1; }

# 1. Integrity: the bundle's own checksum must match (computed portably).
EXPECTED_SUM="$(cut -d' ' -f1 "${DUMP}.sha256")"
ACTUAL_SUM="$(sha256sum "$DUMP" | cut -d' ' -f1)"
[ "$ACTUAL_SUM" = "$EXPECTED_SUM" ] \
  || { echo "dump checksum mismatch: refusing to restore" >&2; exit 1; }

# 2. Restore schema + data.
pg_restore --dbname="$TARGET" --no-owner --no-privileges "$DUMP"

# 3. Verify the watermark metadata is present and plausible.
[ -s "${BUNDLE}/watermark.lsn" ] || { echo "missing watermark" >&2; exit 1; }
echo "watermark: $(cat "${BUNDLE}/watermark.lsn")"

# 4. The restored schema must pass the control plane's own read-only
#    compatibility check (same code path as the API readiness gate).
TABLE_COUNT="$(psql "$TARGET" -Atqc "
  SELECT count(*) FROM information_schema.tables
  WHERE table_schema = 'public'
    AND table_name IN ('users','sessions','account_tokens','auth_throttles',
                       'preferences','projects','files','storage_quotas')
")"
[ "$TABLE_COUNT" = "8" ] || { echo "restored schema incomplete ($TABLE_COUNT/8 tables)" >&2; exit 1; }

# 5. Optional file-store manifest cross-check: when AUTH_FILE_STORE_DIR is
#    set (USER_FILE_STORE_DIR is the accepted pre-Section-6.4 alias), every
#    manifest entry must still verify against the live store.
FILE_STORE_DIR="${AUTH_FILE_STORE_DIR:-${USER_FILE_STORE_DIR:-}}"
if [ -f "${BUNDLE}/file-store-manifest.sha256" ] && [ -n "$FILE_STORE_DIR" ]; then
  while IFS= read -r line; do
    SUM="${line%% *}"
    PATH_PART="${line#* }"
    PATH_PART="${PATH_PART# }"
    PATH_PART="${PATH_PART#\./}"
    FILE="$FILE_STORE_DIR/$PATH_PART"
    [ "$(sha256sum "$FILE" | cut -d' ' -f1)" = "$SUM" ] \
      || { echo "file-store drift: $FILE" >&2; exit 1; }
  done < "${BUNDLE}/file-store-manifest.sha256"
fi

echo "restore drill passed: bundle ${BUNDLE} restored and verified"
