# Hosted deployment, backup, and rollback runbook

Phase 7 operations slice (Sections 6.5, 10, 12, and the `deploy.sh`/`install.sh` disposition in 13.2). This replaces the legacy host-mutating `deploy.sh` flow: releases are immutable images, migrations are an explicit step, and rollback is image + database-pointer surgery, never source mutation on the host.

## Stack layout

- `deploy/compose/hosted-stack.yml` — Rust API (`archaeodash-api` `hosted` binary) + PostgreSQL 16 control plane + durable user-file volume. Secrets via environment, never committed.
- `deploy/container/Dockerfile` — multi-stage build; non-root runtime user; healthcheck installed.
- `deploy/container/healthcheck` — liveness (`/api/v1/health/live`) plus the API's own fail-closed readiness (`/api/v1/health/ready`, the read-only schema check from Section 6.5). Unhealthy reports to the orchestrator; restart is orchestrator policy, never a shell script parsing HTML plus CPU.
- `deploy/backup.sh` — coordinated control-plane dump + watermark + file-store manifest.
- `deploy/restore.sh` — restore drill: checksum, restore, watermark, schema check, optional file-store drift check.

## Environment (all required unless defaulted)

| Variable | Purpose | Default |
|---|---|---|
| `DATABASE_URL` | Control-plane Postgres DSN | — |
| `AUTH_PEPPER` | 64-hex throttle-key pepper (Section 11) | — |
| `AUTH_BASE_URL` | Public base URL for verification/reset links — must be the **web client origin** so `{base}/auth/verify?token=…` and `{base}/auth/reset?token=…` land on the client routes that consume the token | — |
| `AUTH_EMAIL_MODE` | `sendmail` \| `smtp` \| `dev` | `sendmail` |
| `AUTH_SMTP_*` | SMTP host/port/username/password/from when mode is `smtp` | — |
| `AUTH_ALLOW_DEV_EMAIL` | Must be `1` for the dev sink; refused otherwise | unset |
| `AUTH_BIND` | Listen address | `127.0.0.1:8080` |
| `AUTH_APPLY_MIGRATIONS` | `1` applies SQLx migrations at startup | unset |
| `AUTH_FILE_STORE_DIR` | Per-user file namespace root (Section 6.4); opaque UUID object keys live under `users/<id>/projects/<id>/` | `./data/user-files` |
| `AUTH_QUOTA_BYTES` | Per-user logical byte quota (Section 6.9); uploads reserve before staging and reconcile after | `1073741824` |
| `AUTH_RETENTION_DAYS` | Tombstones older than this many days are purged from the catalog and trash by the hourly sweep (Section 6.9) | `30` |

The dev email mode logs redacted recipients and action links and refuses to start without the explicit opt-in — it can never silently serve production.

## Deploy procedure

1. Build the image from a clean checkout at the release tag (CI does this; the Dockerfile never receives host state).
2. Start the new container with `AUTH_APPLY_MIGRATIONS=1`: migrations apply once, then the read-only schema check gates serving. A failed check exits non-zero and the orchestrator keeps the previous healthy container.
3. Wait for `health/ready` → 200 behind the load balancer's health gate before shifting traffic. Readiness is fail-closed: any store error is 503 with no sensitive detail.

## Backup

`deploy/backup.sh <output-dir>` (cron/systemd timer, daily per Section 6 item 6):

- `pg_dump --format=custom` of the control plane into a staging directory.
- A watermark file (`backup.meta` + `watermark.lsn`) recording the dump's WAL position, so a restore can match control-plane pointers to file-store versions. Object-store versioning alone is not a backup.
- A SHA-256 manifest of the user-file store (names + checksums only; contents are backed up by the storage volume mechanism) and a checksum of the dump itself.
- Atomic single-`mv` publication into `backup-<stamp>/`, then retention pruning (`KEEP_BACKUPS`, default 30 days per Section 15 item 10).

The dump and manifest land in the same bundle so control-plane and file-store positions can never drift by more than one backup interval.

## Restore drill (Phase 7 exit gate)

1. Provision a clean PostgreSQL instance.
2. `deploy/restore.sh <bundle> <target-dsn>`: verifies the dump checksum, restores, checks the watermark, validates the eight control-plane tables exist, and cross-checks the file-store manifest when `AUTH_FILE_STORE_DIR` is set.
3. Start the API against the restored DSN with `AUTH_APPLY_MIGRATIONS` unset — startup must pass the schema check without applying anything.
4. Exercise `health/ready` and an authenticated read.

Run the drill against a real backup before any promotion; a backup that has never been restored is a hypothesis.

## Auth/security browser e2e (Phase 7 exit gate)

`scripts/e2e/auth-security.mjs` drives headless Chromium through the real web
UI against the real hosted API: security headers, HttpOnly/CSRF cookie
split, register → email-link verify → replay rejection → remembered sign-in
→ in-page CSRF 403/204 probe → password-reset round trip → old password
rejected → sign-out-everywhere. Prerequisites:

1. PostgreSQL reachable at `DATABASE_URL` (any disposable database;
   `AUTH_APPLY_MIGRATIONS=1` applies migrations).
2. `AUTH_PEPPER` (64 hex characters), `AUTH_EMAIL_MODE=dev`,
   `AUTH_ALLOW_DEV_EMAIL=1` — the dev sink logs action links, which the
   script reads from the API process log.
3. `cargo build -p archaeodash-api --bin hosted`, then `npx vite --port 4173`
   in `apps/web` (the dev proxy forwards `/api` to `127.0.0.1:8787`).
4. Playwright chromium (`PLAYWRIGHT_MODULE` overrides the import).

Run: `DATABASE_URL=… AUTH_PEPPER=… node scripts/e2e/auth-security.mjs`
(prints `auth-security e2e: all assertions passed`; artifacts in
`scripts/e2e/out/`).

## Rollback

1. Stop traffic at the proxy (the API is stateless; sessions live in Postgres).
2. Re-point the container at the previous image tag and start it **without** `AUTH_APPLY_MIGRATIONS` — the previous image's schema check passes against the current schema because SQLx migrations are additive-forward; a down-migration is never executed automatically.
3. If the schema must move backward (rare; requires a written decision), restore the newest pre-change backup per the drill and replay nothing: the control plane holds only identity/session/preference state, and user files live in the versioned store.
4. Verify `health/ready` and one authenticated flow before shifting traffic back.
