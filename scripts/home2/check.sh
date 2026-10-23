#!/usr/bin/env bash
# Runs the project checks on home2 against the repository on the share.
#
# Usage: bash scripts/home2/check.sh [MODE] [CRATE...]
#   all     (default) Rust fmt check, tests, clippy; web typecheck and tests
#   rust    Rust only. Optional CRATE names (archaeodash-*) limit tests and
#           clippy to those packages; otherwise the whole workspace except
#           the Tauri app (which needs GTK on Linux).
#   web     TypeScript typecheck (contracts, client, web) and web tests
#   parity  Rust golden parity tests (tests/parity)
#   fmt     Applies cargo fmt to the workspace (rewrites files)
#
# Exits non-zero if any check fails.
set -euo pipefail
. "$(dirname "$0")/common.sh"

mode="${1:-all}"
[ "$#" -gt 0 ] && shift
case "$mode" in all|rust|web|parity|fmt) ;; *) die "unknown mode '$mode' (all|rust|web|parity|fmt)";; esac
for crate in "$@"; do
  [[ "$crate" =~ ^archaeodash-[a-z0-9-]+$ ]] || die "invalid crate name '$crate'"
  [ "$mode" = rust ] || die "crate names are only accepted with the rust mode"
done

run_remote '
set -uo pipefail
mode="$1"; shift
cd '"$HOME2_REPO"'
export CARGO_TARGET_DIR='"$HOME2_TARGET"'
status=0
step() { printf "\n== %s ==\n" "$1"; }
packages=()
if [ "$#" -gt 0 ]; then for c in "$@"; do packages+=(-p "$c"); done
else packages=(--workspace --exclude archaeodash-desktop-app); fi

if [ "$mode" = fmt ]; then
  step "cargo fmt"; cargo fmt --all && echo "formatted" || status=1
  exit $status
fi
if [ "$mode" = all ] || [ "$mode" = rust ]; then
  step "cargo fmt --check"
  cargo fmt --all -- --check >/tmp/archaeodash-fmt.txt 2>&1 && echo "ok" || { grep "^Diff in" /tmp/archaeodash-fmt.txt | head -20; status=1; }
  step "cargo test"
  out=$(cargo test "${packages[@]}" 2>&1); rc=$?
  printf "%s\n" "$out" | grep -E "^test result|FAILED|panicked|^error" | sort | uniq -c
  [ $rc -eq 0 ] || status=1
  step "cargo clippy (-D warnings)"
  out=$(cargo clippy "${packages[@]}" -- -D warnings 2>&1); rc=$?
  printf "%s\n" "$out" | grep -E "^(warning|error)" -A6 | head -40
  [ $rc -eq 0 ] && echo "ok" || status=1
fi
if [ "$mode" = parity ]; then
  step "parity tests"
  out=$(cargo test -p archaeodash-parity 2>&1); rc=$?
  printf "%s\n" "$out" | grep -E "^test result|FAILED|panicked|^error" | sort | uniq -c
  [ $rc -eq 0 ] || status=1
fi
if [ "$mode" = all ] || [ "$mode" = web ]; then
  tsc="node '"$HOME2_REPO"'/node_modules/typescript/bin/tsc"
  for pkg in packages/contracts packages/client apps/web; do
    step "typecheck $pkg"
    (cd "$pkg" && $tsc --noEmit) && echo "ok" || status=1
  done
  step "web tests"
  out=$(cd apps/web && node --test --test-reporter=tap --import ../../scripts/ts-register.mjs "src/**/*.test.tsx" "src/**/*.test.ts" 2>&1); rc=$?
  printf "%s\n" "$out" | grep -E "^# (tests|pass|fail)"
  printf "%s\n" "$out" | grep -A14 "^not ok" | grep -E "not ok|error:|expected|actual" | head -30
  [ $rc -eq 0 ] || status=1
fi
printf "\n== %s ==\n" "$([ $status -eq 0 ] && echo "ALL CHECKS PASSED" || echo "SOME CHECKS FAILED")"
exit $status
' "$mode" "$@"
