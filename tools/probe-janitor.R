#!/usr/bin/env bash
set -euo pipefail
ORACLE="$HOME/.local/share/archaeodash-r-oracle/archaeodash-r-oracle"
export R_LIBS_USER="$ORACLE/.uvr/library"
Rscript -e '
suppressPackageStartupMessages(library(janitor))
cat("dedupe1:", janitor::make_clean_names(c("as","as","as"), case="none"), "\n")
cat("dedupe2:", janitor::make_clean_names(c("as","as_2","as"), case="none"), "\n")
cat("edges:", janitor::make_clean_names(c("", "2020 data", "% off", "#num", "  lead", "if", "a  b", "a...b", "ANID"), case="none"), "\n")
cat("nan_numeric:", as.numeric("NaN"), " is.na:", is.na(as.numeric("NaN")), "\n")
cat("fmt:", as.character(3.784), "|", as.character(15587.9), "|", as.character(0.6452), "\n")
'
