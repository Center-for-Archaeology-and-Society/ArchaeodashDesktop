Up: [[Interaction_Log]]

# Interaction log 2026-11-05

- Asked whether multiplot pop-out windows can be arranged; then "implement and deploy". One-window-per-plot now tiles across the screen on open, with a "Tile windows" button to re-arrange (browser and desktop). See [[Multiplot_Popout_Window_Tiling_2026-11-05]]. Deployed to cas as `cas-671c2ff`.
- Asked for a desktop build deployed to cas, using scripts. Built the Windows exe on home2 and added `scripts/home2/deploy-desktop-cas.sh`; it is published at https://cas.rc.asu.edu/archaeodash/download/ArchaeoDash.exe (Apache download rule installed with configtest and a backup). See [[Desktop_Download_On_Cas_2026-11-05]].
- Asked to deploy the web version to cas too. No app code had changed since `cas-671c2ff` (the new image has the same ID), but redeployed as `cas-2226f7b` with the existing `deploy-cas.sh`; no new script needed.
