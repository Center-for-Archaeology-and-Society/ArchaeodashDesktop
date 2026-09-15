#!/usr/bin/env bash
# Print the latest stable version of crates used by the ArchaeoDash workspace.
set -u
for c in faer csv serde axum tokio arrow parquet calamine rust_xlsxwriter kodama kmedoids thiserror uuid argon2 rand tower-http statrs; do
  v=$(curl -s -A "archaeodash-setup" --max-time 10 "https://crates.io/api/v1/crates/$c" | python3 -c "import sys,json;print(json.load(sys.stdin)['crate']['max_stable_version'])" 2>/dev/null)
  echo "$c: ${v:-UNREACHABLE}"
done
