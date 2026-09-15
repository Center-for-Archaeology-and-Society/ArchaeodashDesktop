#!/usr/bin/env Rscript

# Phase-0 legacy R oracle. Run from the repository root with:
#   cd ~/.local/share/archaeodash-r-oracle/archaeodash-r-oracle
#   uvr run /absolute/path/to/tools/legacy-export-r/capture_baselines.R --root /absolute/path/to/repo
#
# It copies the immutable INAA fixture, captures the 14 procedures specified in
# IMPLEMENTATION.md §15.4, and writes only generated artifacts below
# fixtures/golden/. Do not hand-edit the resulting JSON files.

suppressPackageStartupMessages({
  library(dplyr); library(tidyr); library(tibble); library(purrr); library(data.table)
  library(jsonlite); library(magrittr)
})

args <- commandArgs(trailingOnly = TRUE)
script_args <- commandArgs(trailingOnly = FALSE)
script_file <- sub("^--file=", "", script_args[grep("^--file=", script_args)][[1L]])
arg_value <- function(name, default = NULL) {
  idx <- match(name, args)
  if (is.na(idx) || idx == length(args)) return(default)
  args[[idx + 1L]]
}
root <- normalizePath(arg_value("--root", getwd()), mustWork = TRUE)
seed <- as.integer(arg_value("--seed", "20260914"))
fixture_source <- file.path(root, "inst", "app", "INAA_test.csv")
fixture_copy <- file.path(root, "fixtures", "INAA_test.csv")
out_dir <- file.path(root, "fixtures", "golden")
dir.create(out_dir, recursive = TRUE, showWarnings = FALSE)
if (!file.exists(fixture_source)) stop("Missing legacy fixture: ", fixture_source)

sha256 <- function(path) {
  fields <- strsplit(system2("sha256sum", path, stdout = TRUE)[[1L]], "[[:space:]]+")[[1L]]
  unname(fields[[1L]])
}
if (!file.exists(fixture_copy) || !identical(sha256(fixture_source), sha256(fixture_copy))) {
  if (!file.copy(fixture_source, fixture_copy, overwrite = TRUE)) stop("Could not create canonical fixture copy")
}

finite_json <- function(x) {
  if (is.data.frame(x)) {
    for (nm in names(x)) if (is.numeric(x[[nm]])) x[[nm]][!is.finite(x[[nm]])] <- NA_real_
  } else if (is.numeric(x)) x[!is.finite(x)] <- NA_real_
  x
}
write_capture <- function(name, value) {
  path <- file.path(out_dir, name)
  jsonlite::write_json(finite_json(value), path, dataframe = "rows", auto_unbox = TRUE, pretty = TRUE, na = "null", null = "null")
  list(file = file.path("fixtures", "golden", name), sha256 = sha256(path))
}
package_versions <- function(pkgs) {
  setNames(vapply(pkgs, function(pkg) as.character(utils::packageVersion(pkg)), character(1)), pkgs)
}
function_defaults <- function(fun) {
  lapply(formals(fun), function(x) paste(deparse(x, width.cutoff = 500L), collapse = " "))
}
numeric_frame <- function(df, columns) {
  out <- df[, columns, drop = FALSE]
  out[] <- lapply(out, function(x) suppressWarnings(as.numeric(as.character(x))))
  out
}
clean_log <- function(x, method) {
  before <- suppressWarnings(if (identical(method, "log10")) log10(as.matrix(x)) else log(as.matrix(x)))
  out <- as.data.frame(before)
  out[] <- lapply(out, function(col) { col <- round(col, 3); col[!is.finite(col)] <- 0; col })
  list(values = out, non_finite_to_zero = sum(!is.finite(before)))
}
run_mice <- function(data, method, seed) {
  set.seed(seed)
  imp <- mice::mice(data, m = 1L, maxit = 5L, method = method, printFlag = FALSE)
  list(values = mice::complete(imp, 1L), logged_events = imp$loggedEvents)
}
lda_capture <- function(df, chem, group) {
  mdl <- MASS::lda(stats::as.formula(paste0("`", group, "` ~ .")), data = df[, c(group, chem)])
  centered <- scale(df[, chem], center = colMeans(df[, chem]))
  scores <- centered %*% mdl$scaling - matrix(colMeans(centered %*% mdl$scaling), nrow = nrow(df), ncol = ncol(mdl$scaling), byrow = TRUE)
  list(prior = mdl$prior, means = mdl$means, scaling = mdl$scaling, svd = mdl$svd, scores = as.data.frame(scores))
}
silhouette_mean <- function(x, clusters) {
  if (length(unique(clusters)) < 2L) return(NA_real_)
  mean(cluster::silhouette(clusters, stats::dist(x))[, "sil_width"])
}

# Load the current pure implementation functions, with notification hooks made
# inert so this command-line oracle has no Shiny-session dependency.
mynotification <- function(...) invisible(NULL)
app_is_verbose <- function() FALSE
app_require_packages <- function(packages, ...) all(vapply(packages, requireNamespace, logical(1), quietly = TRUE))
source(file.path(root, "R", "quietly.R"))
source(file.path(root, "R", "zScore.R"))
source(file.path(root, "R", "columnTypeHints.R"))
source(file.path(root, "R", "DataLoader.R"))
source(file.path(root, "R", "Group_probs.R"))
source(file.path(root, "R", "EuclideanDistance.R"))
source(file.path(root, "R", "plot_missing.R"))
mynotification <- function(...) invisible(NULL)

loaded <- dataLoader(fixture_copy)
chem <- default_chem_columns(names(loaded))
id_col <- default_id_column(names(loaded))
group_col <- "CORE"
if (!group_col %in% names(loaded)) stop("Expected CORE column after legacy name cleaning")
base_chem <- chem[seq_len(8L)]
features <- numeric_frame(loaded, base_chem)
membership_chem <- base_chem[seq_len(4L)]
membership_df <- bind_cols(loaded[, c("rowid", id_col, group_col)], numeric_frame(loaded, membership_chem))

captures <- list()
add_capture <- function(id, title, parity_class, value, entry_point, config = list()) {
  artifact <- write_capture(sprintf("%02d_%s.json", as.integer(id), gsub("[^a-z0-9]+", "_", tolower(title))), value)
  captures[[length(captures) + 1L]] <<- list(
    id = as.integer(id), title = title, parity_class = parity_class,
    entry_point = entry_point, config = config, artifact = artifact
  )
}

# 1. Import semantics.
group_partitions <- loaded |> count(.data[[group_col]], name = "row_count") |> arrange(.data[[group_col]])
add_capture(1, "CSV import and numeric inference", "E", list(
  columns = names(loaded), row_count = nrow(loaded), id_column = id_col,
  default_chem_columns = chem, numeric_like_columns = guess_numeric_columns_fast(loaded, sample_n = 1500L),
  group_partitions = group_partitions
), "DataLoader.R; columnTypeHints.R", list(sample_n = 1500L, min_parse_rate = 0.95))

# 2-4. Transform and ratio semantics.
add_capture(2, "zScore", "E", zScore(features), "zScore.R", list(columns = base_chem))
add_capture(3, "log transforms", "E", list(log10 = clean_log(features, "log10"), log = clean_log(features, "log")), "datainputTab.R", list(columns = base_chem, round_digits = 3L))
ratio_specs <- tibble(ratio = c("as_la", "lu_nd"), numerator = c("as", "lu"), denominator = c("la", "nd"))
ratio_input <- features
ratio_input$la[[1L]] <- 0
ratio_input$nd[[2L]] <- NA_real_
ratio_output <- ratio_input
for (i in seq_len(nrow(ratio_specs))) {
  ratio_output[[ratio_specs$ratio[[i]]]] <- ifelse(is.na(ratio_input[[ratio_specs$denominator[[i]]]]) | ratio_input[[ratio_specs$denominator[[i]]]] == 0, NA_real_, ratio_input[[ratio_specs$numerator[[i]]]] / ratio_input[[ratio_specs$denominator[[i]]]])
}
add_capture(4, "Ratio construction and application", "E", list(specs = ratio_specs, values = ratio_output), "datainputTab.R", list(mode = "append"))

# 5. MICE variants receive a deterministic fixture and recorded seeds. The
# legacy UI does not set one; these are replayable oracle captures, not claims
# about historical unseeded runs.
impute_input <- as.data.frame(features[seq_len(80L), seq_len(4L)])
impute_input[cbind(c(2L, 9L, 22L, 47L), c(1L, 2L, 3L, 4L))] <- NA_real_
imputations <- list(none = impute_input)
for (method in c("pmm", "midastouch", "rf")) imputations[[method]] <- run_mice(impute_input, method, seed)
add_capture(5, "Imputation", "D", imputations, "datainputTab.R; mice", list(seed = seed, methods = names(imputations), maxit = 5L))

# 6-8. Ordination methods.
pca <- stats::prcomp(features)
add_capture(6, "PCA", "T", list(sdev = pca$sdev, rotation = pca$rotation, scores = as.data.frame(pca$x), center = pca$center, scale = pca$scale), "ordinationTab.R", list(columns = base_chem, center = TRUE, scale = FALSE))
umap_runs <- lapply(c(seed, seed + 1L, seed + 2L), function(s) { set.seed(s); fit <- umap::umap(features); list(seed = s, layout = as.data.frame(fit$layout), config = unclass(fit$config)) })
add_capture(7, "UMAP", "D", umap_runs, "ordinationTab.R; umap", list(seeds = c(seed, seed + 1L, seed + 2L)))
add_capture(8, "LDA", "T", lda_capture(bind_cols(loaded[, group_col, drop = FALSE], features), base_chem, group_col), "lda.R", list(group = group_col, columns = base_chem))

# 9. Clustering and diagnostics.
set.seed(seed)
kmeans_fit <- stats::kmeans(features, centers = 5L, iter.max = 100L, nstart = 25L)
pam_fit <- cluster::pam(features, k = 5L, metric = "euclidean")
hc_fit <- stats::hclust(stats::dist(features), method = "ward.D2")
diana_fit <- cluster::diana(as.matrix(features), metric = "euclidean")
wss <- vapply(1:10, function(k) { set.seed(seed + k); stats::kmeans(features, centers = k, iter.max = 100L, nstart = 25L)$tot.withinss }, numeric(1))
sil <- vapply(2:10, function(k) { set.seed(seed + k); fit <- stats::kmeans(features, centers = k, iter.max = 100L, nstart = 25L); silhouette_mean(features, fit$cluster) }, numeric(1))
add_capture(9, "Clustering", "T", list(kmeans = list(cluster = kmeans_fit$cluster, centers = kmeans_fit$centers, tot_withinss = kmeans_fit$tot.withinss), pam = list(cluster = pam_fit$clustering, medoids = pam_fit$id.med, objective = pam_fit$objective), hclust = list(merge = hc_fit$merge, height = hc_fit$height, order = hc_fit$order), diana = list(merge = diana_fit$merge, height = diana_fit$height, order = diana_fit$order), wss = wss, silhouette = sil), "clusterTab.R", list(seed = seed, centers = 5L, nstart = 25L, iter_max = 100L))

# 10. Membership. Four features keep all five CORE groups eligible and make
# Hotelling's T2 computation tractable for a baseline fixture.
eligible <- getEligible(membership_df, membership_chem, group_col)
hotelling <- group.mem.probs(membership_df, membership_chem, group_col, eligible, "Hotellings", id_col)
mahalanobis <- group.mem.probs(membership_df, membership_chem, group_col, eligible, "Mahalanobis", id_col)
add_capture(10, "Membership probabilities", "T", list(eligibility = eligible, hotelling = hotelling, mahalanobis = mahalanobis), "Group_probs.R", list(group = group_col, columns = membership_chem))

# 11. Nearest matches uses the current function directly.
euclidean <- calcEDistance(membership_df, projection = eligible, id = id_col, attrGroups = group_col, chem = membership_chem, limit = 5L, withinGroup = FALSE)
add_capture(11, "Euclidean nearest matches", "E", euclidean, "EuclideanDistance.R", list(group = group_col, columns = membership_chem, limit = 5L, within_group = FALSE))

# 12. Explore data: missing profile, fixed-bin histogram, and compositional
# profile rows are serialised instead of plots, which are the stable inputs to
# the legacy renderers.
missing_profile <- profile_missing(features)
missing_profile$Band <- cut(missing_profile$pct_missing, breaks = c(-Inf, .05, .4, .8, 1), labels = c("Good", "OK", "Bad", "Remove"))
histogram <- graphics::hist(features[[1L]], breaks = 30L, plot = FALSE)
composition <- features |> mutate(rowid = loaded$rowid) |> pivot_longer(-rowid, names_to = "element", values_to = "value")
add_capture(12, "Explore views", "E", list(missing_profile = missing_profile, histogram = list(breaks = histogram$breaks, counts = histogram$counts), compositional_profile = composition), "plot_missing.R; exploreTab.R; comp.profile.R", list(histogram_bins = 30L))

# 13. Capture the deterministic slice-head rule used by the interactive
# multiplot. This synthetic pair table has 180,000 candidates and therefore
# crosses the 100,000-point ceiling before recording the exact selected rows.
facets <- expand.grid(xvar = paste0("x", 1:4), yvar = paste0("y", 1:3), stringsAsFactors = FALSE)
groups <- sort(unique(as.character(loaded[[group_col]])))
ceiling <- 100000L; facet_count <- nrow(facets); group_count <- length(groups)
per_group_facet <- max(25L, as.integer(floor(ceiling / (facet_count * group_count))))
candidate_rows <- purrr::map_dfr(seq_len(nrow(facets)), function(facet_i) {
  purrr::map_dfr(groups, function(group_value) {
    source_rows <- loaded$rowid[as.character(loaded[[group_col]]) == group_value]
    tibble(xvar = facets$xvar[[facet_i]], yvar = facets$yvar[[facet_i]], group = group_value,
           ordinal = seq_len(3000L), source_rowid = rep(source_rows, length.out = 3000L))
  })
})
sampled_rows <- candidate_rows |> group_by(.data$xvar, .data$yvar, .data$group) |> slice_head(n = per_group_facet) |> ungroup()
add_capture(13, "Multiplot interactive sampling", "E", list(max_points_total = ceiling, candidate_count = nrow(candidate_rows), facet_count = facet_count, group_count = group_count, max_points_per_group_facet = per_group_facet, selected_count = nrow(sampled_rows), selected_index_set = sampled_rows), "plot.R", list(strategy = "group_by(xvar, yvar, group) |> slice_head"))

# 14. Measured-data CSV export/import round trip.
export_path <- file.path(out_dir, "14_measured_data_export.csv")
rio::export(loaded, export_path)
round_trip <- rio::import(export_path, setclass = "data.frame")
add_capture(14, "Measured-data export round trip", "E", list(export_file = file.path("fixtures", "golden", basename(export_path)), export_sha256 = sha256(export_path), source_columns = names(loaded), round_trip_columns = names(round_trip), source_rows = nrow(loaded), round_trip_rows = nrow(round_trip), cell_text_equal = identical(lapply(loaded, as.character), lapply(round_trip, as.character))), "saveexportTab.R; rio", list(format = "CSV"))

manifest <- list(
  schema_version = 1L,
  generated_at_utc = format(Sys.time(), tz = "UTC", usetz = TRUE),
  source_fixture = list(path = file.path("inst", "app", "INAA_test.csv"), sha256 = sha256(fixture_source)),
  canonical_fixture = list(path = file.path("fixtures", "INAA_test.csv"), sha256 = sha256(fixture_copy)),
  r = list(version = R.version.string, rng_kind = RNGkind()),
  legacy_source = list(
    git_commit = tryCatch(system2("git", c("-C", root, "rev-parse", "HEAD"), stdout = TRUE)[[1L]], error = function(e) NA_character_),
    oracle_script_sha256 = sha256(normalizePath(script_file, mustWork = TRUE)),
    sourced_files = setNames(lapply(c("R/quietly.R", "R/zScore.R", "R/columnTypeHints.R", "R/DataLoader.R", "R/Group_probs.R", "R/EuclideanDistance.R", "R/plot_missing.R"), function(path) sha256(file.path(root, path))), c("R/quietly.R", "R/zScore.R", "R/columnTypeHints.R", "R/DataLoader.R", "R/Group_probs.R", "R/EuclideanDistance.R", "R/plot_missing.R"))
  ),
  packages = package_versions(c("mice", "umap", "MASS", "cluster", "factoextra", "ICSNP", "candisc", "rio", "jsonlite")),
  captured_defaults = list(mice = function_defaults(mice::mice), umap = function_defaults(umap::umap), prcomp = function_defaults(getS3method("prcomp", "default")), lda = function_defaults(getFromNamespace("lda.default", "MASS"))),
  seed = seed,
  procedures = captures,
  limitations = list("Legacy application does not record seeds for MICE or UMAP; capture 5 and 7 are deterministic replay baselines, not historical-run equivalence.", "JSON is the canonical Phase-0 interchange artifact; Arrow output is deferred until an Arrow producer is pinned."),
  regeneration = "Run this script through the pinned uvr oracle environment. Do not hand-edit generated files."
)
manifest_path <- file.path(out_dir, "manifest.json")
jsonlite::write_json(manifest, manifest_path, auto_unbox = TRUE, pretty = TRUE, na = "null", null = "null")
cat("Captured", length(captures), "procedures to", out_dir, "\n")
