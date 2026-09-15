# Phase 0 Production Inventory — 2026-09-14

Status: completed for repository and local-runtime evidence; this is not an assertion about inaccessible live infrastructure.

## Scope decision

Per the migration direction, MySQL is excluded from the new runtime and from analytical-data migration. No MySQL connection was made and no MySQL table, row, credential, or user record was inspected for this inventory.

The new hosted PostgreSQL control plane is limited to necessary identity/contact and security records:

- opaque account ID, normalized username/email, password hash, verification status/time, and account-contact state;
- server-side session/remember-token hashes, password-reset and email-verification token hashes, expiration/revocation state, rate-limit state, and minimal security audit records.

PostgreSQL must not store analytical rows, elemental values, source spreadsheets, group Parquet contents, transformations, result matrices, plot data, or general user preferences. Those items remain in the project/user file store or project metadata as specified by the file-first design. MySQL is not a dependency of the new desktop or hosted application.

This supersedes any interpretation of the older plan that requires MySQL analytical-table discovery, extraction, reconciliation, or import. If retained legacy accounts need a transition, it is limited to an authorized, minimal identity/contact transition with password-reset/verification handling; it is not a dataset migration.

## Evidence captured

| Area | Verified inventory | Evidence/source |
|---|---|---|
| Repository baseline | `master` at `3b3169c` (`2026-09-11`); 48 legacy R files, 32 test files, 95 `inst/` files | Git and tracked-file inventory |
| Legacy runtime | R/Shiny package; Docker image base `rocker/shiny-verse:4.4.3`; `RMySQL`/`DBI` are legacy dependencies | `DESCRIPTION`, `Dockerfile`, `R/connect.R` |
| Legacy identity/contact behavior | Registration, email verification, password reset, 30-day remembered login, hashed passwords/tokens, and email delivery configuration | `R/loginServer.R`, `R/authEmail.R` |
| Configuration contract | Placeholder-only `.Renviron.example`; documented base URL, sender/reply-to, sendmail or SMTP settings | `.Renviron.example`, `README.md` |
| Local runtime | Ubuntu 24.04.1 on WSL2; PostgreSQL accepts local connections; no Docker application containers are running | local read-only checks on 2026-09-14 |
| Deployment scripts | `deploy.sh` expects an external `../docker-compose.yml`, default service/container `archaeodashbeta`, and mutates source/version tags during deployment | `deploy.sh` |
| Reverse proxy/health | Apache hardening snippet uses report-only CSP; health check and beta watchdog scripts exist | `security/` |
| Storage capacity | Local development filesystem: 926 GiB free at capture time | local read-only check |

## New-runtime control-plane boundary

| Allowed in PostgreSQL | Explicitly prohibited in PostgreSQL |
|---|---|
| Account identity/contact, password hashes, verification/reset/session-token hashes, expiry/revocation, rate-limit and minimal security audit records | Dataframe rows, elemental values, analytical UUID-to-row mappings, source bytes, Parquet/Arrow blobs, transformations, analysis results, plots, general preferences, uploaded files, and arbitrary user content |

Identity/contact records use opaque account IDs and server-set `HttpOnly`, `Secure` cookies. Raw passwords, raw verification/reset/remember tokens, SMTP passwords, and storage credentials are never stored in this report, source control, client code, or Vault.

## Deployment facts not available from this workspace

The following have no checked-in, sanitized source and therefore remain **unknown**, not assumed:

- live URL/domain, proxy virtual-host routing, TLS certificate ownership/renewal, and effective CSP;
- external Compose services, ports, volumes, environment injection, restart policy, and image digest;
- production backup/restore evidence, retention schedule, monitoring destination, and incident contacts;
- active account count and whether an authorized identity/contact transition is needed.

These are deployment-operator facts. They must be supplied as sanitized evidence before hosted cutover, without collecting production secrets or MySQL analytical data.

## Consequences for Phase 0

The R oracle and INAA baselines are complete. The remaining Phase 0 work is independent of MySQL analytical storage: numerical/UMAP spikes, tolerance approval, legal-text ownership, and the sanitized external deployment evidence above. Dataset-size and quota decisions must be based on fixture/performance-ladder measurements and future file-store telemetry, not MySQL table sizes.
