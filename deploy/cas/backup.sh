#!/usr/bin/env bash
# Nightly backup of the ArchaeoDash hosted server on cas (root cron):
#   - a PostgreSQL dump of accounts and projects, kept for 14 days;
#   - a mirror of the user files and project workspaces.
# Both go to the NFS share, off the VM's own disk.
#
# Usage: backup.sh [DESTINATION]   (default /mnt/storage/backups/archaeodash)
set -euo pipefail
# The dump holds account emails and password hashes, and the share is open
# to every app on the host: keep everything root-only.
umask 077

DEST="${1:-/mnt/storage/backups/archaeodash}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$DEST/db" "$DEST/user-files"
chmod 700 "$DEST"

# Dump to a temporary name first so a failed run never looks like a backup.
docker exec archaeodash-db-1 pg_dump -U postgres -d archaeodash_control --format=custom \
  > "$DEST/db/control-$STAMP.dump.partial"
mv "$DEST/db/control-$STAMP.dump.partial" "$DEST/db/control-$STAMP.dump"
find "$DEST/db" -name 'control-*.dump' -mtime +14 -delete
find "$DEST/db" -name '*.partial' -delete

# Owners and modes are not copied: root is squashed on the NFS share.
rsync -rlt --delete /srv/archaeodash/user-files/ "$DEST/user-files/"
