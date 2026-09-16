#!/usr/bin/env bash
set -uo pipefail
source "$HOME/.cargo/env"
cd ~/ArchaeodashDesktop
cargo test --workspace 2>&1 | grep -E "^(error|warning: unused|test .*(ok|FAILED)|test result|---- .* stdout)" | head -60
