#!/usr/bin/env bash
# Builds the Linux desktop app bundles (AppImage and .deb) with the Tauri CLI.
# Runs inside the deploy/desktop-linux container (scripts/home2/build-linux.sh)
# from a checkout on a filesystem that allows executables, with installed
# node_modules.
#
# Output: $CARGO_TARGET_DIR/release/bundle/{appimage,deb}/
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

echo "Building the web interface..."
(cd apps/web && node node_modules/vite/bin/vite.js build)

echo "Bundling the desktop app for Linux..."
# linuxdeploy (used for the AppImage) is itself an AppImage; containers have
# no FUSE, so it unpacks itself instead.
export APPIMAGE_EXTRACT_AND_RUN=1
cd apps/desktop/src-tauri
cargo tauri build --bundles appimage,deb \
  --config '{"bundle":{"active":true}}' \
  -- --locked
