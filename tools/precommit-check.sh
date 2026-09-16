#!/usr/bin/env bash
# Remove temporary diagnostic probe and run TS checks.
set -euo pipefail
export PATH="$HOME/.local/bin:$PATH"
cd ~/ArchaeodashDesktop
rm -f crates/data-io/tests/probe_metadata.rs
rmdir crates/data-io/tests 2>/dev/null || true
echo "=== pnpm build ==="
pnpm -r build 2>&1 | grep -cE ": Done"
echo "=== pnpm test ==="
pnpm -r test 2>&1 | grep -E "# (pass|fail)" | sort | uniq -c
