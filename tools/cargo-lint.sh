#!/usr/bin/env bash
# Section 15.3 Rust gates: fmt --check and clippy with warnings denied.
set -uo pipefail
source "$HOME/.cargo/env"
cd ~/ArchaeodashDesktop
echo "=== cargo fmt --check ==="
cargo fmt --check 2>&1 | head -40
fmt_status=$?
echo "=== cargo clippy --workspace -- -D warnings ==="
cargo clippy --workspace --quiet -- -D warnings 2>&1 | tail -30
clippy_status=$?
echo "fmt_status=$fmt_status clippy_status=$clippy_status"
exit $((fmt_status + clippy_status))
