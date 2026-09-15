#!/usr/bin/env bash
# Install pnpm workspace dependencies and run builds/tests.
set -euo pipefail
export PATH="$HOME/.local/bin:$PATH"
cd ~/ArchaeodashDesktop
pnpm install "$@" 2>&1 | tail -12
