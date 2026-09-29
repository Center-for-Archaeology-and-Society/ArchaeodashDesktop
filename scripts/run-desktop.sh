#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
cd "$repo_root"

die() { printf 'Desktop launcher: %s\n' "$*" >&2; exit 1; }

command -v node >/dev/null 2>&1 || die 'Node.js 22 or newer is required. Install Node.js, then rerun this script.'
node_major="$(node -p 'Number(process.versions.node.split(".")[0])')"
(( node_major >= 22 )) || die "Node.js 22 or newer is required (found $(node --version))."
command -v cargo >/dev/null 2>&1 || die 'Rust and Cargo are required. Install the stable Rust toolchain, then rerun this script.'

pnpm_version="$(node -p 'JSON.parse(require("fs").readFileSync("package.json", "utf8")).packageManager.replace(/^pnpm@/, "")')"
pnpm_cmd=()
probe_pnpm() {
  local actual
  actual="$("$@" --version 2>/dev/null)" || return 1
  [[ "$actual" == "$pnpm_version" ]] || return 1
  pnpm_cmd=("$@")
}

# PATH shims can be stale or point at a mismatched Corepack cache. Accept only
# a working executable that reports the version pinned by package.json.
if command -v pnpm >/dev/null 2>&1; then
  probe_pnpm pnpm || true
fi
if ((${#pnpm_cmd[@]} == 0)) && command -v corepack >/dev/null 2>&1; then
  probe_pnpm corepack pnpm || true
fi
if ((${#pnpm_cmd[@]} == 0)); then
  node_bin="$(command -v node)"
  adjacent_corepack="$(dirname -- "$node_bin")/corepack"
  if [[ -x "$adjacent_corepack" ]]; then
    probe_pnpm "$adjacent_corepack" pnpm || true
  fi
fi
if ((${#pnpm_cmd[@]} == 0)); then
  corepack_home="${COREPACK_HOME:-${XDG_CACHE_HOME:-$HOME/.cache}/node/corepack}"
  cached_pnpm="$corepack_home/pnpm/$pnpm_version/bin/pnpm.mjs"
  if [[ -f "$cached_pnpm" ]]; then
    probe_pnpm node "$cached_pnpm" || true
  fi
fi
((${#pnpm_cmd[@]} > 0)) || die "Could not find a working pnpm $pnpm_version. Install the repository's pinned pnpm version or repair Corepack, then rerun this script."

printf 'Installing locked JavaScript dependencies when needed...\n'
"${pnpm_cmd[@]}" install --frozen-lockfile
printf 'Building the desktop web interface...\n'
"${pnpm_cmd[@]}" --filter @archaeodash/web build
printf 'Starting the native ArchaeoDash desktop app...\n'
cargo run --locked -p archaeodash-desktop-app
