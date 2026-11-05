---
type: Playbook
title: Desktop downloads on cas
description: How the Windows and Linux desktop builds are built on home2 and published at per-platform download paths on cas.
status: stable
generated:
  by: claude-code/claude-opus-5-5
  at: 2026-11-05T17:40:00+00:00
sources:
  - id: deploy-script
    resource: ../scripts/home2/deploy-desktop-cas.sh
  - id: linux-build
    resource: ../scripts/home2/build-linux.sh
  - id: apache-conf
    resource: ../deploy/cas/apache-archaeodash.conf
---

Up: [Index](Index.md)

# Desktop downloads on cas

Each platform has its own download folder on cas.[^deploy-script]

| Platform | Stable link |
|---|---|
| Windows | https://cas.rc.asu.edu/archaeodash/download/windows/ArchaeoDash.exe |
| Linux (any distro) | https://cas.rc.asu.edu/archaeodash/download/linux/ArchaeoDash.AppImage |
| Linux (Debian/Ubuntu) | https://cas.rc.asu.edu/archaeodash/download/linux/ArchaeoDash.deb |

Each folder also holds versioned copies `ArchaeoDash-<short sha>.<ext>` (the three newest are kept) and a `SHA256SUMS` file. The old root link `/archaeodash/download/ArchaeoDash.exe` redirects (301) to the Windows path.[^apache-conf]

## How to publish a new build
1. Build. Both scripts take no arguments, and each writes a `BUILD_INFO` file (commit and dirty count) next to its output:
   - `bash scripts/home2/build-windows.sh`: cross-compiles on home2 (about 5 minutes when cached).
   - `bash scripts/home2/build-linux.sh`: builds in an Ubuntu 22.04 Docker container on home2, so the result runs on glibc 2.35 or newer. It outputs an AppImage and a .deb.[^linux-build] The image is rebuilt only when the Dockerfile or the node/rust versions change (about 15 minutes); a cached compile takes about 13 minutes.
   - Run the two builds one after the other, not at the same time: they share home2's build folders.
2. Publish with `bash scripts/home2/deploy-desktop-cas.sh [all|windows|linux]` (default `all`).[^deploy-script] The script:
   - takes the version label from `BUILD_INFO`, so committing after a build does not mislabel it;
   - uploads through homepc and checks sha256 on cas;
   - installs each file into `/var/www/archaeodash-downloads/<platform>/`;
   - installs the Apache config only if it matches the installed copy outside the `# BEGIN download` / `# END download` blocks. It backs up first, needs `apachectl configtest` to pass, and rolls back on failure;
   - checks each download (status, size, sha256), the old-link redirect and the public pages.

## Apache (shared cas vhost)
- `Alias /archaeodash/download/ /var/www/archaeodash-downloads/` with `Options None` (no listing), `ForceType application/octet-stream`, `Content-Disposition: attachment` and `nosniff`.[^apache-conf]
- `<Location /archaeodash/download/> ProxyPass ! </Location>` comes after the app proxy, so the downloads are not sent to the app.

## Current builds (observed 2026-11-05)
All built from commit `aab9438` with no uncommitted changes, and published at 17:31 UTC. The script's public checks passed for all three files.

| File | Bytes | sha256 (start) |
|---|---|---|
| ArchaeoDash.exe | 34,414,592 | `dc1b7ca3…` |
| ArchaeoDash.AppImage | 92,932,600 | `52493a06…` |
| ArchaeoDash.deb | 15,724,736 | `6d8c9c44…` |

These include the multiplot window tiling described in [Multiplot pop-out window tiling](Multiplot_Popout_Window_Tiling_2026-11-05.md).

## Limitations
- Builds are unsigned. Windows SmartScreen warns on first run, and Windows needs the WebView2 runtime (included with Windows 10/11).
- The AppImage needs `chmod +x` before running. Its runtime needs FUSE 2; on systems without it, run it with `--appimage-extract-and-run`.
- The Linux builds were not launched on a desktop as part of this check.
- The downloads are not yet linked from the web app's pages.

[^deploy-script]: [deploy-desktop-cas.sh](../scripts/home2/deploy-desktop-cas.sh)
[^linux-build]: [build-linux.sh](../scripts/home2/build-linux.sh) and [Dockerfile](../deploy/desktop-linux/Dockerfile)
[^apache-conf]: [apache-archaeodash.conf](../deploy/cas/apache-archaeodash.conf)
