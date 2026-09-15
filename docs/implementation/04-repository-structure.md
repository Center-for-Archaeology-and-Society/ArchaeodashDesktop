# IMPLEMENTATION Section 4 - Recommended repository structure

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 4. Recommended repository structure

```text
ArchaeoDash/
├── apps/
│   ├── web/                         # Vite React entry, web transport/config
│   ├── desktop/                     # Tauri 2 shell and capabilities
│   │   ├── src-tauri/
│   │   └── capabilities/
│   └── api/                         # Axum binary composition/root router
├── packages/
│   ├── client/                      # Shared React feature code
│   ├── contracts/                   # Generated TS API/IPC types; never hand-edited
│   ├── design-system/               # Tokens, accessible components, themes
│   └── test-fixtures/               # Client fixtures and mock transport
├── crates/
│   ├── domain/                      # IDs, entities, invariants, errors
│   ├── application/                 # Use cases, authorization, transactions
│   ├── analysis/                    # Transform and statistical algorithms
│   ├── data-io/                     # CSV/XLSX/Parquet/Arrow and name rules
│   ├── contracts/                   # Serde DTOs, OpenAPI/IPC schemas
│   ├── jobs/                        # Job state, progress, cancellation, limits
│   ├── storage/                     # Project/catalog/user-file traits only
│   ├── control-postgres/            # Hosted auth/catalog/job control plane
│   ├── project-manifest/            # Project metadata, journals, schema versions
│   ├── file-store-fs/               # Desktop and single-node hosted files
│   ├── file-store-s3/               # Hosted S3-compatible user files
│   ├── cache/                       # Rebuildable Arrow/computation cache adapters
│   ├── auth/                        # Password/session/token/email policy
│   ├── api/                         # Axum routes/middleware/problem responses
│   └── desktop/                     # Tauri commands and native integration
├── migrations/
│   ├── postgres/                    # Hosted control-plane schema only
│   └── project-manifest/            # Project metadata schema upgrades
├── tests/
│   ├── parity/                      # R-vs-Rust golden cases
│   ├── contract/
│   ├── integration/
│   ├── e2e-web/
│   ├── e2e-desktop/
│   ├── security/
│   └── performance/
├── fixtures/
│   ├── INAA_test.csv
│   ├── edge-cases/
│   └── golden/
├── tools/
│   ├── legacy-export-r/             # Temporary oracle/export tools only
│   └── legacy-account-transition/   # Optional, authorized identity/contact transition only
├── docs/
│   ├── architecture/
│   ├── operations/
│   ├── security/
│   ├── help/
│   └── adr/
├── deploy/
│   ├── container/
│   ├── compose/
│   └── systemd/
├── Cargo.toml                       # Rust workspace
├── Cargo.lock                       # committed for applications
├── package.json
├── pnpm-workspace.yaml
├── pnpm-lock.yaml
├── rust-toolchain.toml
└── IMPLEMENTATION.md
```

Keep the current R tree on a `legacy-r` branch or under a time-limited `legacy/` directory until parity and data migration are signed off. Do not intermingle R and new runtime code indefinitely.
