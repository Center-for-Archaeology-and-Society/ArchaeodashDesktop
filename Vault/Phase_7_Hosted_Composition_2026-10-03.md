# Phase 7: Hosted Preferences, Router Composition, and Server Binary (2026-10-03)

Commit `27534bc` on `master`.

## What was built

1. **Hosted preferences storage** — `crates/control-postgres/migrations/0002_user_preferences.sql`: `preferences` table with PK `(user_id, key)` and JSONB values, FK to `users` with `ON DELETE CASCADE`, per Section 6.5. `ControlStore::get_preferences` (ordered read) and `set_preference` (single-statement atomic upsert).

2. **Shared allowlist validation** — `application::validate_preference(key, value)`: the desktop file-based store and the hosted routes now share one typed allowlist (`PreferenceKey`: theme, lastOpenedDataset, columnVisibility, compactMode) and per-key shape validation, so they cannot drift.

3. **Preferences routes** — `GET/PUT /api/v1/preferences` in the auth router: session-scoped, CSRF double-submit enforced on PUT, unknown key / wrong shape → 422, unauthenticated → 401.

4. **Hosted composition** (`crates/api/src/hosted.rs`) — `hosted_router(state: HostedState)`: auth surface + `GET /health/live` (process liveness only) + `GET /health/ready` (read-only `schema_is_current()`, fails closed with 503, no sensitive details) wrapped in correlation-ID middleware (`x-request-id`: validated echo of a caller ID ≤128 chars from an alphanumeric/dash/underscore/dot set, else generated UUIDv7) and the security-headers middleware from Section 11.3.

5. **Hosted binary** (`crates/api/src/bin/hosted.rs`) — env-driven: `DATABASE_URL`, `AUTH_PEPPER` (64-hex), `AUTH_BASE_URL`, `AUTH_BIND` (default 127.0.0.1:8080), `AUTH_EMAIL_MODE` (`sendmail` default | `smtp` | `dev`), `AUTH_LOG_FORMAT=json|text`. `AUTH_APPLY_MIGRATIONS=1` is the explicit deployment migration step; startup always performs the read-only schema check and refuses to serve if migrations are absent (Section 6.5). Dev email mode refuses to start without `AUTH_ALLOW_DEV_EMAIL=1` and logs redacted recipients with action links.

## Real bugs the first true DB-backed test run caught

The earlier "DB-backed tests pass" claims were hollow: sqlx pool connect timeouts made `auth_state()` return `None` and the tests silently skipped (30 s of connect timeout, then green). With a working database (docker `--network host`; the userland proxy was eating connections on the mapped port), the tests surfaced:

- **`db_error` flattened every `ControlError` to 500** — duplicate username/email returned `internal_error` instead of the typed 409 `username_taken`/`email_taken` the enumeration-resistance contract requires. `db_error` now maps the typed variants.
- **Login replaced instead of appended the CSRF Set-Cookie** — the second `insert(SET_COOKIE, ...)` dropped the session cookie from the response; now `append`.
- **Handlers extracted `ConnectInfo` unconditionally** — fine behind `into_make_service_with_connect_info`, but the hosted binary didn't provide it (every auth route 500'd in the smoke test), and tests use `oneshot` without connect info. Handlers now take `Option<ConnectInfo<SocketAddr>>` with a `0.0.0.0` throttle fallback; the binary supplies real peer addresses; tests inject unique per-request peers via a middleware layer so per-IP throttle buckets never collide across parallel tests.
- Test-infrastructure only: `test_peer()` generates unique `10.x.y.z` addresses (client IP keys per request), `auth_state()` now fails loudly when `DATABASE_URL` is set but unreachable, and the hosted tests use `connect_lazy` so middleware tests run without a DB.

## Verification

- Live smoke test of the binary: register 202 → verify 204 → login 200 (both cookies) → preferences unauth 401, GET 200, PUT no-CSRF 403, PUT bad-key 422, PUT valid 204 with read-back `{"theme":"dark"}`; security headers + request IDs on responses; ready=200 with current schema.
- 233 workspace lib tests pass with a live `postgres:16-alpine`; fmt clean; clippy clean on all new code.
- Hosted CI is still blocked by the GitHub Actions billing failure (owner action required) — see [[Phase_7_Auth_Primitives_2026-10-01]].

## Remaining for Phase 7

Observability (Section 12: metrics beyond the request-ID/logs started here), backups, and deploy/rollback (Section 13 deployment disposition). Then Phases 8–9.
