# Placeholder directories from IMPLEMENTATION.md Section 4.

This tree exists so the Phase 1 skeleton matches the documented layout.
Populated by their owning phases:

- `parity/` — R-vs-Rust golden cases (Phase 2+; goldens live in `fixtures/golden/`)
- `contract/` — group-profile, scanner, store, and API contract tests (Phase 2+)
- `integration/` — cross-crate integration tests
- `e2e-web/` — Playwright web end-to-end tests (Phase 5+)
- `e2e-desktop/` — Tauri end-to-end tests (Phase 5+)
- `security/` — cross-user, CSRF, upload, redaction tests (Phase 7+)
- `performance/` — Section 12 benchmark ladder (10k/100k/1M rows)
