#!/usr/bin/env bash
# Commits on home2, where git sees file modes correctly (git on Windows
# misreads them through the share).
#
# Usage: bash scripts/home2/commit.sh MESSAGE_FILE PATH...
#   MESSAGE_FILE  local file holding the full commit message (any path
#                 readable here, e.g. a scratch file); at most 64 KiB
#   PATH          repository-relative files or directories to stage; each
#                 must exist or be a tracked file being deleted
#
# Before staging, every changed file under the given paths gets the mode git
# already records for it (new files: 755 for scripts starting with "#!",
# else 644), undoing the executable bits the Samba share adds. Commits as the
# author of the previous commit. Never pushes.
set -euo pipefail
. "$(dirname "$0")/common.sh"
usage="usage: bash scripts/home2/commit.sh MESSAGE_FILE PATH..."
[ "$#" -ge 2 ] || die "$usage"
message_file="$1"
shift
[ -f "$message_file" ] || die "message file not found: $message_file"
size=$(wc -c < "$message_file")
[ "$size" -gt 0 ] && [ "$size" -le 65536 ] || die "message file must be 1 byte to 64 KiB"
for p in "$@"; do
  case "$p" in
    -*|/*|*\\*) die "invalid path '$p' (repository-relative paths only)" ;;
  esac
  case "/$p/" in
    */../*|*/./*) die "invalid path '$p' (no . or .. components)" ;;
  esac
done
message_b64="$(base64 -w0 < "$message_file")"

run_remote '
set -euo pipefail
message_b64="$1"; shift
cd '"$HOME2_REPO"'
g() { git -c safe.directory="*" "$@"; }
for p in "$@"; do
  if [ ! -e "$p" ] && ! g ls-files --error-unmatch -- "$p" >/dev/null 2>&1; then
    echo "path does not exist and is not tracked: $p" >&2; exit 2
  fi
done
# Restore recorded file modes on everything about to be staged.
while IFS= read -r -d "" entry; do
  file="${entry:3}"
  [ -f "$file" ] && [ ! -L "$file" ] || continue
  recorded=$(g ls-tree HEAD -- "$file" | cut -c1-6)
  if [ "$recorded" = 100755 ]; then chmod 755 "$file"
  elif [ -n "$recorded" ]; then chmod 644 "$file"
  elif head -c2 "$file" | grep -q "^#!"; then chmod 755 "$file"
  else chmod 644 "$file"; fi
done < <(g status --porcelain -z -uall -- "$@")
g add -- "$@"
if g diff --cached --quiet; then echo "nothing to commit for the given paths" >&2; exit 1; fi
author_name=$(g log -1 --format=%an)
author_email=$(g log -1 --format=%ae)
printf "%s" "$message_b64" | base64 -d | \
  GIT_AUTHOR_NAME="$author_name" GIT_AUTHOR_EMAIL="$author_email" \
  GIT_COMMITTER_NAME="$author_name" GIT_COMMITTER_EMAIL="$author_email" \
  g commit -q -F -
g log --stat -1 --format="%h %s" | cat
echo "-- still uncommitted --"
g status --short | head -30
' "$message_b64" "$@"
