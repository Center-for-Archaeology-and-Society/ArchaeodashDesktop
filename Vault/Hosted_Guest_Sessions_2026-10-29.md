Up: [[Index]]

# Hosted guest sessions (2026-10-29)

Anonymous use of the hosted app, on top of [[Hosted_Project_Workspaces_2026-10-28]] and [[Cas_Hosted_Deployment_2026-10-29]]. Owner's rule: data is held in temporary files and can only be reached while the page is open, and it is cleared daily, or sooner when storage runs short. Not committed or deployed yet.

## How it works
- **Starting:** `POST /api/v1/guest` creates a workspace in `AUTH_GUEST_DIR` (default `/tmp/archaeodash-guests`, inside the container) and returns a random token (`OpaqueToken`, 256-bit).
  - The server keeps only the SHA-256 of the token. The folder name is a separate UUID, so the token never reaches the filesystem.
  - No database rows are written and no cookie is set, so there is no cross-site request forgery exposure.
- **Use:** requests go to `/g/{token}/api/v1/...`, the same analysis router as `/w/{project_id}`, with the body limit and shared analysis slots (keyed by the guest ID).
  - Using a path prefix keeps job progress working, since `EventSource` can't send headers.
  - The app's request log records the route pattern, not the token. Apache's access log does contain the URL.
- **Page-only:** the web app keeps the token in React state only (no `localStorage`, no cookie). A reload or a closed tab loses it, and on `pagehide` the page sends `navigator.sendBeacon` to `POST /api/v1/guest/end`, which deletes the folder at once.
  - `beforeunload` asks you to confirm before leaving.
  - The page pings `POST /api/v1/guest/ping` every 5 minutes.
- **Cleanup (`GuestSessions::sweep`):** removes sessions idle longer than 2 h or older than 24 h, then the least recently used while all guest data is over the total (down to 80% of it).
  - It runs every 10 minutes, before each new session and before each request that adds data.
  - At startup the guest folder is emptied, but only if it carries the `.archaeodash-guests` marker. A non-empty folder without it is refused, so a mistyped setting can't wipe user files.
- **Limits (`GuestLimits`, env in `bin/hosted.rs`):**

  | Setting | Default |
  |---|---|
  | `AUTH_GUEST_QUOTA_BYTES` (per guest) | 200 MiB |
  | `AUTH_GUEST_IDLE_SECS` | 2 h |
  | `AUTH_GUEST_MAX_AGE_SECS` | 24 h |
  | `AUTH_GUEST_TOTAL_BYTES` | 5 GiB |
  | `AUTH_GUEST_MAX_SESSIONS` | 100 (beyond it, 503 `guest_capacity`) |
  | `AUTH_GUEST_STARTS_PER_IP_PER_HOUR` | 10, in memory (beyond it, 429) |

  Guests are on unless `AUTH_GUESTS=0`. The settings are documented in `deploy/cas/compose.yml`, `env.example` and `hosted-stack.yml`.
- **Errors:** an unknown or expired token gets 401 `guest_expired`. The web app then says the session ended and its data was deleted, and goes back to Account.

## Code
- **Server:**
  - `crates/api/src/guests.rs` (new);
  - `WorkspaceRegistry::{with_guests, guests, open_router, forget_slots}` (`workspaces.rs`);
  - routes in `hosted.rs`, and `/g/` excluded from the web fallback;
  - settings and the 10-minute sweep in `bin/hosted.rs`.
- **Client:** `GuestService { start, ping, end }`, `HostedWorkspaceService.setGuest`, and `transport.guest` on `HttpTransport`.
- **Web:**
  - `GuestContext` and `GuestStart` (`shell/hosted.tsx`): "Continue without an account" on the Account page and the "Sign in to continue" screen, or a note while a session is open;
  - in `main.tsx`: guest state, ping, `beforeunload`, `pagehide` end, the expiry handler, and signing in ends the guest session;
  - in the top bar: "Guest session" with a "Temporary" badge and Sign in.
- **Tests:**
  - Rust: 5 tests (startup marker and emptying; start, end, rate limit and cap; idle and over-total sweep; the routes end to end with quota and end; off means 404);
  - client: 1 (`/g/` scoping, ping and end bodies);
  - web: 2 (start button and note; top-bar badge).
  - `check.sh rust` (api and auth crates) and `check.sh web` (314 tests) pass, and the client package has 57 passing tests.

## Rehearsal on home2 (passed, then removed)
- **Setup:** the image behind httpd with the real Apache snippet, as in [[Cas_Hosted_Deployment_2026-10-29]].
- **Checks:**
  - "Continue without an account" opens the Data Manager as "Guest session" with the Temporary badge;
  - upload and import through `/archaeodash/g/<token>/` work with no cookies, and PCA runs;
  - leaving the page logged `removed guest session`, after which the old token gets 401 and the guest folder is empty.

## Not done
- Guests can't save a session into an account. Signing in ends it, and the note says so.
- Not deployed to cas.
