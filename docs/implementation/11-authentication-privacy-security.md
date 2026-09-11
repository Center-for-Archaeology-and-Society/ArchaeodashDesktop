# IMPLEMENTATION Section 11 - Authentication, privacy, and security

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 11. Authentication, privacy, and security

### 11.1 Hosted auth replacement

- Verify existing libsodium hashes during migration, then rehash on successful login to the selected Argon2id policy if parameters are outdated.
- Enforce normalized username character/length rules and unique normalized email. The current “first row by email” ambiguity is removed.
- Generate 256-bit random session/account tokens; store only SHA-256/HMAC digests; rotate remembered sessions at use and login.
- Use opaque HttpOnly cookies. Never restore the current `document.cookie -> Shiny input` flow.
- Require CSRF protection on state-changing cookie-authenticated requests; strict CORS allowlist; origin checks for browser requests.
- Rate-limit login by privacy-minimized IP/account key and reset/verification by IP/account/email, persistently across replicas. Use uniform messages and timing where enumeration matters.
- Verification/reset links expire in 24 hours and are single-use. Reset revokes existing sessions. Add resend with separate throttle.
- Email sender supports SMTP and local sendmail adapters plus an explicit development sink. Never log tokens or full email content in production.

### 11.2 Upload and data controls

- Stream uploads to bounded quarantine, inspect signature, and delete on failure/expiry.
- Reject path components from uploaded filenames and never use them as storage keys.
- Protect against ZIP bombs, formula injection, malformed workbooks, excessive sheets/styles/shared strings, oversized decompression, and parser timeouts.
- Treat retained native files as untrusted downloads: use safe `Content-Type`/`Content-Disposition`, disable inline execution for active formats, and never serve user objects from the application origin as executable content.
- Authorize every resolved project/file/group/revision/result by owner ID; never trust IDs or storage keys merely because the UI listed them.
- Reject symlink, hard-link, archive traversal, and project-manifest escape attempts. Hosted storage adapters must enforce the opaque owner/project prefix after resolving authorization.
- Encrypt transport with TLS and hosted storage with platform-supported encryption. Document backup encryption and key rotation.
- Redact data values, emails, usernames where unnecessary, tokens, paths, and SQL details from logs. Use actor IDs/correlation IDs.

### 11.3 Desktop controls

- Tauri capabilities default deny. Grant only dialog, app-specific directories, updater, and narrowly scoped path access required by the commands.
- Keep file parsing in Rust, not the webview; validate imported/exported paths and canonicalize safely.
- Apply a strict CSP without `unsafe-eval`; bundle JS/CSS/fonts locally; no remote script execution.
- Disable arbitrary navigation/new-window behavior. External help/issues links open through a constrained shell/open plugin allowlist.
- Sign Windows/macOS installers and updates. Protect updater signing private keys in CI secrets; ship only the public verification key.
- Telemetry is absent or opt-in and documented. Local logs rotate and never contain analytical-unit row values.

### 11.4 Legal/help content

Retain the substantive Help, Terms, Privacy, account communication notice, ownership/no-sharing statements, backup warning, MIT/no-warranty statement, support contact, hosted URL, issue tracker, and credits. Rewrite mode-dependent wording so project-contained source files, self-describing group files, reference-group editability, ephemeral calculations, explicit result exports, quotas, retention, deletion, recovery, and backups are distinguished clearly. Have the privacy/terms text reviewed before launch; this plan is not legal advice.

The old cookie banner should not be copied mechanically. Determine with counsel whether strictly necessary session/remember cookies need a banner in the deployment jurisdiction. Regardless, remembered login remains an explicit user choice and its behavior is documented.
