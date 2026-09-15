# R Oracle Baseline Capture 2026-09-14

## Outcome

Created [[../tools/legacy-export-r/README|the legacy R oracle]] and generated all fourteen `INAA_test.csv` baseline captures required by [[../IMPLEMENTATION]] §15.4. The command-line capture sources current pure legacy R functions, avoiding a Shiny-session dependency.

Artifacts are in `fixtures/golden/`, with the source fixture copied to `fixtures/INAA_test.csv`. `manifest.json` records the R/package versions, RNG kind, fixed replay seed, captured defaults, legacy Git commit, SHA-256 hashes for every sourced R module and generated artifact, procedure parity class, and configuration.

Validation confirmed all fourteen procedure IDs and artifact hashes. A repeat capture with seed `20260914` produced identical generated artifacts. The multiplot baseline exercises the 100,000-point ceiling with 180,000 synthetic candidates and records its 99,960 selected rows.

## Important boundary

MICE and UMAP legacy executions were historically unseeded. The oracle records deterministic replay captures for those procedures, not historical byte-for-byte equivalence; their class remains distributional. JSON is the generated canonical interchange format in this initial capture; Arrow output awaits a separately pinned Arrow producer.

## Related

- [[Implementation_Readiness_Audit_2026-09-14]]
- [[Implementation_Plan_Open_Question_Recommendations_2026-09-09]]
- [[Quality_MOC]]
