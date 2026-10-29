Up: [[Index]]

# Deploying ArchaeoDash (Rust/React) on cas — recommendation (2026-10-28)

Investigation only: nothing was installed, changed or restarted on cas, and no code changed.

## Access
- `ssh cas` now works from the Windows PC. The `Host cas` entry and key came from homepc's `~/.ssh` (fetched through home2):
  - host `collectiveaccess.rc.asu.edu`, user `rjbischo`;
  - key `~/.ssh/id_ed25519_cas`, with the file ACL limited to the user;
  - homepc's known_hosts lines for cas, whose ED25519 host key is `SHA256:xOipMr+B9zB/7Gk3/9jiQJHsBKtwXxFa6JD4/mDvg7w`.
- SSH (22) only answers from the ASU network or VPN; it times out from off campus.

## What cas is (surveyed 2026-10-28)
- **OS and hardware:**
  - Ubuntu 24.04.5 LTS, kernel 6.8, a Xen/Hyper-V VM, up 249 days;
  - 16 vCPU (Xeon Gold 6230), 31 GiB RAM (about 25 GiB available) and 4 GiB swap;
  - `/` is a 250 GB ext4 volume with 142 GB free;
  - `/mnt/storage` is an NFSv3 share from `horizon.rc.asu.edu:/collectiveaccess_vm` with 1.9 PB free. All apps live there (`/mnt/storage/apps/...`, `/mnt/storage/docker/...`).
- **User:** `rjbischo` (uid 1001) is in the `sudo` and `docker` groups and has `NOPASSWD: ALL`.
- **Runtimes:**
  - Docker 28.0.1 and Compose v2.33.1 (data in `/var/lib/docker`, overlay2); no Podman;
  - Node 24.22 and npm 10.9; no Rust/cargo and no pnpm;
  - R/Rscript; the MySQL client and server.
- **Web and TLS:**
  - Apache 2 with a single HTTPS vhost (`default-ssl.conf`, `cas.rc.asu.edu`; `collectiveaccess.rc.asu.edu` redirects to it). Port 80 redirects to HTTPS.
  - Apps are published as **sub-paths** with `ProxyPass`: `/app/Archaeodash/` → :13838, `/app/Archaeodashbeta/` → :23838, `/app/barcodes/` → :53838, `/heurist/` and `/h7-alpha/` → :8080, `/adminapi/` → :8000 (the CASdbAPI Plumber service).
  - Headers set globally: HSTS (includeSubDomains), X-Frame-Options, Referrer-Policy, Permissions-Policy and a report-only CSP.
  - Modules loaded: proxy, proxy_http, proxy_wstunnel, headers, rewrite, ssl, auth_basic. No Shibboleth or CAS SSO module.
  - Certbot timer is active. Certificates: `cas.rc.asu.edu` (SAN collectiveaccess, expires 2027-01-14) and `collectiveaccess.rc.asu.edu` (expires 2026-12-13).
- **Compose projects:**
  - `casrshinysrvr` (`/mnt/storage/apps/CASRShinySrvr/docker-compose.yml`) runs Shiny `archaeodash`, `archaeodashbeta` and `barcodes`.
  - `heurist` (web plus mysql:8.0).
  - `archaeodashjs` (`.../CASRShinySrvr/archaeodashjs`): `archaeodash-frontend` on :4173 and `archaeodash-backend` on :8011, an older JS/Plumber port. It had been up 7 months and was **stopped 2026-10-28**. Apache has no route to it, and its frontend was built with `VITE_API_BASE_URL=http://localhost:8011`, so it is effectively dead.
- **systemd services:** apache2, docker, mysql, php8.3-fpm, redis, casdbapi (Plumber), postfix/sendmail (local only, 127.0.0.1:25 and :587), nagios-nrpe, unattended-upgrades.
- **Ports and firewall:**
  - Host firewall (ufw) is inactive and iptables INPUT is set to ACCEPT. Docker publishes 13838, 23838, 53838, 4173 and 8011 on `0.0.0.0`.
  - The ASU perimeter blocks those ports from the campus PC and from off campus (13838, 23838, 4173, 8011 and 3306 all time out from off campus).
  - **Exception:** heurist's MySQL `10.126.24.122:3306` *is* reachable from the campus network.
  - Free choices for a new app: 127.0.0.1:18787 or anything not in use.
- **Public reach:** 80 and 443 are public (206.207.50.96 in public DNS; 10.126.24.122 inside ASU).

## What in the repo can run as a server
- **`crates/api` has two separate routers:**
  - `root_router` (`crates/api/src/lib.rs:805`) holds every analysis endpoint the web UI uses: groups, imports, jobs (including SSE `/api/v1/jobs/{id}/events`), ordination, cluster, explore, exports and preferences. It serves one project folder and has **no auth**. The only way to start it is `examples/local.rs` (`LOCAL_API_ADDR`, default 127.0.0.1:8787).
  - `hosted_router` (`crates/api/src/hosted.rs:85`, binary `src/bin/hosted.rs`) has accounts (register, verify, login, CSRF), projects, per-user files, quota and `/api/v1/health/*`. It needs Postgres, `AUTH_PEPPER`, `AUTH_BASE_URL` and email. It has **no analysis or group routes**, and IMPLEMENTATION.md lists the hosted data plane as unfinished.
- **Web UI (`apps/web`):** outside Tauri it uses `HttpTransport('')`, so it calls same-origin `/api/v1/...`. `vite build` produces `dist/`. Vite `base` is unset and `index.html` and the API calls use root paths.
- **Container recipe (`deploy/container/Dockerfile`, `deploy/compose/hosted-stack.yml`):** this builds only the `hosted` binary, and it probably fails as written:
  - it pins Rust 1.82, but the toolchain is 1.98.1;
  - it copies only `crates/`, but the workspace also lists `apps/desktop/src-tauri` and `tests/parity`;
  - the healthcheck uses `wget`, but the image installs only `curl`.
- **Tooling:** `scripts/home2/` builds on home2, and `preview.sh` builds and serves `dist/`. There is no server-image script.

## Recommendation

### 1. What to deploy
Deploy the analysis API (`root_router`) and the static web UI together as **one service**, as a staging instance with one shared project. Keep the production Shiny app at `/app/Archaeodash/`. The `hosted` binary is not useful on its own yet, because nothing in it runs analysis. A public multi-user version needs the hosted data plane first.

### 2. How to run it
- **Use a container**, matching how everything else on cas runs. cas has no Rust and should not build Rust itself: builds would compete with production Shiny, heurist and CollectiveAccess.
- Build the image on home2 and copy it with `docker save | ssh cas docker load`. Alternatively, push to GHCR under the CAS org using the CAS PAT.
- Give it its own compose project, for example `/mnt/storage/apps/ArchaeodashRust/compose.yml`, next to the other apps. Use `restart: unless-stopped` (Docker runs under systemd, so no separate unit is needed).
- Publish the port on **127.0.0.1 only**, for example `127.0.0.1:18787:8080`. The existing apps publish on 0.0.0.0, which works only because the perimeter firewall blocks those ports.
- Set limits such as `cpus: 8` and `mem_limit: 12g`.
- Do not reuse the `casrshinysrvr` compose file, and do not use `deploy.sh`.

### 3. Reverse proxy and TLS
- Use the existing pattern: a **sub-path** on the `cas.rc.asu.edu` vhost, for example `/app/ArchaeodashNext/`, with `ProxyPass` to 127.0.0.1:18787. TLS and certbot then stay exactly as they are. The cost is a small code change: Vite `base` plus an API base derived from `import.meta.env.BASE_URL`, plus root-relative icon links.
- **Alternative:** a new host name. That needs ASU DNS, a new vhost and `certbot --expand`, but no code change.
- Add to the new `<Location>`:
  - `LimitRequestBody` sized for spreadsheet uploads (the Shiny paths use 0, meaning unlimited);
  - unbuffered SSE for `/api/v1/jobs/*/events` (`flushpackets=on`, and `ProxyTimeout` of at least 300);
  - **authentication.** `auth_basic` is the only module available now. Use Basic auth with an htpasswd file outside the repo, or ask RC whether Shibboleth or CAS SSO can be enabled.
- Run `apachectl configtest` before reloading. The vhost is shared with every other app, so a syntax error takes them all down.

### 4. Where project data lives
- The default is to put it next to the other apps on the NFS share: `/mnt/storage/apps/ArchaeodashRust/projects/<name>`, bind-mounted read-write. `/mnt/storage` has mode 777 and existing app files belong to 1001:1002. Run the container as `user: "1001:1002"`, or `chown` the folder to 10001.
- **NFS caveat:** the project lock (`project.lock`, shared and exclusive `flock`) goes over NFSv3/NLM. `rpc-statd` is running, so it should work. Test it before trusting it: run two writers, then kill one mid-write. The fallback is local disk at `/srv/archaeodash-next/projects` (142 GB free), with backups added.
- Check with RC whether Horizon NFS is backed up or snapshotted. If it isn't, schedule `deploy/backup.sh`-style copies.

### 5. Gaps and risks
- **No auth in the analysis API.** It depends entirely on Apache auth and the localhost-only port. Anyone with the Apache login can read, overwrite or delete any group.
- **No production binary for `root_router`.** One is needed with:
  - bind address and project folder settings from env;
  - JSON logs and graceful shutdown;
  - security headers;
  - body limits;
  - `ServeDir` for `dist/`;
  - a health route for the healthcheck.
- **Shared project.** Users see and overwrite each other's work. The project lock serialises writes, but job state and caches live in one process, so run one replica only.
- **Hosted mode is incomplete:** there are no analysis routes. For email, the host's postfix listens only on 127.0.0.1, so a container would need SMTP to an ASU relay, or postfix opened to the Docker bridge. Postgres would need backups. Shiny accounts are not migrated.
- **The container recipe needs fixing** before the first image: the Rust 1.82 vs 1.98.1 mismatch, missing workspace members, and wget vs curl.
- **The shared Apache vhost** is a single point of failure for every app on cas.
- **Found while surveying (outside this task):**
  - heurist's MySQL 3306 is open to the ASU network;
  - the stale `archaeodashjs` stack (:4173, :8011) was **stopped on 2026-10-28** (restart policy `no`, so it stays down after reboot). Its containers, images and the `archaeodashjs_archaeodash_data` volume are kept; restart with `docker compose start` in `/mnt/storage/apps/CASRShinySrvr/archaeodashjs`;
  - the firewall is off and depends on the perimeter;
  - the `collectiveaccess.rc.asu.edu` certificate is separate from the combined one and expires 2026-12-13.

### 6. Next steps
1. Decide: staging behind Basic auth (recommended now), or wait for the public hosted product.
2. Repo work, built on home2:
   - a server binary based on `examples/local.rs` that serves `dist/` and `/api`;
   - the sub-path support in `apps/web`;
   - fix `deploy/container/Dockerfile` (toolchain 1.98.1, workspace members, curl healthcheck, Node stage for `vite build`);
   - `deploy/compose/staging.yml`;
   - a `scripts/home2/build-server-image.sh` wrapper.
3. On cas, after approval:
   - create `/mnt/storage/apps/ArchaeodashRust/` with the compose file and data folder;
   - `docker load` the image and run `docker compose up -d`;
   - test NFS locking;
   - add the `/app/ArchaeodashNext/` `ProxyPass` and `<Location>` with auth and SSE settings;
   - `apachectl configtest`, then `systemctl reload apache2`.
4. Add the new URL to `security/live_healthcheck.sh` and to backups.
5. Raise separately: closing 3306 to the campus network, deleting the stopped `archaeodashjs` containers and volume if nobody needs them, and the hosted data plane before the Shiny cutover.

Related: [[Deploy_Script_Docker_Compose_Beta_2026-05-01]], [[Production_App_Promotion_2026-07-01]], [[Live_Healthcheck_Timer_Setup_2026-02-21]], [[Home2_Build_Wrappers_2026-10-23]].
