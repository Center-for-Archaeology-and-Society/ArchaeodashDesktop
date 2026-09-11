# Review of IMPLEMENTATION.md — Node.js/TypeScript + Rust Migration Plan

Reviewed: 2026-09-09
Scope: full read of `IMPLEMENTATION.md` (1,210 lines) with spot-verification of its factual claims against the current R/Shiny source. This review evaluates the proposal; it does not change any application code.

## Verdict

The plan is unusually strong for a migration blueprint: it is evidence-based, nearly every claim I checked against the code is accurate, and it correctly identifies real bugs (e.g., the `rvals$pcaData` export bug) and dead code. The architecture direction (file-first Parquet groups, PostgreSQL control-plane only, hidden immutable row identity, recompute-transformations-on-demand) is sound and directly addresses the known weaknesses of the per-user dynamic MySQL table model.

The main weaknesses are **scale and effort realism**, a ** handful of technical imprecisions**, a few **internal tensions** (one-group-per-file vs. the app's most frequent mutation), and **several gaps** that should be resolved before Phase 0 exit.

---

## 1. Verified claims (spot-checked against the code)

These claims are factually correct as of today's source:

| Plan claim | Verification |
|---|---|
| `saveexportTab` reads nonexistent `rvals$pcaData` | Confirmed — `R/saveexportTab.R:58` reads it; no assignment exists anywhere in `R/` or `inst/app/` |
| Ordination computes `umapheader` but renders no UMAP view | Confirmed — `R/ordinationTab.R:64` is the only `umap*` output; no plot render exists |
| `subsetData` depends on nonexistent `rvals$df`; UI not mounted | Confirmed — `inst/app/server.R:196` calls `subsetDataServer` (with a "no longer used?" comment); `rvals$df` is never assigned |
| `cda.R`/`getCDA` unreachable from UI | Confirmed — defined and exported, zero call sites |
| PCA contribution table assumes ≥ 4 PCs | Confirmed — `R/ordinationTab.R:179` hard-codes `contrib[,1:4]` |
| LDA vector plot assumes LD1/LD2 | Confirmed — `R/lda.R:100–105` hard-codes `coefficients[,1]`, `[,2]` and `LD1`/`LD2` labels |
| `zScore` = row proportions × 100, then column z-score, rounded to 3 decimals | Confirmed — `R/zScore.R:11` |
| `log10`/`log` round to 3 decimals, then non-finite → 0 | Confirmed — `R/datainputTab.R:1890–1897` (order as described) |
| Eligibility `group_size > max(n_features, n_groups) + 1` | Confirmed — `R/Group_probs.R:95` (`n > (m + 1)`) |
| Hotelling p-value rounded to 5 decimals × 100; best group = max | Confirmed — `R/Group_probs.R:45, 126–127` |
| Multiplot 100,000-point interactive ceiling | Confirmed — `R/plot.R:253` |
| Euclidean top-N UI limit 1–100 | Confirmed — `R/EuclideanDistance.R:41–42` |
| Histogram bins 2–100, default 30 | Confirmed — `R/exploreTab.R:230–232` |
| k-means centers 1–20, starts 1–100, iterations 1–200 | Confirmed — `R/clusterTab.R:499–520` |
| Default INAA elemental list (33 elements) | Confirmed — exact match to `common_inaa_elements` in `R/DataLoader.R:21–25` |
| Numeric-like inference: 1,500-row sample, 95% parse rate | Confirmed — `R/datainputTab.R:464`, `R/columnTypeHints.R` |
| Missing-plot bands Good ≤ 5% / OK ≤ 40% / Bad ≤ 80% / Remove ≤ 100% | Confirmed — `R/plot_missing.R:15–16` |
| Data ellipse level clamped 0.50–0.99 | Confirmed — `R/plot.R:102` |
| `Risk_Reduction_Plan_2026-02-18.md` is an empty placeholder | Confirmed — title line only |
| No authored Quarto `.qmd` sources exist; only `.quarto` cache | Confirmed — `quarto/` contains only `.quarto` |
| `.github/workflows/r-tests.yml` is manual-only, Windows | Confirmed — `workflow_dispatch` only, `windows-latest` |
| `profvis` is imported but unused | Confirmed — `inst/app/server.R:4` calls `library(profvis)`; it is not in `DESCRIPTION` |
| `deploy.sh` references external untracked `../docker-compose.yml` | Confirmed — `deploy.sh:56` |

This level of accuracy is a major strength: the disposition tables in §13 can plausibly be trusted as a complete inventory (the `R/` file list in §13.2 matches all 48 files on disk).

---

## 2. Errors and imprecisions

1. **Calamine does not read CSV/TSV (§7.1).** The sentence "CSV, TSV, XLSX, XLS, XLSB, and ODS where Calamine coverage passes fixtures" implies Calamine covers all listed formats. Calamine is a binary-spreadsheet reader; CSV/TSV need a separate text reader with its own dialect/encoding policy. The capability matrix row for CSV/TSV should not be gated on "Calamine coverage."

2. **§13.8 dependency list contains undeclared packages.** `DataExplorer`, `shinydashboard`, `shinythemes`, `markdown`, and `profvis` are not in `DESCRIPTION` (Imports or Suggests). Two different problems are conflated:
   - `profvis` is an **undeclared runtime dependency** in the current app (`library(profvis)` in `server.R` would fail on a clean install that followed the README). This is a live bug worth fixing in the R app now, not just a migration disposition.
   - The others appear to be leftovers from an earlier state; the row should say "not declared; verify nothing imports them, then drop" rather than implying they are current dependencies.
   - `stats` is a base R package; listing it as removable is noise.

3. **Two physical encodings for `analytical_uuid` (§6.6).** Allowing "UUID string or fixed 16-byte UUID" weakens the "unknown required profile versions fail closed" story: two encodings means every reader must handle both forever, and equality/join semantics differ. Pick one canonical encoding (16-byte is more compact; string is more debuggable) and make the other a documented, tested migration path — or drop it.

4. **README references a root `INAA_test.csv` that does not exist.** The fixture inventory (§4, §13.3) tracks `inst/app/INAA_test.csv` and the testthat copy, but `README.md:18` points to a root-level file that is absent. Phase 0's "no unidentified tracked app file" exit criterion should also cover stale README references.

5. **§13.2 disposition for `R/connect.R` says hosted mode uses "SQLx",** but SQLx is never introduced in the crate list in §4 (`control-postgres` is, but the storage layer should name its SQL toolkit once and use it consistently — `sqlx` vs `tokio-postgres` vs `diesel` is an open choice that §17's spike list omits).

---

## 3. Logical inconsistencies and tensions

1. **One-group-per-file vs. the app's most frequent mutation.** The current app reassigns group membership constantly (Visualize & Assign, Cluster, Membership, Euclidean all end in "record assignments"). Under §6.6/§6.7, every assignment transaction rewrites, validates, checksums, and journals two or more complete Parquet files. For the plan's own 100k/1M-row benchmark targets (§12), a 50-row manual reassignment could rewrite a 1M-row group file and re-run full validation. The plan acknowledges journals and staging for correctness but never addresses **throughput of small mutations**. Add an explicit cost model: either accept rewrite cost with measured budgets, allow a bounded in-file revision buffer before rewrite, or document that hosted/desktop group files are expected to stay modest-sized and that very large single tables are a known limitation.

2. **"Never performs a byte-for-byte copy" of sources (§2) vs. hosted upload reality (§6.4).** In hosted mode, uploading a source spreadsheet *is* a copy into object storage — necessarily. The principle is clearly meant to be a desktop-local-path rule; §2 should scope it that way so it cannot be read as prohibiting the hosted upload path that §10.2 requires.

3. **Parity with unrecorded randomness.** §2 demands statistical parity with recorded "seeds," and §8.6 correctly notes the current UMAP runs record no seed. The plan handles this (capture defaults, define tolerances), but the parity suite for stochastic methods (UMAP, RF imputation, MICE) can only ever be *distributional*, not exact. The doc says this — but §15.1's golden-list still reads as if seeded goldens exist for "each imputation method." Phase 0 should explicitly state which goldens are exact and which are statistical, or the parity gate in Phase 6 will be unfalsifiable.

4. **Desktop loses accounts; hosted guests lose durability — acknowledged but under-priced.** §3.1 is internally consistent, but the migration quietly removes the only current persistence mode (authenticated MySQL) for users who today log in from a browser and expect their data tomorrow. The plan's guest-TTL story is good design, but there is no migration note for *existing hosted users* whose current MySQL data becomes group files under an account — §14 covers data mapping but not the user-facing communication/consent of the new storage semantics (quotas, TTL for guests, files instead of invisible tables).

5. **` analytical_uuid` visibility rule vs. debugging.** §5.1/§6.7 hide the UUID from all ordinary surfaces, and §9.4 tests assert its absence. Combined with the export rule (§7.3: never in normal exports), a user with a corrupted group file has no ordinary way to correlate a row back to its identity when reporting a bug. The "advanced support view" escape hatch (§5.1) should be a defined deliverable, not an aside, or support will be blind.

---

## 4. Gaps and omissions

1. **No effort/sizing or team model.** This is a full rewrite of an R/Shiny app into React + TypeScript + ~15 Rust crates + Tauri + Axum + PostgreSQL + object storage + a legacy MySQL migrator, with statistical reimplementation of MICE/PMM/MIDAS/RF, UMAP, DIANA, Ward.D, LDA, Hotelling T², and Mahalanobis — plus golden-parity tests for all of it. Phases 0–9 are sequential. There is no estimate, no staffing assumption, no identification of which phases can parallelize, and no decision on whether **desktop-only parity first** (dropping hosted auth/Phase 7 to a later release) is an option. For a codebase of this size maintained by what appears to be a small team, the single largest project risk is that Phases 3–6 (the numerical parity core) take far longer than everything else. The plan should name a minimum viable cutover (e.g., desktop + import + transformation + PCA/cluster, hosted later) even if it is rejected.

2. **No current-test baseline.** §15 defines excellent target coverage but never measures what exists today. A coverage baseline of the current testthat suite would let Phase 0's "signed feature/disposition matrix" be evidence-based rather than aspirational.

3. **Numerical backend decision deferred but load-bearing.** §17 defers "best maintained numerical backend for reproducible PCA/LDA and RF imputation across target CPUs" to spikes, yet §8.5/§8.7 parity depends entirely on it (BLAS nondeterminism across platforms is exactly the kind of thing that breaks golden tests). This spike should be Phase 0, not later — if no acceptable backend exists, the parity gates are unachievable and the plan needs a documented-tolerance fallback.

4. **`janitor::clean_names(case = "none")` compatibility (§7.2 step 6)** is specified as "a documented compatibility function matching" the R behavior — good instinct, but it needs an explicit golden test enumerated in §15.1's first bullet ("clean names" appears there; make the edge cases explicit: duplicates, special characters, Unicode, empty names).

5. **No explicit disposition for `inst/app/tests/testthat/` fixture files** beyond the one named CSV (§13.3 covers the runner and setup; §13.7 covers test behaviors — probably sufficient, but the inventory claim "every tracked file" should be spot-audited the way §13.2's R list was).

6. **Locale/timezone/collation determinism** is absent: numeric parsing (`as.numeric` semantics vs. Rust `f64` parse), decimal commas in European CSVs, `Sys.setlocale`-dependent sorting, and the MySQL-vs-PostgreSQL collation note in §14.1 are the classic silent-parity killers for numeric science apps. Worth one subsection in §8 or §15.

7. **Rollback story is hosted-centric.** §16 Phase 8 rehearses hosted rollback; desktop rollback is "keep the old installer," which is fine but should be stated (users of the signed desktop build need a downgrade path or an explicit "no downgrade" position).

---

## 5. Practices worth keeping (do not lose these in revision)

- The **"intentional corrections, not bug-for-bug ports"** table (§3.2) is the best part of the document — every entry I checked against the code is a real, verified defect.
- The **authoritative-data boundary** (§6.2): measured values immutable, derived values never persisted, computed results recomputed from definitions + seeds. This fixes the current design's most dangerous property (transformed matrices were persisted into per-user MySQL tables).
- **Storage-invariant tests that fail if a derived value reaches a group file** (§15.2) — make this a CI gate, not a test-suite convention.
- The **fail-closed readiness/validation split** ("Ready to add" ≠ validated) and the manifest-as-index-not-gate rule.
- The §14 migration rule that reconstructed files must never be labeled as original uploads.

---

## 6. Recommended changes before Phase 0 exit

1. Fix the Calamine/CSV imprecision and the §13.8 dependency list; file the undeclared-`profvis` bug against the current app now (one-line `DESCRIPTION` fix or remove the `library()` call).
2. Pin one canonical `analytical_uuid` physical encoding.
3. Add a mutation-throughput budget for group-file rewrites (or a documented size limitation) to §6.7/§12.
4. Scope the "never copy sources" principle to desktop in §2.
5. Promote the numerical-backend spike and the parity-tolerance decision into Phase 0 exit criteria.
6. Add a minimum-viable-cutover option (desktop-first vs. hosted-first) with an explicit accept/reject decision, so "reduced scope" cannot be sneaked in later without review.
7. Split the document: §4–§6 (storage), §8 (statistical parity), §10 (API), §13 (disposition), and §14 (migration) are each self-contained; a single 1,200-line file will not survive contact with implementation review. Cross-link instead of duplicating (the doc already repeats the measured-values-immutability rule five times — consolidate it).
8. Add locale/encoding determinism to the parity spec.
9. Add a stale-reference sweep (README's missing `INAA_test.csv`) to the Phase 0 inventory step.

---

## 7. Overall

**Conditionally approve the architecture; revise before committing to the schedule.** The destination design is coherent and the codebase evidence behind it is accurate. The unacceptable risks are effort realism (gap 1), the numerical-backend spike timing (gap 3), and the unaddressed rewrite cost of the app's most frequent operation — group reassignment — under the one-group-per-file model (inconsistency 1). None of these invalidate the plan; all three are cheap to resolve on paper and expensive to discover in Phase 5.

## 8. Revision status (2026-09-09)

Applied to `IMPLEMENTATION.md` (now split into `docs/implementation/` sub-documents; section numbering unchanged):

- **Error 1 (Calamine/CSV) — fixed** in `docs/implementation/07-import-export-data-semantics.md` §7.1 and §7.3: CSV/TSV use a dedicated text reader with an explicit dialect/encoding policy; Calamine is binary-spreadsheet only.
- **Inconsistency 3 (stochastic parity) — resolved** via new §8.0 parity classes (exact / tolerance / distributional) in `docs/implementation/08-statistical-parity.md`, the §2 principle amendment, and a §15.1 pointer. Unseeded legacy R runs (UMAP, MICE) are documented as non-parity-targetable; new implementations are seeded and versioned.
- **Gap 2 (no current-test baseline) — addressed** via new §15.4 "Baseline benchmark plan (`INAA_test.csv` oracle)": 14 named procedures with current R entry points, parity classes, capture artifacts, and re-execution gates at Phase 0/3/4/6/8 exits and the definition of done.
- **Recommendation 7 (document split) — implemented**: sections 4–14 moved to `docs/implementation/` with original numbering preserved; `IMPLEMENTATION.md` retains §1–3, §15–18, and a sub-document index.

Deliberately not applied at owner direction: the remaining inconsistencies (§3 items 1, 2, 4, 5) and remaining errors/gaps (§2 items 2–5, §4 items 1, 3, 4, 5, 6, 9) remain open findings.
