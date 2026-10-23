# Captures the distribution of legacy mice imputations (IMPLEMENTATION.md
# Section 8.3, parity class D) for the Rust port's distribution tests.
#
# The eight golden elements of fixtures/INAA_test.csv are restricted to
# complete rows; 8% of cells are then masked (MCAR, fixed seed) so the true
# values are known. Each legacy method is run 100 times with seeds 1..100
# (m = 1, maxit = 5, mice defaults otherwise; the legacy app used the first
# of mice's chains, and every chain has the same distribution).
#
# Usage: Rscript scripts/capture_imputation_distribution.R [output.json]
suppressPackageStartupMessages({
  library(mice)
  library(jsonlite)
})

args <- commandArgs(trailingOnly = TRUE)
out <- if (length(args) > 0) args[[1]] else "fixtures/golden/05_imputation_distribution.json"
cols <- c("as", "la", "lu", "nd", "sm", "u", "yb", "ce")
seeds <- 1:100
methods <- c("pmm", "midastouch", "rf")

raw <- read.csv("fixtures/INAA_test.csv", check.names = FALSE)
x <- raw[, cols]
x[] <- lapply(x, as.numeric)
x <- x[stats::complete.cases(x), , drop = FALSE]
rownames(x) <- NULL

set.seed(20261023)
mask <- matrix(stats::runif(nrow(x) * ncol(x)) < 0.08, nrow = nrow(x))
masked <- x
masked[mask] <- NA
cells <- which(mask, arr.ind = TRUE)
cells <- cells[order(cells[, "col"], cells[, "row"]), , drop = FALSE]
truth <- as.matrix(x)[cells]

draws <- list()
for (method in methods) {
  values <- matrix(NA_real_, nrow = nrow(cells), ncol = length(seeds))
  for (s in seq_along(seeds)) {
    imp <- mice(masked, method = method, m = 1, maxit = 5, seed = seeds[[s]], printFlag = FALSE)
    values[, s] <- as.matrix(complete(imp, 1))[cells]
  }
  # One row per masked cell, one column per seed.
  draws[[method]] <- unname(lapply(seq_len(nrow(values)), function(i) values[i, ]))
}

result <- list(
  description = "mice imputation draws for masked cells (parity class D)",
  r_version = R.version.string,
  mice_version = as.character(utils::packageVersion("mice")),
  ranger_version = as.character(utils::packageVersion("ranger")),
  rng_kind = RNGkind(),
  columns = cols,
  mask_seed = 20261023,
  mask_rate = 0.08,
  seeds = seeds,
  maxit = 5,
  # Masked data, row-major, null for masked cells.
  data = unname(lapply(seq_len(nrow(masked)), function(i) unname(unlist(masked[i, ])))),
  # Masked cells as 0-based [row, column] pairs, column-major order.
  cells = unname(lapply(seq_len(nrow(cells)), function(i) unname(cells[i, ] - 1L))),
  truth = truth,
  draws = draws
)
writeLines(toJSON(result, auto_unbox = TRUE, digits = 6, na = "null"), out)
cat("wrote", out, "with", nrow(cells), "masked cells over", nrow(x), "rows\n")
