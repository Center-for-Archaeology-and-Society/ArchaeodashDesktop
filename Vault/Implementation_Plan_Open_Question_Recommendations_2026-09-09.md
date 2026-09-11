# Implementation Plan Open-Question Recommendations (2026-09-09)

## Summary

Added best-practice recommended defaults for every open question in [[../IMPLEMENTATION]] §17 ("Must be answered by spikes/inventory, without reducing scope") as a new §17.1, wired the two Phase-0-critical spikes (numerical backend, `umap_rs`) into Phase 0 exit criteria, and added the missing control-plane SQL toolkit open question flagged by [[Implementation_Plan_Review_2026-09-09]].

Each recommendation is a default with an evidence gate: a spike confirms it or overturns it with a recorded decision. Nothing in §17 lost its gate; scope was not reduced.

## Recommended defaults (§17.1)

1. R defaults/tolerances — capture `formals()`/package versions/RNG kind mechanically via the Phase 0 oracle; tolerances by §8.0 class (E exact; T rel 1e-6 same-platform / 1e-4 cross-platform eigen-based, 1e-9 Euclidean; D fixed-seed + distribution metrics).
2. Numerical backend — pure-Rust deterministic core (`faer`/`ndarray`) for PCA/LDA/Mahalanobis/Hotelling; BLAS multithreading only with re-approved T-class tolerance; Linfa RandomForest for `rf` imputation validated distributionally against R `mice rf`. Decided in Phase 0.
3. `umap_rs` — Phase 0 spike against §15.4 procedure 7: brute-force KNN ≤100k rows, fixed-seed init, pinned epochs, panic isolation; k-NN overlap/trustworthiness gate across ≥3 seeds; fallback = reimplement or hold behind experimental-parity flag.
4. DIANA/Ward.D — crates where parity passes (`kodama`, `kmedoids`); custom O(n²) DIANA in `crates/analysis` with property tests; no clustering method ships without its procedure-9 golden.
5. Sizes/quotas — Phase 0 MySQL inventory is the floor; 1M rows × ~40 columns is the tested ceiling; guest hard quota + published TTL; authenticated default 5 GB operator-configurable.
6. Hosted file backend — `object_store` traits as the only port; production default self-hosted S3-compatible (MinIO/cloud) with versioning + ~30-day noncurrent expiry; filesystem stays a supported deployment mode; §15.2 contract tests run against both.
7. Same-format writeback — XLS/XLSB/ODS import-only; writeback promises only for CSV/TSV and app-generated XLSX; §7.1 capability matrix stays authoritative.
8. Footer metadata/checksums — `archaeodash.profile.v1` namespaced Parquet KV metadata, canonical JSON (UTF-8, sorted keys); checksums over canonical tuple streams, not file bytes; fail-closed unknown majors; one `analytical_uuid` encoding (16-byte preferred) with one-time migration for the other.
9. Desktop platforms/signing — Windows 10+ and macOS 12+ (Intel + Apple silicon) signed/notarized; Linux AppImage with published checksums; Tauri updater with signed manifests; key custody assigned at Phase 1.
10. Retention/cookies — 7-day sliding guest TTL; 30-day authenticated soft-delete window; ~30-day backups; export-all/delete-all requests; strictly necessary session + CSRF + remember-me cookies (banner determination stays with counsel per §11.4).
11. SVG/PDF — PNG at parity launch; SVG only with deterministic renderer fixture tests; PDF deferred post-launch; §7.3 matrix records the final decision.
12. Control-plane SQL toolkit — SQLx (compile-time-checked queries, migrations), named in the Phase 1 control-plane crate; alternatives require overturning the recorded default.

## Method note

Edits confined to the master `IMPLEMENTATION.md` (§17 spike list + new §17.1, Phase 0 bullet/exit line, Revised header). Sub-documents unchanged; §17.1 references existing sections (3.1, 6.5, 6.6, 6.9, 7.1, 7.3, 8.0, 11.4, 12, 13.2, 15.2, 15.4) rather than duplicating them. Section numbering unchanged.

## Related

- [[../IMPLEMENTATION]] (§17.1, Phase 0)
- [[../IMPLEMENTATION_REVIEW]]
- [[Implementation_Plan_Review_2026-09-09]]
- [[Implementation_Plan_Revision_And_Split_2026-09-09]]
- [[Node_Rust_Migration_Implementation_Plan_2026-09-08]]
- [[Interaction_Log_2026-09-09]]
