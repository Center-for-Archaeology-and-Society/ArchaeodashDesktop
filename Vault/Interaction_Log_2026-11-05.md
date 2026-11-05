Up: [[Interaction_Log]]

# Interaction log 2026-11-05

- Asked whether multiplot pop-out windows can be arranged; then "implement and deploy". One-window-per-plot now tiles across the screen on open, with a "Tile windows" button to re-arrange (browser and desktop). See [[Multiplot_Popout_Window_Tiling_2026-11-05]]. Deployed to cas as `cas-671c2ff`.
- Asked for a desktop build deployed to cas, using scripts. Built the Windows exe on home2 and added `scripts/home2/deploy-desktop-cas.sh`; it is published at https://cas.rc.asu.edu/archaeodash/download/ArchaeoDash.exe (Apache download rule installed with configtest and a backup). See [[Desktop_Download_On_Cas_2026-11-05]].
- Asked to deploy the web version to cas too. No app code had changed since `cas-671c2ff` (the new image has the same ID), but redeployed as `cas-2226f7b` with the existing `deploy-cas.sh`; no new script needed.
- Asked for Linux and Windows builds deployed through cas at different paths. Added a Linux build in an Ubuntu 22.04 container on home2 (AppImage and .deb), and moved the downloads to `/archaeodash/download/windows/` and `/archaeodash/download/linux/`. The old exe link now redirects. All three files were published from `aab9438`, and the public checks passed. See [Desktop downloads on cas](Desktop_Download_On_Cas_2026-11-05.md).
- Asked to commit and push the user's AGENTS.md edits (the Vault rules rewritten for OKF v0.2). Pushed as `aab9438`.
- Asked to place the plot at the very top of pop-out windows with everything else below, then update all builds and deploy. Done in `def3593`. Web deployed as `cas-def3593`; Windows and Linux downloads republished. See [Multiplot pop-outs show the plot first](Multiplot_Popout_Plot_First_Layout_2026-11-05.md).
