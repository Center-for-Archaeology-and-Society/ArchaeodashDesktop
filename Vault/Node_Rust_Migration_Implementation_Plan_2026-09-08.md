# Node Rust Migration Implementation Plan 2026-09-08

## Summary

- Inventoried the complete R/Shiny application, supporting assets, tests, persistence/auth flows, deployment scripts, generated files, and dormant prototypes.
- Created the exhaustive root implementation blueprint at [[../IMPLEMENTATION]].
- Recommended a shared React/TypeScript client with a Rust application core, Axum hosted API, and Tauri desktop adapter.
- Revised persistence to an open group-file model: each analytical group is an authoritative, self-describing Parquet file; any metadata-complete Parquet inside the project is discoverable and is fully validated when added/loaded; the manifest is not an eligibility gate.
- Source spreadsheets remain anywhere inside the opened project and are not copied into a required `sources/` directory. Internal row identity is hidden `analytical_uuid`, while UI language uses “analytical unit.”
- Imported research groups default to read-only references but can be cloned or explicitly made editable with provenance retained. Measured elemental values are immutable in group files; logs, ratios, imputations, permutations, ordinations, and other calculated values are recomputed on demand and may only be exported as separate results.
- Hosted mode uses a private filesystem/S3-compatible user file store with PostgreSQL only for the control plane; Arrow/non-profile Parquet caches are rebuildable.
- Assigned every current component an explicit port, replacement, retirement, or archive disposition; no application code was changed.
- Included the public group-file profile, import partitioning, metadata readiness versus full validation, multi-group transactions/recovery, local file operations, statistical parity, legacy MySQL-to-group-file migration, security, testing, phased delivery, and definition-of-done gates.

## Related

- [[ArchaeoDash_Analysis_MOC]]
- [[System_Architecture_ArchaeoDash]]
- [[Quality_MOC]]
- [[Persistence_MOC]]
