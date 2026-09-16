#!/usr/bin/env bash
set -euo pipefail
cd ~/ArchaeodashDesktop
git add -A
git commit -q -F - <<'MSG'
Phase 2 (partial): group Parquet profile, import engine, transforms, goldens 1-4

- data-io: R-compatible numerics (parse/format/round with FMA tie handling),
  janitor clean_names(case="none") port verified against the oracle,
  DataLoader port (rowid prepend, INAA defaults, 1500-row/95% numeric
  inference), group Parquet profile v1 with Parquet footer KV metadata,
  UUID-preserving rewrite, measured-element checksum, project scanner,
  full validation-on-add, derived-column storage invariant
- analysis: zScore (scale with na.rm=TRUE semantics), log transforms,
  ratio application; oracle-verified via live R probes
- tests/parity: golden procedures 1-4 (import, zScore, log, ratios) pass
  exactly against the R oracle fixtures; 6 group-profile integration tests
- clippy -D warnings clean; fmt clean; full workspace test suite green
MSG
git log --oneline -3
