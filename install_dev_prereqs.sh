#!/usr/bin/env bash
#
# install_dev_prereqs.sh - bootstrap the developer environment for the
# Node.js/TypeScript + Rust migration described in IMPLEMENTATION.md.
#
# Installs the software required before implementation can begin:
#   --system (sudo apt):  build toolchain; Tauri 2 webkit/GTK system
#                        libraries (master IMPLEMENTATION.md section 1);
#                        PostgreSQL control plane (section 6.5); R runtime +
#                        dev headers for the section 15.1/15.4 oracle harness;
#                        Playwright/shinytest2 browser system dependencies
#                        (section 15.2); legacy MySQL client headers for the
#                        section 14 migrator (RMySQL); libsodium (auth crate).
#   --user   (no sudo):   rustup stable incl. rustfmt/clippy (section 15.3
#                        gates); pnpm (section 9.1); sqlx-cli (section 6.5 /
#                        17.1 item 12); tauri-cli; the current R package's
#                        DESCRIPTION dependencies + uvr companion via pak;
#                        Playwright chromium.
#   default:           both sections, skipping anything already present.
#   --check:           report present/missing software, install nothing.
#
# Ubuntu 24.04/22.04 (WSL or native). PostgreSQL may alternatively run as a
# Docker container (docker is present); MinIO for the S3 contract tests
# (section 17.1 item 6) is optional and is not installed here.
# Control-plane database/user creation is a Phase 1 sqlx migration task,
# not a prerequisite task.
#
# Idempotent: safe to re-run. Run from the repository root.

set -euo pipefail

cd "$(dirname "$0")"

CHECK_ONLY=0
SCOPE=all
for arg in "$@"; do
  case "$arg" in
    --check)  CHECK_ONLY=1 ;;
    --system) SCOPE=system ;;
    --user)   SCOPE=user ;;
    -h|--help) printf 'usage: %s [--check|--system|--user]\n' "$0"; exit 0 ;;
    *) printf 'usage: %s [--check|--system|--user]\n' "$0" >&2; exit 2 ;;
  esac
done

say()  { printf '\n==> %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }
apt_installed() { dpkg-query -W -f='${db:Status-Abbrev}' "$1" 2>/dev/null | grep -q 'ii'; }

# Pick up user-local tool paths (non-login shells miss ~/.cargo/bin and pnpm).
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export PATH="$HOME/.local/share/pnpm/bin:$PATH"

MISSING_TOOLS=""
MISSING_APT=""
check_tool() {
  if have "$1"; then printf '  present  %s\n' "$1"
  else printf '  MISSING  %s\n' "$1"; MISSING_TOOLS="$MISSING_TOOLS $1"; fi
}
check_apt() {
  if apt_installed "$1"; then printf '  present  %s (apt)\n' "$1"
  else printf '  MISSING  %s (apt)\n' "$1"; MISSING_APT="$MISSING_APT $1"; fi
}

# --- shared system-package list (sudo apt) ---------------------------------
SYSTEM_APT=(build-essential pkg-config curl wget file ca-certificates
            libssl-dev libpq-dev
            postgresql postgresql-contrib postgresql-client
            r-base r-base-dev
            libsodium-dev libmariadb-dev libcurl4-openssl-dev)

. /etc/os-release
case "${VERSION_ID:-}" in
  24.*|23.*) WEBKIT=libwebkit2gtk-4.1-dev ;;
  22.*)     WEBKIT=libwebkit2gtk-4.0-dev ;;
  *)        WEBKIT="" ;;
esac
if [ -n "$WEBKIT" ]; then
  SYSTEM_APT+=("$WEBKIT" libgtk-3-dev librsvg2-dev libxdo-dev
               libayatana-appindicator3-dev)
else
  say "WARNING: unsupported Ubuntu ${VERSION_ID:-unknown}; pick the Tauri webkit2gtk dev package (4.1 for 24.04, 4.0 for 22.04) manually"
fi

if [ "$SCOPE" = system ] || [ "$SCOPE" = all ]; then
  say "System packages (sudo apt)"
  if [ "$CHECK_ONLY" = 1 ]; then
    for p in "${SYSTEM_APT[@]}"; do check_apt "$p"; done
    if have node && have npx; then
      echo "  note: browser system deps install via: sudo npx --yes playwright install-deps chromium"
    else
      echo "  MISSING  playwright browser system deps (install node first, then run: sudo npx --yes playwright install-deps chromium)"
      MISSING_TOOLS="$MISSING_TOOLS playwright-install-deps"
    fi
  else
    need=()
    for p in "${SYSTEM_APT[@]}"; do apt_installed "$p" || need+=("$p"); done
    # Current CRAN packages (e.g. MASS, Deriv) require R >= 4.4/4.5, which
    # Ubuntu's stock r-base (4.3.x on 24.04) does not provide; enable the CRAN
    # apt repository so r-base resolves to a current R. Idempotent.
    R_TOO_OLD=1
    if have Rscript; then
      Rscript --vanilla -e 'quit(status=as.integer(getRversion() < "4.4.0"))' && R_TOO_OLD=0 || true
    fi
    if [ ! -f /etc/apt/sources.list.d/cran-r.list ] || [ "$R_TOO_OLD" = 1 ]; then
      say "R >= 4.4 not detected; enabling the CRAN apt repository"
      sudo apt-get install -y --no-install-recommends dirmngr ca-certificates wget
      wget -qO- https://cloud.r-project.org/bin/linux/ubuntu/marutter_pubkey.asc \
        | sudo gpg --dearmor -o /usr/share/keyrings/r-project.gpg
      echo "deb [signed-by=/usr/share/keyrings/r-project.gpg] https://cloud.r-project.org/bin/linux/ubuntu ${VERSION_CODENAME:-noble}-cran40/" \
        | sudo tee /etc/apt/sources.list.d/cran-r.list > /dev/null
    fi
    # An installed-but-outdated r-base is not in need[]; upgrade it explicitly.
    if [ "$R_TOO_OLD" = 1 ]; then
      need+=("r-base" "r-base-dev")
    fi
    if [ "${#need[@]}" -gt 0 ]; then
      sudo apt-get update
      sudo DEBIAN_FRONTEND=noninteractive apt-get install -y "${need[@]}"
    else
      echo "  all system packages present"
    fi
    # Playwright/shinytest2 browser system dependencies (section 15.2); invokes sudo apt itself.
    if have node && have npx; then
      if sudo -n true 2>/dev/null; then
        npx --yes playwright install-deps chromium \
          || echo "  WARNING: playwright install-deps failed; re-run: sudo npx --yes playwright install-deps chromium"
      else
        echo "  SKIPPED playwright install-deps (needs sudo password); run: sudo npx --yes playwright install-deps chromium"
      fi
    fi
    # Control-plane service: enable now if possible; sqlx migrations are Phase 1.
    if have pg_isready; then
      sudo systemctl enable --now postgresql 2>/dev/null \
        || sudo service postgresql start 2>/dev/null || true
      if pg_isready -q; then echo "  postgresql: ready"
      else echo "  postgresql: installed but not ready; start it manually or run it in Docker"; fi
    fi
  fi
fi

if [ "$SCOPE" = user ] || [ "$SCOPE" = all ]; then
  say "User-level toolchain (no sudo)"
  if [ "$CHECK_ONLY" = 1 ]; then
    check_tool git
    check_tool node
    check_tool npm
    check_tool corepack
    check_tool pnpm
    check_tool rustup
    check_tool rustc
    check_tool cargo
    check_tool sqlx
    check_tool cargo-tauri
    check_tool Rscript
    check_tool psql
    check_tool pg_isready
    if [ -d "$HOME/.cache/ms-playwright" ] && [ -n "$(ls -A "$HOME/.cache/ms-playwright" 2>/dev/null)" ]; then
      echo "  present  playwright browsers (~/.cache/ms-playwright)"
    else
      echo "  MISSING  playwright browsers (~/.cache/ms-playwright)"
      MISSING_TOOLS="$MISSING_TOOLS playwright-browsers"
    fi
    if have docker; then
      echo "  present  docker (optional: MinIO for S3 contract tests can run as a container)"
    else
      echo "  absent   docker (optional MinIO/S3 backend)"
    fi
  else
    say "rustup stable + rustfmt + clippy (section 15.3 release gates)"
    if have rustup; then echo "  rustup present"
    else
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile default --default-toolchain stable
    fi
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"

    say "pnpm (section 9.1 workspace manager)"
    if have pnpm; then echo "  pnpm present"
    else
      curl -fsSL https://get.pnpm.io/install.sh | sh -
    fi
    export PATH="$HOME/.local/share/pnpm/bin:$PATH"

    say "sqlx-cli (section 6.5 control-plane migrations, section 17.1 item 12)"
    if ! have sqlx; then
      cargo install sqlx-cli --locked \
        || echo "  WARNING: sqlx-cli install failed; re-run: cargo install sqlx-cli --locked"
    fi

    say "tauri-cli (section 1 desktop adapter)"
    if ! have cargo-tauri; then
      cargo install tauri-cli --locked \
        || echo "  WARNING: tauri-cli install failed; re-run: cargo install tauri-cli --locked"
    fi

    say "R oracle dependencies (DESCRIPTION Suggests/Imports/Depends + uvr companion)"
    if have Rscript; then
      # Ensure R's user library exists so installs never target the
      # sudo-protected system site-library (R only uses R_LIBS_USER if it exists).
      RULIB="$(Rscript --vanilla -e 'cat(file.path(path.expand("~/R"), paste0(R.version$platform, "-library"), paste0(R.version$major, ".", R.version$minor)))' 2>/dev/null)"
      mkdir -p "$RULIB"
      export R_LIBS_USER="$RULIB"
      echo "  R user library: $RULIB"
      Rscript -e 'if (!requireNamespace("pak", quietly=TRUE)) install.packages("pak", repos="https://cloud.r-project.org")' \
        || echo "  WARNING: pak install failed"
      Rscript -e 'deps <- read.dcf("DESCRIPTION")[1, c("Depends","Imports","Suggests")]; v <- unlist(strsplit(paste(na.omit(deps), collapse=","), "[[:space:]]*,[[:space:]]*")); v <- sub(" *\\(.*$", "", v); v <- setdiff(trimws(v), c("", "R")); cat("installing", length(v), "R packages via pak\n"); pak::pak(v)' \
        || echo "  WARNING: some R packages failed; see output above"
      Rscript -e 'if (!requireNamespace("uvr", quietly=TRUE)) pak::pak("nbafrank/uvr-r")' \
        || echo "  WARNING: uvr companion install failed"
      Rscript -e 'ip <- rownames(installed.packages()); need <- c("data.table","cluster","DT","ggplot2","plotly","shiny","bslib","bsicons","shinyjs","dplyr","tidyr","tibble","tidyselect","purrr","stringr","glue","magrittr","janitor","rio","later","promises","MASS","mice","umap","factoextra","cowplot","dendextend","ICSNP","candisc","sodium","DBI","RMySQL","curl","testthat","shinytest2"); miss <- setdiff(need, ip); if (length(miss)) cat("R packages still missing:", paste(miss, collapse=" "), "\n") else cat("all R oracle packages present\n")'
    else
      echo "  Rscript absent; run --system first (r-base), then --user again"
    fi

    say "Playwright chromium (section 15.2 web/desktop e2e)"
    if have node && have npx; then
      npx --yes playwright install chromium \
        || echo "  WARNING: chromium download failed; re-run: npx --yes playwright install chromium"
    else
      echo "  node absent; install browsers at Phase 1 with the pinned @playwright/test version"
    fi
  fi
fi

if [ "$CHECK_ONLY" = 1 ]; then
  say "Summary"
  if [ -n "$MISSING_APT" ];   then echo "missing apt packages:  $MISSING_APT";   fi
  if [ -n "$MISSING_TOOLS" ]; then echo "missing tools:        $MISSING_TOOLS"; fi
  if [ -z "$MISSING_APT$MISSING_TOOLS" ]; then echo "all tracked prerequisites present"; fi
  exit 0
fi

say "Done. Re-run with --check to verify."
