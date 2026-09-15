#!/usr/bin/env bash
# Run workspace tests, showing only summary lines.
set -euo pipefail
source "$HOME/.cargo/env"
cd ~/ArchaeodashDesktop
cargo test --workspace 2>&1 | grep -E "running [0-9]+ test|test result|error\[|^error|warning" || true
