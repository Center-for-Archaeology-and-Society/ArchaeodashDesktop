# Phase 7 Auth Primitives — 2026-10-01

## Context

Phase 6 exit gates were closed (see [[Phase_6_Performance_Budgets_Ratified_2026-10-01]]). Phase 7 of [[../../IMPLEMENTATION.md]] ("Hosted auth and operations") begins with the `archaeodash-auth` crate, which was a Phase 1 stub.

## What was built (commits `adadf98`, `35e20bf`)

Pure auth primitives per Section 11.1 of `docs/implementation/11-authentication-privacy-security.md`, all in `crates/auth/src/`:

- **password.rs** — Argon2id policy: 19,456 KiB memory, t=2, p=1 (OWASP baseline), PHC string format. `verify_password` returns `Valid`/`Invalid`/`RehashNeeded` (RehashNeeded when algorithm, version, or params are below policy — the hook for legacy libsodium migration in Phase 8). Length bounds 12–1024 chars. `verify_or_dummy` hashes against a process-lifetime dummy for uniform timing when the account does not exist. Malformed stored hashes return `Invalid`, never an error (no oracle).
- **token.rs** — 256-bit OS-RNG tokens via `getrandom::fill` (rand 0.10's `OsRng` no longer exists in this workspace's dep graph), base64url unpadded presentation, SHA-256 digests for storage, constant-time `digests_equal`, `ONE_TIME_LINK_TTL` = 24 h, remember-me choices [30, 90] days per Section 17.1 item 10.
- **identity.rs** — normalized username (a-z0-9._-, 3–40 chars, alphanumeric first/last, no consecutive dots) and email (single @, ≤64-char local, valid domain labels, ≤254 total, lowercased) for uniqueness checks.
- **throttle.rs** — throttle keys are HMAC-SHA256 (hand-rolled per RFC 4231 test vector) of `category || 0x00 || identifier` under a 32-byte server pepper, so leaked control-plane tables reveal no raw emails/IPs. `ThrottleStore` trait records-and-checks atomically (PostgreSQL implementation arrives with the control-plane crate); `InMemoryThrottleStore` for tests/dev. Named policies: login 10/15 min per account, 50/15 min per IP; verify 5/day; reset 5/hour.

## Control plane slice (commit `35e20bf`)

`crates/control-postgres` implemented per Section 6.5 (`docs/implementation/06-storage-architecture.md`):

- **Migration 0001** (`migrations/0001_auth_control_plane.sql`): `users` (normalized username/email UNIQUE, PHC password_hash, verified/disabled timestamps), `sessions` (32-byte BYTEA token_hash UNIQUE — raw tokens never stored), `account_tokens` (kind `verify_email`|`password_reset`, single-use via `used_at`), `auth_throttles` (HMAC key PK, window bucket, count).
- **`ControlStore`**: `migrate()` = explicit deployment step; `schema_is_current()` = read-only readiness check (API startup fails readiness if behind; no ad hoc DDL in user sessions).
- **ThrottleStore impl**: single atomic `INSERT ... ON CONFLICT DO UPDATE ... RETURNING` so fixed-window limits are replica-persistent. Documented fail-open on DB error (login still verifies passwords; outage visible in monitoring).
- **Sessions**: create (12 h cookie-session default, 30/90 d remember), live lookup with expiry/revocation/disabled checks + `last_seen_at` touch, atomic rotation at use/login (one transaction: revoke old digest + insert new), revoke one/all.
- **Account tokens**: atomic single-use consume; `password_reset` consumption revokes all sessions per Section 11.1.
- **create_user**: normalized uniqueness enforced by schema, surfaced as `UsernameTaken`/`EmailTaken` typed errors for uniform client messages.

Tests: 5 DB-backed integration tests, gated on `DATABASE_URL` (skip on machines/CI runners without a database so mac/win workspace jobs stay green). Verified locally against `postgres:16-alpine` in Docker: migrations, uniqueness violations, session lifecycle + rotation + revocation, single-use token replay rejection + reset revokes sessions, cross-replica throttle persistence (two `ControlStore` instances over one pool). The CI Rust job now runs a `postgres:16-alpine` service container.

Note: `ThrottleStore` became an async trait (RPITIT `async fn`) so the SQLx store can implement it without blocking; the in-memory store gained `#[tokio::test]` signatures.

## Dependency notes (cost us a few compile cycles)

- argon2 0.6 API differs from older tutorials: `PasswordHash` moved to `password_hash::phc::PasswordHash` (no lifetime param), salt via `password_hash::generate_salt()`, `hash_password_with_salt(&[u8], &[u8])`, params read back via `Params::try_from(&ParamsString)`.
- rand 0.10 removed `OsRng`; the workspace uses `getrandom = "0.4"` directly.
- CI clippy runs `-D warnings`; production code avoids `expect`/`unwrap` (`OnceLock` + `unwrap_or_default`, `PoisonError::into_inner` for mutex recovery). Test modules carry `#![allow(clippy::expect_used)]` like `crates/application/src/import.rs`.

## Verification

- `cargo test -p archaeodash-auth`: 21 passed (RFC 4231 HMAC vector, rehash-needed for weaker params and Argon2i, fixed-window reset math, base64url token shape, identity boundary lengths).
- `cargo clippy --workspace --all-targets`: no auth-crate warnings.
- `cargo fmt` applied. Pushed to `origin/master` as `adadf98`; CI result to be confirmed.

## Next steps for Phase 7

1. HTTP surface in `crates/api`: the `/auth/*` endpoints from Section 10 (register, verify, login, logout, logout-all, session, password-reset request/confirm) with HttpOnly cookies, CSRF, uniform errors.
2. Email adapters: SMTP, sendmail, dev sink (Section 11.1).
3. Headers/CSP, observability, backups, deploy/rollback (rest of Phase 7).

## Email adapters + HTTP surface (commit `6d5f79c`, `6cc5387`)

- `crates/auth/src/email.rs`: object-safe `EmailSender` trait (boxed futures, so runtime adapter selection works), `DevSinkEmailSender` (explicit in-memory sink), `SendmailEmailSender` (stdin pipe, no shell, CRLF header-injection stripping tested), `SmtpEmailSender` (lettre 0.11, rustls, `tokio1-rustls` feature required). `redact_email` masks local parts for logs; `build_action_link` validates base URL scheme/path before appending one-time tokens.
- `crates/api/src/auth.rs`: the eight `/api/v1/auth/*` routes per Section 10.1 over `AuthState { ControlStore, Arc<dyn EmailSender>, ThrottlePepper, base_url }`. Session cookie HttpOnly/Secure/SameSite=Lax; CSRF cookie SameSite=Strict without HttpOnly, double-submit via `X-CSRF-Token`; login does verify-or-dummy timing parity, unverified/disabled gates, rehash-on-login (store `update_password_hash`), session revocation + fresh issue; password-reset confirm revokes all sessions via the store. `ControlStore::mark_email_verified` added.
- Verified locally: 27 auth unit tests, 6 control-postgres DB tests, 30 API lib tests (8 DB-backed auth HTTP flows against postgres:16-alpine), fmt + clippy clean on all three crates.
- **Hosted CI outage**: from `398efca` (docs-only!) onward every job fails in ~4 s with zero steps and no runner — Actions infrastructure/billing failure, not code. Last green run: `2da9820`. Local gates (fmt, targeted clippy -D warnings, full workspace lib tests) verified instead; re-run CI when the account recovers.
