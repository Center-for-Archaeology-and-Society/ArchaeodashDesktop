#!/usr/bin/env bash
# Copies the repository (on the share) into home2's local build mirror, where
# native build tools can run. Takes no arguments.
#
# Usage: bash scripts/home2/sync.sh
set -euo pipefail
. "$(dirname "$0")/common.sh"
[ "$#" -eq 0 ] || die "usage: bash scripts/home2/sync.sh"

run_remote "set -e
mkdir -p $HOME2_MIRROR
$SYNC_SNIPPET
echo \"synced to $HOME2_MIRROR\""
