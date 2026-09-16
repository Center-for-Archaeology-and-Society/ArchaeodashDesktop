#!/usr/bin/env bash
set -euo pipefail
ORACLE="$HOME/.local/share/archaeodash-r-oracle/archaeodash-r-oracle"
export R_LIBS_USER="$ORACLE/.uvr/library"
Rscript -e '
ns <- asNamespace("janitor")
print(get("clean_names.default", envir = ns))
cat("===== make_clean_names =====\n")
print(get("make_clean_names", envir = ns))
cat("===== handle_if_special_names_used =====\n")
print(get("handle_if_special_names_used", envir = ns))
' 2>&1 | head -220
