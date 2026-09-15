#!/usr/bin/env bash
# Ignore Tauri generated schemas, then commit the Phase 1 skeleton.
set -euo pipefail
cd ~/ArchaeodashDesktop
if ! grep -q "src-tauri/gen" .gitignore; then
  printf '\n# Tauri generated\napps/desktop/src-tauri/gen/\n' >> .gitignore
fi
git rm -r --cached apps/desktop/src-tauri/gen -q 2>/dev/null || true
git add -A
git commit -q -F - <<'MSG'
Phase 1: Rust/TS monorepo skeleton per IMPLEMENTATION.md Section 4

- 15-crate Rust workspace (domain IDs/entities/errors, contracts, application
  use cases, Axum api, Tauri desktop shell, stubs for remaining crates)
- Shared app_info smoke use case verified through HTTP and Tauri adapters
- pnpm workspace: contracts, design-system, test-fixtures, client, apps/web
- Toolchain pins, workspace lints (unsafe forbidden), CI workflow
- Phase 0 artifacts already present: R oracle, 14 goldens, inventory docs
- Vault notes for Phase 0/1 and interaction log updates
MSG
git log --oneline -3
