Up: [[Index]]

# Windows desktop download on cas (2026-11-05)

The Windows desktop app is published at **https://cas.rc.asu.edu/archaeodash/download/ArchaeoDash.exe** (stable name), with versioned copies `ArchaeoDash-<short sha>.exe` and `SHA256SUMS` next to it.

## How to publish a new build
1. `bash scripts/home2/build-windows.sh`: cross-compiles on home2 from the share (rsynced to the home2 mirror); about 6 min.
2. `bash scripts/home2/deploy-desktop-cas.sh` (no arguments):
   - uploads the build through homepc and checks its sha256;
   - installs it in `/var/www/archaeodash-downloads` (root, 755; files 644) and keeps the three newest versioned builds;
   - installs `deploy/cas/apache-archaeodash.conf` only if it matches the installed copy outside the `# BEGIN download` / `# END download` blocks (blank lines ignored). It backs up to `/root/archaeodash-location.conf.bak-<time>`, needs `apachectl configtest` to pass before reloading, and restores the backup otherwise;
   - checks the download (status, size, sha256) and the public pages.
- The version label is the home2 repository's HEAD when the script runs. Publish before committing anything new, or the label will not match the build.
- To run both without a prompt, add them to `.claude/settings.local.json` (not done by the agent).

## Apache (shared cas vhost)
- `Alias /archaeodash/download/ /var/www/archaeodash-downloads/`. The `<Directory>` has `Options None` (no listing: `/download/` returns 403), `ForceType application/octet-stream`, `Content-Disposition: attachment` and `nosniff`.
- `<Location /archaeodash/download/> ProxyPass ! </Location>` comes after the `/archaeodash/` proxy section, so these files are not proxied to the app.
- After the change, `/archaeodash/`, `/projects`, `/healthz`, `/app/Archaeodash/`, `/heurist/` and `/` still return 200.

## First publish
- `ArchaeoDash-ca98b59.exe` (34,414,592 bytes, sha256 `ef07e7cc…32798`): includes the multiplot window tiling from [[Multiplot_Popout_Window_Tiling_2026-11-05]].
- The exe is unsigned, so Windows SmartScreen warns on first run. It needs the WebView2 runtime (included with Windows 10/11).
- Not yet linked from the web app's pages.
