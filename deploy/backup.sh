#!/usr/bin/env bash
# Coordinated hosted backup (Section 6.5): one PostgreSQL dump of the control
# plane plus a watermark file recording the dump's position relative to the
# user-file store, so a restore can match control-plane pointers to file
# versions. Object-store versioning alone is not a backup.
#
# Usage: backup.sh <output-directory>
# Environment: DATABASE_URL (required), USER_FILE_STORE_DIR (optional; when
# set, a file-store manifest with per-object SHA-256 is recorded alongside).
#
# The dump and manifest are written to a staging directory and renamed into
# place atomically (single `mv` of the finished bundle directory), so a
# crash mid-backup never leaves a half-written backup in the rotation.
set -euo pipefail

OUT_ROOT="${1:?usage: backup.sh <output-directory>}"
: "${DATABASE_URL:?DATABASE_URL is required}"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
STAGING="$(mktemp -d "${OUT_ROOT}/.staging-${STAMP}.XXXXXX")"
BUNDLE="${OUT_ROOT}/backup-${STAMP}"
cleanup() { rm -rf "$STAGING"; }
trap cleanup EXIT

# 1. Control-plane dump: schema + data, consistent single snapshot.
pg_dump --format=custom --file="${STAGING}/control_plane.dump" "$DATABASE_URL"

# 2. Watermark: the replication LSN at dump time plus wall-clock metadata.
#    A restore validates file-store objects against this position.
psql "$DATABASE_URL" -Atqc "SELECT pg_current_wal_lsn()" > "${STAGING}/watermark.lsn"
{
  echo "stamp=${STAMP}"
  echo "lsn=$(cat "${STAGING}/watermark.lsn")"
  echo "database=$(psql "$DATABASE_URL" -Atqc 'SELECT current_database()')"
} > "${STAGING}/backup.meta"

# 3. User-file store manifest (names + SHA-256 + mtimes only; file contents
#    are backed up by the storage layer's own volume/snapshot mechanism —
#    this manifest is what lets restore detect drift between the two).
if [ -n "${USER_FILE_STORE_DIR:-}" ]; then
  (cd "$USER_FILE_STORE_DIR" && find . -type f -print0 | sort -z | xargs -0 sha256sum) \
    > "${STAGING}/file-store-manifest.sha256"
fi

# 4. Checksum the dump so restore can detect truncation/corruption.
sha256sum "${STAGING}/control_plane.dump" > "${STAGING}/control_plane.dump.sha256"

mkdir -p "$OUT_ROOT"
mv "$STAGING" "$BUNDLE"
trap - EXIT
echo "backup complete: ${BUNDLE}"

# 5. Retention: keep the newest $KEEP_BACKUPS bundles, delete older ones.
KEEP_BACKUPS="${KEEP_BACKUPS:-30}"
ls -1dt "${OUT_ROOT}"/backup-* 2>/dev/null | tail -n "+$((KEEP_BACKUPS + 1))" | while read -r old; do
  rm -rf "$old"
  echo "pruned old backup: ${old}"
done
