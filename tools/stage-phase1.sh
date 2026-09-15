#!/usr/bin/env bash
# Commit the Phase 1 monorepo skeleton (and prior Phase 0 artifacts, still uncommitted).
set -euo pipefail
cd ~/ArchaeodashDesktop
rm -f tools/cleanup-session-scripts.sh tools/git-status.sh tools/pnpm-build-full.sh tools/cargo-test-summary.sh tools/phase1-dirs.sh
git add -A
git status --short | head -60
