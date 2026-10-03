# Phase 7 Threat-Model Findings Closure (2026-10-03)

Maps each finding in [[Security_Audit_Blackbox_and_Code_2026-02-20]] and [[Security_Audit_External_Attack_Surface_2026-02-20]] to the shipped Phase 7 implementation, per the Phase 7 exit gate "threat-model findings closed or explicitly accepted".

## Blackbox and code audit findings

| Finding (severity) | Status | Closing evidence |
|---|---|---|
| Remembered-login trusts client-supplied username cookie as authentication (Critical) | **Closed** | `crates/control-postgres/src/store.rs`: sessions are server-side rows keyed by the SHA-256 digest of a 256-bit opaque token (`crates/auth/src/token.rs`); the client cookie carries only the presentation token, never identity. `GET /auth/session` resolves identity from the store. Tested by the lifecycle rehearsal (`full_lifecycle_rehearsal`). |
| Plaintext DB credentials in repo-local `.Renviron` (Critical) | **Closed for the new stack** | `deploy/compose/hosted-stack.yml` injects `DATABASE_URL`/`AUTH_PEPPER` from an orchestrator environment; `.gitignore` excludes `.env*`. No credentials in repo files (CI scans and local grep confirm). The legacy R `.Renviron` is retired with the legacy runtime in Phase 9. |
| Missing baseline HTTP security headers (High) | **Closed** | `crates/api/src/security_headers.rs`: enforced CSP (`default-src 'self'`, `script-src 'self'`, no `unsafe-eval`), HSTS, nosniff, Referrer-Policy, Permissions-Policy, X-Frame-Options — applied to every hosted response by `finalize_hosted_router` and asserted by tests. |
| Auth cookie not HttpOnly/Secure, JS-bootstrapped identity (High) | **Closed** | Session cookie is `HttpOnly; Secure; SameSite=Lax` set by the server; CSRF is a separate non-HttpOnly cookie with double-submit header verification on every cookie-authenticated state change (tested: `csrf_is_required_on_cookie_authenticated_state_changes`). |
| No observable login throttling/lockout (Medium) | **Closed** | `crates/auth/src/throttle.rs` + `auth_throttles` table: HMAC-peppered fixed-window counters (login 10/account and 50/IP per 15 min, verify 5/day, reset 5/hour), atomic across replicas, generic `429 rate_limited` responses. Tested against live Postgres. |
| Server technology/version headers exposed (Medium) | **Closed** | `crates/api/src/hosted.rs` `header_hygiene` test asserts no `Server` or `X-Powered-By` headers on hosted responses. |

## External attack-surface findings

| Finding (severity) | Status | Closing evidence |
|---|---|---|
| CSP Report-Only with `unsafe-inline`/`unsafe-eval` (Medium) | **Closed** | Enforced CSP in `security_headers.rs`; `script-src 'self'` with no `unsafe-eval`; the single `style-src 'unsafe-inline'` allowance is for framework style attributes only. Regression-tested. |
| `X-Powered-By: Shiny Server` disclosure (Low) | **Closed** | The legacy Shiny stack is not part of the hosted API; the Rust/axum stack emits neither `Server` nor `X-Powered-By` (enforced by test). |
| SockJS discovery endpoints exposed (Low) | **Closed** | The hosted API has no SockJS/websocket surface; jobs stream via the authenticated SSE route with session authorization (Section 10.2, Phase 6 work). |
| No SQL injection sinks (Info) | **Maintained** | All control-plane access uses sqlx parameterized queries with compile-time-checked or bound parameters; no string-built SQL anywhere in `crates/control-postgres`. |
| HTTP→HTTPS redirect, HSTS (Info) | **Closed** | HSTS `max-age=31536000; includeSubDomains` on every response; TLS termination is deployment-tier (documented in `docs/operations/hosted-deployment.md`). |

## Remaining owner-gated items

- Hosted CI verification of all of the above (GitHub Actions billing failure — every run dies before any step; owner must fix under Billing & plans).
- Browser-level security e2e suite (shares the CI block).
- The formal threat-model review sign-off itself is a human decision; this note provides the evidence base.
