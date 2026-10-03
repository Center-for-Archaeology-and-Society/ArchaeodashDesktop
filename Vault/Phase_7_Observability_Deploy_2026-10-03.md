# Phase 7: Observability, Deploy Stack, Backup/Restore (2026-10-03)

Commit `41aa5c7` on `master`, following [[Phase_7_Hosted_Composition_2026-10-03]].

## What was built

1. **Request observability middleware** (`crates/api/src/hosted.rs`, Section 12): one structured log line per request — matched route template, method, status, `latency_ms`, correlation ID. Route templates only: a log-capture test proves submitted usernames/emails and raw unmatched paths never reach the logs. The `hosted` binary already emits JSON logs (`AUTH_LOG_FORMAT=json`).

2. **Deployment stack** (`deploy/`, replacing the legacy host-mutating `deploy.sh` flow per Section 13.2):
   - `deploy/compose/hosted-stack.yml` — API + PostgreSQL 16 + user-file volume; secrets injected via environment (never committed); DB not host-published; healthchecks on both services.
   - `deploy/container/Dockerfile` — multi-stage cargo-chef build, non-root runtime user (uid 10001).
   - `deploy/container/healthcheck` — liveness (`health/live`) distinguished from readiness (`health/ready`, the fail-closed schema gate); exit codes feed orchestrator restart policy, never shell-script restarts.

3. **Backup + restore drill** (Section 6.5; Phase 7 exit gate "restore drill pass"):
   - `deploy/backup.sh` — coordinated `pg_dump --format=custom` + WAL-position watermark (`backup.meta`, `watermark.lsn`) + user-file-store SHA-256 manifest + dump checksum, published atomically (single `mv`), `KEEP_BACKUPS` retention pruning (default 30, per Section 15 item 10).
   - `deploy/restore.sh` — refuses tampered bundles (checksum mismatch verified live), restores schema+data, validates the watermark, checks the five control-plane tables exist, optionally cross-checks file-store drift.
   - `docs/operations/hosted-deployment.md` — full deploy/backup/restore/rollback runbook (rollback = previous image tag without `AUTH_APPLY_MIGRATIONS`; down-migrations never automatic).

## Verification

- Restore drill executed against `postgres:16-alpine`: clean bundle restores and passes the schema check (exit 0); a corrupted dump is refused (exit 1, "dump checksum mismatch").
- Log-capture test proves route-template logging with no PII leakage.
- 252 workspace lib tests pass with a live DB (after making test peer IPs per-process random so repeated runs against a shared database don't collide with leftover per-IP throttle rows). fmt clean, clippy clean on all new code.

## Remaining for Phase 7 exit

- Hosted CI verification (blocked: GitHub Actions billing/spending limit — owner action).
- Threat-model findings sign-off.
- Browser-level security e2e suite (Phase 6 browser CI is also billing-blocked); the Section 14.3.1 API-level lifecycle rehearsal is now covered by `full_lifecycle_rehearsal` (commit `c9fb478`).
