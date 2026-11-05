Up: [[Index]]

# Hosted deployment to cas at /archaeodash (2026-10-29)

Step 5 of [[Hosted_Login_Integration_Plan_2026-10-28]]. The owner chose a sub-path, `https://cas.rc.asu.edu/archaeodash/`, over a separate host name. Trade-off: it shares an origin with Shiny, heurist and CollectiveAccess, so an XSS bug in any of them could act as a signed-in ArchaeoDash user. **Live since 2026-10-29 16:25 UTC** with open registration (owner's choice). Repo changes not committed yet.

## Code changes (uncommitted)
- **Web sub-path:**
  - `ARCHAEODASH_BASE_PATH` sets Vite `base` (validated as `/name`).
  - New `apps/web/src/basePath.ts` (`BASE_PATH`, `publicUrl`); it falls back to the site root when `import.meta.env` is missing (node tests).
  - `HttpTransport(BASE_PATH)`, router `basename`, the help-window path, and logo and brand-mark URLs.
  - The desktop app and root builds are unchanged.
- **Email links:** `build_action_link` (`crates/auth/src/email.rs`) now accepts a base URL with a path (`https://host/archaeodash` → `/archaeodash/auth/verify?token=`). It still refuses queries, fragments, credentials and non-http schemes.
- **`AUTH_EMAIL_MODE=smtp-local`:** plain SMTP without TLS or login, to a loopback host only (`SmtpEmailSender::local`, `is_loopback_host`). cas's postfix listens on 127.0.0.1 only, so the app container uses the host network.
- **`AUTH_TRUSTED_PROXIES`:** `trust_forwarded_client` (`crates/api/src/hosted.rs`). From a trusted peer, the last `X-Forwarded-For` entry becomes the client address for login and registration throttles. Without it, everyone behind Apache would share 127.0.0.1, and one attacker could lock out all logins.
- **Dockerfile:** `ARG ARCHAEODASH_BASE_PATH` in the web stage.
- Tests:
  - action links with a sub-path and the refusals;
  - local SMTP refuses non-loopback hosts;
  - forwarded client trusted only from the proxy.
- `check.sh rust` (auth and api crates) and `check.sh web` (312 tests) pass.

## Deployment files (`deploy/cas/`)
- `compose.yml` (project `archaeodash`, in `/srv/archaeodash` on local disk):
  - `postgres:16-alpine` on `127.0.0.1:55433`, data in `/srv/archaeodash/postgres`, 2 GB memory.
  - `archaeodash-hosted:${ARCHAEODASH_TAG}`: host network, `AUTH_BIND=127.0.0.1:18787`, `AUTH_BASE_URL=https://cas.rc.asu.edu/archaeodash`, `smtp-local` to 127.0.0.1:25, trusted proxies 127.0.0.1 and ::1, 4 analysis slots, `cpus: 8`, `mem_limit: 8g`.
  - User files in `/srv/archaeodash/user-files`, which must be owned by 10001.
- `env.example`: `ARCHAEODASH_TAG`, `POSTGRES_PASSWORD`, `AUTH_PEPPER` and `AUTH_SMTP_FROM`. The real `.env` is root-only (mode 600) on cas, never in the repo or the Vault.
- `apache-archaeodash.conf`:
  - `RedirectMatch` from `/archaeodash` to `/archaeodash/`;
  - `<Location /archaeodash/>` with `ProxyPass http://127.0.0.1:18787/ flushpackets=on timeout=600` and `ProxyPassReverse`;
  - `ProxyPassReverseCookiePath / /archaeodash/` and `LimitRequestBody 70000000`.
  - Install as `/etc/apache2/conf-available/archaeodash-location.conf`, `Include` it inside the cas.rc.asu.edu:443 vhost, then `apachectl configtest` before reloading.
- `backup.sh` (root cron, nightly):
  - `pg_dump` from `archaeodash-db-1` to `/mnt/storage/backups/archaeodash/db`, keeping 14 days;
  - `rsync -rlt --delete` of user files (a mirror, not history).
- Image `archaeodash-hosted:cas` (137 MB) is built on home2 with `--build-arg ARCHAEODASH_BASE_PATH=/archaeodash`. Copy it with `ssh home2 docker save | ssh cas docker load` from a machine that can reach both.

## Rehearsal on home2 (passed, then removed)
- **Setup:** the real `deploy/cas/compose.yml` plus an override (local folders, Mailpit as postfix, `httpd:2.4` with the real Apache snippet on 127.0.0.1:18080), reached through an SSH tunnel.
- **Checks:**
  - `/archaeodash` redirects (301), `/archaeodash/healthz` returns 200, and icons and assets load under `/archaeodash/`;
  - registration sends a verification email through `smtp-local` with a link that includes `/archaeodash`;
  - verify, then login, then Projects; creating a project opens the Data Manager;
  - upload and import through `/archaeodash/w/<id>/` with CSRF;
  - PCA and UMAP;
  - k-means as a background job, with its event stream opening through Apache;
  - login cookies come back with `Path=/archaeodash/`;
  - the app logs that it trusts X-Forwarded-For from 127.0.0.1 and ::1.

## Deployed on cas (2026-10-29)
- **Access:** SSH from this PC timed out (off the ASU network). It worked through homepc, which can reach cas:
  `ssh -o ProxyCommand="ssh home2 ssh homepc -W %h:%p" cas`. The image went the same way: `docker save | gzip` on home2, through homepc, into `docker load` on cas.
- **Mail:** cas runs **sendmail** (not postfix) on 127.0.0.1:25 and :587, with `SMART_HOST smtp.asu.edu`. The September log shows relayed deliveries succeeding, so `smtp-local` to 127.0.0.1:25 fits.
- **Apache:** global headers are HSTS, X-Frame-Options SAMEORIGIN and a report-only CSP. They don't conflict with the app's own headers; HSTS and X-Frame-Options appear twice with the same values. No existing rule matches `/archaeodash`.
- **Installed:**
  - `/srv/archaeodash` (root, 700): `compose.yml`, `.env` (600, fresh random secrets, `ARCHAEODASH_TAG=cas-20261029`), `backup.sh`, `postgres/`, and `user-files/` (10001, 700).
  - Image `archaeodash-hosted:cas-20261029` (ID `be335294cb1d`, the one rehearsed).
  - Started with `sudo docker compose --project-directory /srv/archaeodash -f /srv/archaeodash/compose.yml up -d`. Use `--project-directory`, because the folder is root-only and `cd` fails. Migrations applied; the app listens on 127.0.0.1:18787; both containers are healthy.
  - `/etc/apache2/conf-available/archaeodash-location.conf`, included at line 102 of `sites-available/default-ssl.conf` (before `<Location /app/Archaeodash/>`). The backup of the original is `/root/default-ssl.conf.bak-20261029`. configtest passed and Apache reloaded.
  - `/etc/cron.d/archaeodash-backup`: 03:30 nightly, logging to `/var/log/archaeodash-backup.log`. It writes to `/mnt/storage/backups/archaeodash`, which is root-only (700). The first run was fine. `backup.sh` now sets `umask 077`, because the dump holds emails and password hashes.
- **Smoke test:**
  - `/archaeodash` returns 301; `/archaeodash/`, `/healthz`, `/projects` and `/api/v1/auth/session` return 200; assets load under `/archaeodash/`.
  - `/app/Archaeodash/`, `/heurist/` and `/` still return 200.
  - The sign-in page renders in a browser.
- **Redeploys:** build on home2 as `archaeodash-hosted:cas-<short sha>` (with `--build-arg ARCHAEODASH_BASE_PATH=/archaeodash`), ship it through homepc, set `ARCHAEODASH_TAG` in `/srv/archaeodash/.env`, then `up -d app`. Postgres keeps running.
  - 2026-10-29: `cas-804bb54` (sign-in page layout and terms links). App healthy; public pages return 200.
  - 2026-10-30: `cas-cfe6205` (anonymous guest sessions, see [[Hosted_Guest_Sessions_2026-10-29]]); compose file updated, backup `compose.yml.bak-cfe6205`. App healthy; public pages return 200.
  - 2026-10-30: `cas-4e0a443` (responsive multiplot pickers and pop-out plot windows, see [[Multiplot_Responsive_Pickers_And_Popout_Windows_2026-10-30]]). App healthy; public pages and `/archaeodash/multiplot-window/...` return 200.
  - 2026-10-30: `cas-557400a` (CSP allows `blob:` images so Plotly PNG export works, see [[Hosted_CSP_Blob_Images_For_PNG_Export_2026-10-30]]). App healthy; public pages return 200.
  - 2026-10-30: `cas-51c4b80` (project Trash, Sample IDs choice, combining groups with different columns; first deploy with `scripts/home2/deploy-cas.sh`, see [[Deploy_And_DB_Check_Wrappers_2026-10-30]]). App healthy; public pages return 200; `/api/v1/projects/trash` returns 401 without a session.
  - 2026-11-04: `cas-06d3294` (multiplot wraps single-axis panels into a grid, see [[Multiplot_Single_Axis_Grid_Wrap_2026-11-04]]; one-window-per-plot cap raised from 24 to 36). App healthy; public pages return 200; the live bundle has the cap at 36.
  - 2026-11-05: `cas-671c2ff` (one-window-per-plot windows tile across the screen, plus a Tile windows button, see [[Multiplot_Popout_Window_Tiling_2026-11-05]]). App healthy; public pages return 200.
  - 2026-11-05: `cas-2226f7b` (no app change: the image ID matches `cas-671c2ff`; redeployed so the tag matches HEAD after the desktop download was added, see [[Desktop_Download_On_Cas_2026-11-05]]). App healthy; public pages and the download return 200.
  - 2026-11-05: `cas-def3593` (pop-out windows show the plot first, see [Multiplot pop-outs show the plot first](Multiplot_Popout_Plot_First_Layout_2026-11-05.md)). App healthy; public pages return 200.
- **Not yet checked:** delivery of a real verification email; check `/var/log/mail.log` after the first sign-up.
- **Follow-ups:** add the URL to `security/live_healthcheck.sh`; commit the repo changes. Registration is open to anyone; quota 1 GB per user; 4 analysis slots.
