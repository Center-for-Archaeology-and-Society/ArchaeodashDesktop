#!/usr/bin/env bash
set -uo pipefail
source "$HOME/.cargo/env"
cd ~/ArchaeodashDesktop
cargo clippy --workspace 2>&1 | grep -B1 -A8 "^warning:" | grep -E "warning:|-->" | head -50
