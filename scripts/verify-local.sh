#!/usr/bin/env bash
# Local CI-equivalent verification (Section 15.3 release-gate subset that
# GitHub Actions used to run as the `rust` and `node` jobs in ci.yml).
#
# GitHub Actions is unavailable on this account (billing/spending-limit
# failure), so the hosted verification is replaced by this script: the same
# checks run in Docker containers against a real PostgreSQL 16 service.
#
#   1. cargo fmt --check
#   2. cargo clippy --workspace -- -D warnings
#   3. cargo test --workspace        (DATABASE_URL points at the container)
#   4. pnpm install --frozen-lockfile && pnpm -r build && pnpm -r test
#
# The multi-OS desktop-cross and desktop-launch-smoke jobs, the Phase 6
# browser/performance workflows, and the legacy R workflow remain hosted-CI
# only; they have no local Linux-container equivalent (macOS/Windows).
#
# Usage: scripts/verify-local.sh [job ...]   (default: all; e.g.
#        `scripts/verify-local.sh rust` runs only the Rust gate)
set -euo pipefail
cd "$(dirname "$0")/.."

compose() {
  docker compose -f deploy/compose/verify.yml -p archaeodash-verify "$@"
}
jobs=("$@")
if [ ${#jobs[@]} -eq 0 ]; then
  jobs=(rust node)
fi

compose up -d --wait postgres
cleanup() {
  compose down --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

status=0
for job in "${jobs[@]}"; do
  case "$job" in
    rust)
      echo "== verify-local: rust fmt =="
      compose run --rm rust-check cargo fmt --all -- --check || status=1
      echo "== verify-local: rust clippy =="
      compose run --rm rust-check cargo clippy --workspace -- -D warnings || status=1
      echo "== verify-local: rust test (live PostgreSQL) =="
      compose run --rm rust-check cargo test --workspace || status=1
      ;;
    node)
      echo "== verify-local: node build =="
      compose run --rm node-check bash -lc \
        'corepack pnpm install --frozen-lockfile && corepack pnpm -r build' || status=1
      echo "== verify-local: node test =="
      compose run --rm node-check corepack pnpm -r test || status=1
      ;;
    *)
      echo "unknown job: $job (expected rust or node)" >&2
      exit 2
      ;;
  esac
done

if [ "$status" -ne 0 ]; then
  echo "verify-local: FAILED" >&2
  exit "$status"
fi
echo "verify-local: all gates passed"
