#!/usr/bin/env bash
set -uo pipefail
source "$HOME/.cargo/env"
cd ~/ArchaeodashDesktop
echo "=== cargo fmt ==="
cargo fmt
echo "done"
echo "=== cargo clippy -D warnings ==="
cargo clippy --workspace --quiet 2>&1 | grep -E "^(error|warning)" | head -20
echo "clippy done"
echo "=== cargo test ==="
cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked" | head -40
