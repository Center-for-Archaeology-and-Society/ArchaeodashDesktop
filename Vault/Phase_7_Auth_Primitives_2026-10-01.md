# Phase 7 Auth Primitives — 2026-10-01

## Context

Phase 6 exit gates were closed (see [[Phase_6_Performance_Budgets_Ratified_2026-10-01]]). Phase 7 of [[../../IMPLEMENTATION.md]] ("Hosted auth and operations") begins with the `archaeodash-auth` crate, which was a Phase 1 stub.

## What was built (commit `adadf98`)

Pure auth primitives per Section 11.1 of `docs/implementation/11-authentication-privacy-security.md`, all in `crates/auth/src/`:

- **password.rs** — Argon2id policy: 19,456 KiB memory, t=2, p=1 (OWASP baseline), PHC string format. `verify_password` returns `Valid`/`Invalid`/`RehashNeeded` (RehashNeeded when algorithm, version, or params are below policy — the hook for legacy libsodium migration in Phase 8). Length bounds 12–1024 chars. `verify_or_dummy` hashes against a process-lifetime dummy for uniform timing when the account does not exist. Malformed stored hashes return `Invalid`, never an error (no oracle).
- **token.rs** — 256-bit OS-RNG tokens via `getrandom::fill` (rand 0.10's `OsRng` no longer exists in this workspace's dep graph), base64url unpadded presentation, SHA-256 digests for storage, constant-time `digests_equal`, `ONE_TIME_LINK_TTL` = 24 h, remember-me choices [30, 90] days per Section 17.1 item 10.
- **identity.rs** — normalized username (a-z0-9._-, 3–40 chars, alphanumeric first/last, no consecutive dots) and email (single @, ≤64-char local, valid domain labels, ≤254 total, lowercased) for uniqueness checks.
- **throttle.rs** — throttle keys are HMAC-SHA256 (hand-rolled per RFC 4231 test vector) of `category || 0x00 || identifier` under a 32-byte server pepper, so leaked control-plane tables reveal no raw emails/IPs. `ThrottleStore` trait records-and-checks atomically (PostgreSQL implementation arrives with the control-plane crate); `InMemoryThrottleStore` for tests/dev. Named policies: login 10/15 min per account, 50/15 min per IP; verify 5/day; reset 5/hour.

## Dependency notes (cost us a few compile cycles)

- argon2 0.6 API differs from older tutorials: `PasswordHash` moved to `password_hash::phc::PasswordHash` (no lifetime param), salt via `password_hash::generate_salt()`, `hash_password_with_salt(&[u8], &[u8])`, params read back via `Params::try_from(&ParamsString)`.
- rand 0.10 removed `OsRng`; the workspace uses `getrandom = "0.4"` directly.
- CI clippy runs `-D warnings`; production code avoids `expect`/`unwrap` (`OnceLock` + `unwrap_or_default`, `PoisonError::into_inner` for mutex recovery). Test modules carry `#![allow(clippy::expect_used)]` like `crates/application/src/import.rs`.

## Verification

- `cargo test -p archaeodash-auth`: 21 passed (RFC 4231 HMAC vector, rehash-needed for weaker params and Argon2i, fixed-window reset math, base64url token shape, identity boundary lengths).
- `cargo clippy --workspace --all-targets`: no auth-crate warnings.
- `cargo fmt` applied. Pushed to `origin/master` as `adadf98`; CI result to be confirmed.

## Next steps for Phase 7

1. `archaeodash-control-postgres`: SQLx store implementing `ThrottleStore`, session store (digest-keyed, rotation at use/login), verification/reset single-use token store.
2. HTTP surface in `crates/api`: the `/auth/*` endpoints from Section 10 (register, verify, login, logout, logout-all, session, password-reset request/confirm) with HttpOnly cookies, CSRF, uniform errors.
3. Email adapters: SMTP, sendmail, dev sink (Section 11.1).
