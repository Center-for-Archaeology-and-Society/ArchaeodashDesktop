# Phase 8 Non-Gated Rehearsals and Content Update (2026-10-04)

Phase 8's account transition itself is owner-gated (Section 14.2: "only if an
authorized product decision requires retaining legacy accounts"). These items
are the non-gated parts, verified without that decision:

## Section 14.3 rehearsals

1. **Auth lifecycle rehearsal** — implemented as
   `crates/api/src/auth.rs::lifecycle_rehearsal` (`full_lifecycle_rehearsal`,
   plus the remember-days validation test): register → verify + replay
   rejection → session reflection → remembered login (30/90 asserted) →
   logout-all revocation → password reset → old password dead → re-login.
   Passes against live PostgreSQL.
2. **No analytical content in the control plane** — enforced as a test
   invariant (`schema_contains_no_analytical_content` in
   `crates/control-postgres/src/store.rs`): the migrated schema holds exactly
   the Section 6.5 tables (`users`, `sessions`, `account_tokens`,
   `auth_throttles`, `preferences`, `_sqlx_migrations`), and the `users`
   columns match the spec.
3. **File-store authorization does not rely on MySQL identifiers** — verified
   by inspection and grep: no MySQL dependency exists anywhere in the new
   runtime (no `mysql` in any `Cargo.toml`/`package.json`), and ownership is
   opaque UUIDs (`crates/domain/src/lib.rs`: `SessionId`,
   `ProjectId` "Opened directory (desktop) or hosted namespace identity") —
   no legacy identifier can leak into authorization decisions because none is
   ever stored or consulted.

## Help/Terms/Privacy update (Section 13 disposition)

- `inst/app/www/privacy.md` and `terms.md`: rewritten for the implemented
  architecture (desktop vs hosted storage, Argon2id, HttpOnly/CSRF cookies,
  30/90-day remember, retention/subprocessors/backups) — marked draft pending
  the pre-launch counsel review Section 11 requires.
- `apps/web/src/content/help.md`: revised from the legacy Shiny-era text
  (R Shiny timeouts, LOGIN button, "account database area", rio package) to
  the implemented two-mode architecture with the actual UI labels; stripped
  the RMarkdown front-matter and target=_blank script (retired with the
  legacy runtime in Phase 9). The legacy `inst/app/www/` copies stay as-is
  until Phase 9's archival tag.
- The hosted web client is the single canonical help/privacy/terms surface;
  the desktop shell shares the same routes through Tauri.

## Repository scan (Phase 9 exit criterion, verified early)

- No secrets: only placeholder examples (`.Renviron.example`) and env-var
  references (`${POSTGRES_PASSWORD:?…}`) are committed; `.Renviron` itself is
  gitignored (the 2026-02-20 audit's Critical finding is structurally closed).
- No orphaned MySQL dependencies in the new runtime.
- The remaining orphaned legacy artifacts (`inst/app/www/help_files/*`
  vendored jquery/bootstrap, the R package itself) are retired in Phase 9
  after the rollback window and archival tag — owner-gated.

## Consent-version enforcement (Section 10.1)

Registration validates `consent_version` against the published constant
(`CONSENT_VERSION = "2026-10"`): stale or missing versions are rejected with
422, and the accepted version + timestamp are recorded per account
(migration `0003_user_consent.sql`). `GET /api/v1/auth/consent` publishes the
current version so clients can display and accept the right notice. The
client registration dialog fetches it and blocks submission until accepted.
