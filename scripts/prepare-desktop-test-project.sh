#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
destination="${1:-${TMPDIR:-/tmp}/archaeodash-desktop-test-drive}"

command -v cargo >/dev/null 2>&1 || { printf 'Sample setup: Rust and Cargo are required.\n' >&2; exit 1; }
if [[ -e "$destination" ]]; then
  printf 'Sample setup: destination already exists; choose a new path to protect its contents:\n  %s\n' "$destination" >&2
  exit 1
fi

mkdir -p -- "$(dirname -- "$destination")"
printf 'Creating a disposable INAA test project at %s\n' "$destination"
cargo run --locked --manifest-path "$repo_root/Cargo.toml" -p archaeodash-application --example desktop_test_project -- "$destination"
