#!/usr/bin/env bash
# Cross-compiles the Windows desktop app (x86_64-pc-windows-msvc) on Linux.
#
# One-time setup on the Linux host:
#   sudo apt-get install clang lld llvm
#   rustup target add x86_64-pc-windows-msvc
#   cargo install --locked cargo-xwin
# plus Node.js 22+ and installed node_modules (pnpm install).
#
# cargo-xwin downloads Microsoft's C runtime and Windows SDK on first use.
# Set XWIN_ACCEPT_LICENSE=1 to accept Microsoft's license terms for them.
#
# Run from a checkout on a filesystem that allows executables (the web build
# runs native esbuild/rollup binaries).
#
# Output: $CARGO_TARGET_DIR (default ./target)/x86_64-pc-windows-msvc/release/
#         archaeodash-desktop.exe - a single exe; the web UI is embedded and
#         the WebView2 loader is linked statically. Needs the WebView2 runtime
#         (included with Windows 10/11).
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ "${XWIN_ACCEPT_LICENSE:-}" != 1 ]]; then
  echo "Set XWIN_ACCEPT_LICENSE=1 to accept the Microsoft CRT/Windows SDK license used by cargo-xwin." >&2
  exit 1
fi
# Ask cargo, which also finds subcommands in ~/.cargo/bin when it is not on PATH.
cargo xwin --version >/dev/null 2>&1 || {
  echo "cargo-xwin not found: cargo install --locked cargo-xwin" >&2
  exit 1
}

echo "Building the web interface..."
(cd apps/web && node node_modules/vite/bin/vite.js build)

echo "Cross-compiling the desktop app for Windows..."
# tauri/custom-protocol is what `tauri build` enables for release builds:
# the app serves its embedded frontend instead of looking for a dev server.
cargo xwin build --locked --release \
  --target x86_64-pc-windows-msvc \
  -p archaeodash-desktop-app \
  --features tauri/custom-protocol

exe="${CARGO_TARGET_DIR:-target}/x86_64-pc-windows-msvc/release/archaeodash-desktop.exe"
echo "Built $exe"
