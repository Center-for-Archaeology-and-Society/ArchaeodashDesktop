#!/usr/bin/env bash
set -euo pipefail
ORACLE="$HOME/.local/share/archaeodash-r-oracle/archaeodash-r-oracle"
export R_LIBS_USER="$ORACLE/.uvr/library"
Rscript -e '
cat("jsonlite digits default:", jsonlite::toJSON(0.1098312345, auto_unbox=TRUE), "\n")
cat("round3 half-even:", round(0.0005,3), round(0.0015,3), round(2.675,2), "\n")
cat("scale uses n-1 sd check:\n")
x <- matrix(c(1,2,3,4,5,6), nrow=3)
print(scale(x)[,1])
cat("manual:", (x[,1]-mean(x[,1]))/sd(x[,1]), "\n")
cat("prop table check:\n")
m <- matrix(c(1,2,3,4), nrow=2)
print(prop.table(m,1)*100)
'
python3 - <<'PYEOF'
import json
g2 = json.load(open('/home/rjbischo/ArchaeodashDesktop/fixtures/golden/02_zscore.json'))
print("zscore rows:", len(g2), "cols:", list(g2[0].keys()))
nulls = sum(1 for row in g2 for v in row.values() if v is None)
print("zscore nulls:", nulls)
neg_zero = sum(1 for row in g2 for v in row.values() if v == 0 and str(v).startswith('-'))
print("zscore -0.0 count:", neg_zero)
g3 = json.load(open('/home/rjbischo/ArchaeodashDesktop/fixtures/golden/03_log_transforms.json'))
print("log keys:", list(g3.keys()))
print("log10 nonfinite:", g3['log10']['non_finite_to_zero'], "log nonfinite:", g3['log']['non_finite_to_zero'])
print("log10 rows:", len(g3['log10']['values']), "nulls:", sum(1 for r in g3['log10']['values'] for v in r.values() if v is None))
g4 = json.load(open('/home/rjbischo/ArchaeodashDesktop/fixtures/golden/04_ratio_construction_and_application.json'))
print("ratio specs:", g4['specs'])
print("ratio rows:", len(g4['values']), "nulls:", sum(1 for r in g4['values'] for v in r.values() if v is None))
r1 = g4['values'][1]
print("row2 as_la:", r1['as_la'], "expected exact:", 3.719/33.861)
PYEOF
