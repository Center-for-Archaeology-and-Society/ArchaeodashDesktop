#!/usr/bin/env bash
# Run pnpm recursive build/typecheck/test for the TS workspace.
set -euo pipefail
export PATH="$HOME/.local/bin:$PATH"
cd ~/ArchaeodashDesktop
echo "=== pnpm -r build ==="
pnpm -r build 2>&1 | tail -25
echo "=== pnpm -r test ==="
pnpm -r test 2>&1 | tail -40
