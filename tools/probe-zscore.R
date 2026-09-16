#!/usr/bin/env bash
set -euo pipefail
ORACLE="$HOME/.local/share/archaeodash-r-oracle/archaeodash-r-oracle"
export R_LIBS_USER="$ORACLE/.uvr/library"
Rscript -e '
suppressPackageStartupMessages({library(dplyr)})
options(digits=17)
zScore = function(x){ as.data.frame(scale(prop.table(as.matrix(x), 1) * 100)) %>% dplyr::mutate_all(round,3) }
x <- matrix(c(1,2,NA,4, 4,5,6,8), nrow=4)
z <- zScore(x)
cat("col1:", sprintf("%.17g", z[,1]), "\n")
cat("col2:", sprintf("%.17g", z[,2]), "\n")
x2 <- matrix(c(0,2,NA,4, 0,5,6,8), nrow=4)
z2 <- zScore(x2)
cat("zero-rowsum col1:", sprintf("%.17g", z2[,1]), "\n")
'
