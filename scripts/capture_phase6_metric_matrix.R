# Capture the Phase 6 metric/linkage matrix from the installed R oracle.
# Run from repository root: Rscript scripts/capture_phase6_metric_matrix.R
stopifnot(getRversion() >= "4.0.0")
library(cluster)
out <- "fixtures/golden/09_metric_linkage_matrix.tsv"
tie <- rbind(c(0,0), c(0,0), c(1,0), c(0,1), c(1,1), c(2,1))
plain <- rbind(c(0,0), c(1,2), c(2,0), c(4,1), c(5,3), c(3,4))
datasets <- list(tie=tie, plain=plain)
metrics <- c("euclidean", "manhattan", "minkowski", "maximum")
methods <- c("average", "complete", "ward.D", "ward.D2")
fmt <- function(x) paste(format(x, digits=17, scientific=FALSE, trim=TRUE), collapse=",")
vec <- function(x) paste(as.integer(x), collapse=",")
lines <- c(paste0("# R=", R.version.string, "\tcluster=", as.character(packageVersion("cluster")),
                   "\tcommand=Rscript scripts/capture_phase6_metric_matrix.R"),
           "kind\tdataset\tmetric\tlinkage\tmerge\theight\torder\tmedoids\tlabels\tbuild\tswap")
for (dn in names(datasets)) for (metric in metrics) for (method in methods) {
  d <- if (metric == "minkowski") dist(datasets[[dn]], method=metric, p=3) else dist(datasets[[dn]], method=metric)
  z <- hclust(d, method=method)
  lines <- c(lines, paste("hca",dn,metric,method,
    paste(apply(z$merge,1,vec),collapse=";"),fmt(z$height),vec(z$order),"","","","",sep="\t"))
}
for (dn in names(datasets)) for (metric in c("euclidean","manhattan")) {
  p <- pam(datasets[[dn]], 2, metric=metric)
  d <- diana(datasets[[dn]], metric=metric)
  lines <- c(lines, paste("pam",dn,metric,"", "", "", "",vec(p$id.med),vec(p$clustering),
    fmt(p$objective[1:2]),sep="\t"))
  lines <- c(lines, paste("diana",dn,metric,"",paste(apply(d$merge,1,vec),collapse=";"),
    fmt(d$height),vec(d$order),"","","","",sep="\t"))
}
writeLines(lines, out)
cat("Wrote ",out," from ",R.version.string," / cluster ",as.character(packageVersion("cluster")),"\n",sep="")
