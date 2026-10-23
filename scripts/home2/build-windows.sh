#!/usr/bin/env bash
# Syncs the home2 mirror and cross-compiles the Windows desktop app there
# (scripts/build-windows-exe.sh, cargo-xwin). The Microsoft CRT/SDK license
# for cargo-xwin was accepted when the toolchain was set up (2026-10-22).
# Takes no arguments. Install the result with install-windows.sh.
#
# Usage: bash scripts/home2/build-windows.sh
set -euo pipefail
. "$(dirname "$0")/common.sh"
[ "$#" -eq 0 ] || die "usage: bash scripts/home2/build-windows.sh"

run_remote "set -eo pipefail
mkdir -p $HOME2_MIRROR
$SYNC_SNIPPET
cd $HOME2_MIRROR
XWIN_ACCEPT_LICENSE=1 CARGO_TARGET_DIR=$HOME2_TARGET bash scripts/build-windows-exe.sh 2>&1 \
  | grep -E 'built in|error|warning: unused|Finished|Built ' | tail -12
ls -la --time-style=+%Y-%m-%dT%H:%M $HOME2_TARGET/x86_64-pc-windows-msvc/release/archaeodash-desktop.exe"
