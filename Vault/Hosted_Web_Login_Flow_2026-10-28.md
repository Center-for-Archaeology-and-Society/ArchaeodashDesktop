Up: [[Index]]

# Hosted web flow: sign in, open a project, analyse (2026-10-28)

Step 2 of [[Hosted_Login_Integration_Plan_2026-10-28]]. It builds on [[Hosted_Project_Workspaces_2026-10-28]], where the server serves each project's analysis API at `/w/{project_id}/api/v1/...`. Not committed.

## What changed

### Client (`packages/client`)
- New `Transport.hostedWorkspace` (`HostedWorkspaceService`: `set(projectId | null)`, `current()`, `onUnauthorized(handler)`).
- In `HttpTransport`, while a project is set, analysis calls go to `/w/<id>/...`. That covers imports, files, groups, transformations, ordination, clustering, jobs including the SSE events URL, explore and exports.
- Root calls (`rootRequest`) are never scoped: `/healthz`, auth, the project catalog, catalog files, hosted transformations and **preferences**. Preferences are account-level on the hosted server.
- A 401 on a scoped request calls the `onUnauthorized` handler. Root 401s (for example a signed-out session check) do not.

### Web (`apps/web`)
- **Mode detection:** `main.tsx` `AppRoot` asks `/api/v1/auth/session` at start. If the call succeeds, the app is in hosted mode. If it fails (the route doesn't exist), the app is in plain mode: the local single-project API used for dev and e2e. `?demo` is always plain (`isDemoMode()` in `transport.ts`), because the demo fetch answers the session route.
- **Session events:** `shell/hosted.tsx` has `observeSession` around the auth service passed to the Account and verify/reset pages. Sign-in and sign-out fire an event, and the app root refreshes the session.
- **Sign-in:** the app reopens this user's last project (remembered per username in `localStorage`, `archaeodash.hosted.lastProject.<user>`). Then it leaves `/account` for the saved return page, `/` if a project reopened, or `/projects` if not. Signing out, or a different user signing in, closes the project.
- **Opening a project:** `transport.hostedWorkspace.set(id)` and a project generation bump, so the Data Manager, data caches and pages remount, as with a desktop project switch. Opening from `/projects` goes to `/`. Creating a project opens it straight away.
- **Session expiry:** a scoped 401 shows the toast "Your session has ended. Sign in again to continue.", marks the session signed out, remembers the current page, and goes to `/account`. After signing in, the app returns to that page with the project reopened.
- **Gate:** `HostedGate` replaces only the analysis pages (`/explore`, `/visualize`, `/ordination`, `/cluster`, `/probabilities`, `/euclidean`, `/data`) with "Checking your session…", "Sign in to continue" or "Open a project". Home, Account, Projects, Info and the email-link pages are never gated.
- **`AppShell` `hosted` prop:**
  - top bar shows the project name, Open/Switch Project (link to `/projects`) and the username (link to `/account`, or "Sign in");
  - the sidebar shows a hosted note until a project is open;
  - styles live in `.hosted-controls` in `styles.css`.
- **`HostedProjectsPage`:** now only lists, creates and opens projects. Each project has an Open button via `HostedWorkspaceContext`; the open project shows "Open now". A guidance line says to import from the Data Manager.
  - The old catalog "Files" upload panel (`/api/v1/files?project_id=`) and the quota line are removed. Those uploads never reached the workspace the analysis pages use, and the quota only counted catalog files.
  - The `files` prop is gone. The client's `hostedFiles` and `quota()` remain for later use.
- **`vite.config.ts`:** the dev proxy also forwards `/w/` to the API.

## Verification
- `check.sh web`: typecheck passes and 311 web tests pass, including 6 new ones in `shell/hosted.test.tsx` (gating, gate text, shell controls, `observeSession`, last-project storage, projects page with and without the context).
- Client tests: 56 pass, including 2 new ones (scoped vs root URLs including SSE; the 401 hook fires only for scoped requests).
- **Browser test** on home2:
  - throwaway Postgres 16, `hosted` built with dev email (`AUTH_EMAIL_MODE=dev`, `AUTH_ALLOW_DEV_EMAIL=1`) on 127.0.0.1:18787, Vite dev on 127.0.0.1:5173;
  - an SSH tunnel to `localhost:5173` on the PC, because the session cookie is `Secure` and browsers accept that over plain HTTP only on localhost;
  - steps checked: signed-out gate → register → verification link (from the dev-sink log) → sign in (→ Projects) → create "Site A" (opens, → Home, workspace folder created) → full reload (session check, project reopened, Explore loads) → session deleted in the database → next scoped request: toast and `/account` → sign in → back on Ordination with Site A open.
  - Everything was torn down afterwards: containers, processes, `/tmp` data, tunnel.
- Port 8787 on home2 is used by something else, so use another port (`AUTH_API_PORT`) for local hosted runs.

## Not done or known gaps
- Importing a file through the Data Manager was not exercised in the browser, because the built-in browser has no file picker. The upload route is covered by the step 1 isolation test.
- Storage usage is not shown anywhere until workspace quotas exist (plan step 4). Show it again then.
- Steps 3 to 5 of the plan remain: single binary serving `dist/`, the Dockerfile, the route audit, quotas and limits, and the cas deployment.
