#!/usr/bin/env bash
# Developer environment check/setup helpers for ArchaeoDash (WSL/Linux).
set -u
export PATH="$HOME/.local/bin:$PATH"

if ! command -v pnpm >/dev/null 2>&1; then
  corepack enable pnpm --install-directory "$HOME/.local/bin" >/dev/null 2>&1 || true
fi

echo "node=$(node --version 2>&1)"
echo "pnpm=$(pnpm --version 2>&1)"
echo "cargo=$(cargo --version 2>&1)"
echo "rustc=$(rustc --version 2>&1)"
if pkg-config --exists webkit2gtk-4.1; then echo "webkit2gtk-4.1=ok"; else echo "webkit2gtk-4.1=missing"; fi
if pkg-config --exists glib-2.0; then echo "glib-2.0=ok"; else echo "glib-2.0=missing"; fi
echo "nproc=$(nproc)"
