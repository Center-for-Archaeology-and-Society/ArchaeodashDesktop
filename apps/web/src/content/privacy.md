# **Privacy Policy**

> Draft revision (2026-10-03) reflecting the implemented desktop/hosted
> architecture, replacing the legacy Shiny-era text that described all data
> as "stored in our secure database". Pending review before launch; this
> document is not legal advice (see IMPLEMENTATION Section 11).

## **Introduction**

ArchaeoDash is committed to protecting your privacy. This Privacy Policy explains how we collect, use, and protect your information, and how it differs between the desktop and hosted modes of the application.

## **Two Modes of Operation**

### **Desktop Mode**

In desktop mode, ArchaeoDash runs entirely on your computer. Your projects, group files, transformations, and results remain in files you control on your own storage. No account is required, no project data is transmitted to any server, and no analytics or telemetry leaves your machine.

### **Hosted Mode**

In hosted mode, you sign in with an account so your projects are available across devices and shared with collaborators you choose.

## **Information Collection**

### **Account Information (hosted mode)**

When you register for an account, we collect your chosen username and email address. Your password is never stored: only a salted, memory-hard hash (Argon2id) is kept, and it cannot be read back. Your email address is used only for account verification, password reset, and essential account communication.

### **Authentication and Security Records (hosted mode)**

To keep your account secure we store: session records (a one-way cryptographic digest of your session token — the raw token is never stored — with creation, expiry, and revocation times), one-way digests of one-time verification and reset tokens, and rate-limiting counters. Rate-limit counters are privacy-minimized: they are keyed by a keyed-hash (HMAC) digest, so raw email addresses, usernames, and IP addresses are never stored in the rate-limit table.

### **Cookies (hosted mode)**

ArchaeoDash uses a strictly necessary session cookie (`HttpOnly`, `Secure`, `SameSite=Lax`) so your browser can authenticate each request, and a strictly necessary CSRF cookie (`SameSite=Strict`) readable by the application to prevent cross-site request forgery. If you choose "remember me", the session cookie persists for your chosen duration of 30 or 90 days; otherwise it lasts only your browser session. No advertising, analytics, or third-party cookies are used. You can log out (which revokes the session server-side) or log out everywhere (which revokes all your sessions) at any time.

### **Preference Settings (hosted mode)**

Your interface preferences — theme, last-opened dataset name, table column visibility, and compact-mode flag — are stored per account so the interface looks the same when you return. These are typed, allowlisted settings; no analytical data is stored in the account database.

### **Your Data (both modes)**

Your project data — source files, group files, transformations, and explicitly exported results — is stored as project files you control (desktop) or in the hosted user file store (hosted mode), never as rows in the account database. Calculations such as PCA, UMAP, LDA, and clustering are ephemeral unless you explicitly save or export their results.

### **Operational Logs**

Server logs record request metadata (route, status, timing, a per-request correlation ID) for security and diagnostics. Logs never contain usernames, email addresses, uploaded file names, or project names.

## **Use of Information**

Account information is used to manage your access and to send verification and password-reset messages. Your project data is used only to provide the analysis and visualization features you invoke, in your own project scope. We do not share your data with third parties except the service providers listed under Subprocessors.

## **Data Security**

Passwords are hashed with a memory-hard algorithm (Argon2id) and are never stored or transmitted in readable form. Session and email-token material is stored only as one-way digests. Hosted transport is encrypted with TLS, and the hosted database and file store are encrypted at rest in the deployment environment. Security headers, strict cookie flags, login rate limiting, and single-use expiring email tokens are enforced by the application.

## **Data Retention and Deletion**

- Sessions end when you log out; you can revoke all sessions at once from every device. Remembered sessions expire after your chosen 30 or 90 days.
- Verification and password-reset tokens expire within 24 hours and can each be used only once.
- Rate-limit counters expire automatically within hours or days of your last attempt.
- Deleting a project removes its files from your visible store; hosted backups retain data for a limited window (about 30 days) purely for disaster recovery, after which it is removed.
- Account deletion (on request through the contact address below) removes your account, sessions, tokens, and preference records, and your hosted files per the published retention schedule.

## **Backups**

The hosted service maintains encrypted operational backups so the service can recover from failures. These backups are for the operator's disaster recovery, not a substitute for your own project backups. You remain responsible for keeping your own copies of your project files, in both desktop and hosted modes.

## **Subprocessors**

Hosted mode relies on an email delivery provider (to send verification and password-reset messages) and a hosting/infrastructure provider. They process data only on our instructions to provide the service.

## **Changes to the Privacy Policy**

We may update this Privacy Policy from time to time. Any changes will be posted on this page, and material changes will be announced in the application before they take effect. Your continued use of the application after changes take effect constitutes acceptance of the updated Privacy Policy.

## **Contact Us**

If you have questions about this Privacy Policy, or to request account deletion or an export of your hosted account data, contact us at [rbischoff\@asu.edu](mailto:rbischoff@asu.edu).
