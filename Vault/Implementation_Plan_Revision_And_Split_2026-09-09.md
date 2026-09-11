# Implementation Plan Revision And Split 2026-09-09

## Summary

Applied four owner-directed revisions to [[../IMPLEMENTATION]] following [[../IMPLEMENTATION_REVIEW]]:

1. **Calamine fix (§7.1, §7.3):** CSV/TSV imports now explicitly use a dedicated text reader with a dialect/encoding policy; Calamine is documented as binary-spreadsheet only (XLSX/XLS/XLSB/ODS). Verified against Calamine's actual coverage; the original wording implied CSV/TSV depended on it.
2. **Stochastic parity classes (new §8.0):** every statistical output is assigned exactly one class — **E** (exact: IDs, counts, assignments, rounded contracts like zScore/log10), **T** (tolerance: PCA/LDA/Mahalanobis/Hotelling/Euclidean/seeded clustering), **D** (distributional: UMAP, MICE `pmm`/`midastouch`/`rf`). Legacy unseeded R runs are declared non-reproducible and not parity targets; class reclassification needs §15.3 sign-off. §2 principle and §15.1 amended to match.
3. **Baseline benchmark plan (new §15.4):** 14 procedures run through the current R implementation on `inst/app/INAA_test.csv` (canonical fixture: `fixtures/INAA_test.csv`, SHA-256 in manifest) before any replacement code lands: import/inference, zScore, log/log10, ratios, imputation, PCA, UMAP, LDA, clustering, membership/Hotelling/Mahalanobis, Euclidean matches, Explore views, multiplot sampling determinism, export round trip. Entry points cite current R files (e.g., `DataLoader.R`, `zScore.R`, `Group_probs.R`, `plot.R` 100k ceiling). Re-execution gates: Phase 0 capture, Phase 3 (procs 1–5), Phase 4 (6–8, 12), Phase 6 (9–11, 13), pre-cutover full suite; wired into Phase 0 exit criteria and §18 definition of done.
4. **Document split:** sections 4–14 moved verbatim to `docs/implementation/NN-*.md` (11 files; original section numbers and cross-references preserved). `IMPLEMENTATION.md` now holds §1–3, §15–18, a sub-document index, and a `Revised: 2026-09-09` header line.

Method note: split performed by scripted line-range extraction (lossless, UTF-8 no BOM, LF preserved), not retyping. All other review findings (group-file rewrite throughput, no-copy principle scoping, guest durability, UUID visibility, §13.8 dependency list, UUID encoding pin, backend-spike timing, locale determinism, stale-reference sweep) remain open in the review.

## Verification

- Master doc: 337 lines; H2 sections 1, 2, 3, Sub-document index, 15, 16, 17, 18 present.
- 11 sub-documents each carry their original `## N.` heading plus a back-link header.
- Edits confirmed by grep/read: Calamine wording, §8.0, §15.4 table, Phase 0 exit, §18 bullet.

## Related

- [[../IMPLEMENTATION_REVIEW]]
- [[Node_Rust_Migration_Implementation_Plan_2026-09-08]]
- [[Quality_MOC]]
