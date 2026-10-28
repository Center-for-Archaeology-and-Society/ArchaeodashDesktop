#!/usr/bin/env bash
# Pushes master to origin from home2, where the GitHub SSH key lives (git on
# this Windows PC cannot verify GitHub's host key). Fetches first and pushes
# only a fast-forward: if origin/master has commits this checkout lacks, it
# stops and says so instead of pushing. Never force-pushes. Takes no
# arguments.
#
# Usage: bash scripts/home2/push.sh
set -euo pipefail
. "$(dirname "$0")/common.sh"
[ "$#" -eq 0 ] || die "usage: bash scripts/home2/push.sh"

run_remote '
set -euo pipefail
cd '"$HOME2_REPO"'
g() { git -c safe.directory="*" "$@"; }
branch=$(g rev-parse --abbrev-ref HEAD)
[ "$branch" = master ] || { echo "on branch $branch, not master; nothing pushed" >&2; exit 2; }
timeout 120 git -c safe.directory="*" fetch -q origin
if ! g merge-base --is-ancestor origin/master HEAD; then
  echo "origin/master has commits this checkout lacks; merge them first. Nothing pushed." >&2
  g log --oneline HEAD..origin/master | head -10 >&2
  exit 1
fi
ahead=$(g rev-list --count origin/master..HEAD)
if [ "$ahead" -eq 0 ]; then echo "already in sync with origin/master"; exit 0; fi
timeout 120 git -c safe.directory="*" push -q origin master
echo "pushed $ahead commit(s); origin/master is now $(g rev-parse --short HEAD)"
'
