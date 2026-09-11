# Install Dev Prereqs Script (2026-09-10)

## Summary

Created [[../../install_dev_prereqs.sh]], an idempotent bootstrap script for the developer environment required by [[../Node_Rust_Migration_Implementation_Plan_2026-09-08]] / [[../IMPLEMENTATION]] Phase 0. Verified green on the dev WSL (Ubuntu 24.04) machine.

## What it does

- `--system` (sudo apt): build toolchain, Tauri 2 webkit/GTK libs (§1), PostgreSQL control plane (§6.5), R runtime + headers (§15.1/15.4 oracle), Playwright/shinytest2 browser deps (§15.2), MariaDB client headers (§14 RMySQL), libsodium (auth crate).
- `--user` (no sudo): rustup stable + rustfmt/clippy (§15.3 gates), pnpm (§9.1), sqlx-cli (§6.5/§17.1 item 12), tauri-cli, DESCRIPTION R dependencies + `uvr` via pak, Playwright chromium.
- `--check`: report present/missing only, install nothing.
- R package list is parsed mechanically from `DESCRIPTION` (Depends/Imports/Suggests), not hardcoded twice — matches the §17.1 item 1 "capture mechanically" principle.

## Bugs found and fixed during verification

1. R dependency split regex was `,[[:space:]]+` but fields were joined with bare `,` — glued tokens like `data.table,cluster` broke pak parsing. Fixed to `[[:space:]]*,[[:space:]]*`.
2. R user library did not exist, so `install.packages` targeted the unwritable system site-library and failed. Fixed with `mkdir -p` + `R_LIBS_USER` export.
3. Ubuntu 24.04 stock R is 4.3.3; current CRAN `MASS` needs R ≥ 4.4 (and `Deriv` ≥ 4.5 in the `factoextra` chain), so dependency resolution failed. The script now enables the CRAN apt repo (`noble-cran40`, signed key) when R < 4.4 and force-upgrades `r-base`/`r-base-dev` even when already installed (an installed-but-outdated package never appears in the `apt-get install` list — that skip was itself a bug).
4. `--check` mode now sources `~/.cargo/env` and pnpm's PATH so installed user tools are not reported MISSING in non-login shells; pnpm path corrected to `~/.local/share/pnpm/bin`.
5. Two message fixes: contradictory "node/npx absent; they are present" wording; `playwright install-deps` sudo-password skip no longer reported as a failure.

## Environment state after run (2026-09-10)

- R upgraded 4.3.3 → 4.6.1 (CRAN repo); all 34 R oracle packages + `uvr` installed in `~/R/x86_64-pc-linux-gnu-library/4.6.1`.
- rustup/rustc/cargo, sqlx-cli 0.9.0, tauri-cli 2.11.4, pnpm 12.3.4, Playwright chromium installed.
- `--check` reports "all tracked prerequisites present".

## Related

- [[../IMPLEMENTATION]] (Phase 0)
- [[Node_Rust_Migration_Implementation_Plan_2026-09-08]]
- [[Uvr_GitHub_Install_Correction_2026-04-30]] (uvr installs from GitHub `nbafrank/uvr-r`, not CRAN — the script follows this)
- [[Interaction_Log_2026-09-10]]
