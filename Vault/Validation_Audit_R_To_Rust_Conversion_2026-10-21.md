# Validation Audit: R Shiny → Rust/TS Conversion (2026-10-21)

Independent static validation of the conversion at the local checkout `be2ddce` (2026-10-04; corresponds to `cb4c520` on GitHub after the 2026-10-08 history purge; GitHub master was 5 commits ahead at `82eca8c` when this was pushed),
read tab-by-tab against `inst/app/ui.R`, `inst/app/server.R` and `R/*.R`.
Full report with file:line evidence: Claude doc "ArchaeoDash R→Rust Conversion
Validation" (claude.ai artifact `2MaWScrRgqQX1x5WqjyH9K`).

## Verdict

Not yet a successful replacement. The statistical core is faithfully ported and
golden-tested; the application around it drops or changes a large share of the
legacy workflow, the hosted binary cannot run analyses, and the desktop build is
unpackaged.

## Verified correct

- All 14 golden fixtures match `fixtures/golden/manifest.json` SHA-256s; the
  `tests/parity` tests are cell-by-cell comparisons (class E 1e-12, class T at
  4-dp serialization with sign/permutation alignment). Procedures 1–4, 6–12, 14
  are covered in Rust; 13 in `multiplot-model.test.ts`.
- k-means is a line-faithful Hartigan-Wong port with R's MT19937 draws; every
  R cluster option has an enum variant and code path; Hotelling→Mahalanobis
  fallback is surfaced; UMAP naive port has ratified class-D thresholds.
- Architecture matches the plan (shared `crates/application` core behind Axum
  and Tauri; immutable measured values; revision checks; project lock).
- §3.2 intentional corrections are present.

## Result-changing divergences (not authorized by §3.2/§8/§13)

1. Ratios applied after the transform (`transforms.rs:353-378`); R applied
   ratios first and transformed ratio columns too (`datainputTab.R:1862-1903`).
2. zScore no longer zeroes non-finite values (`analysis/src/lib.rs:126-134`).
3. Import UI never sends `recipe` (`DataManager.tsx:94`), so R's default
   zero→NA / negative→NA policies are off.
4. Explore/Ordination/Visualize call the API with `transformation: null`.
5. `MAX_DISTANCE_CELLS = 1_000_000` (`clustering.rs:33`) rejects every
   cluster/diagnostic/Euclidean run above 1,000 rows, including k-means; R had
   no cap.
6. Compositional profile draws one polyline per group from the first matching
   row and clamps values >10 (`ExplorePage.tsx:463-483`).
7. Euclidean within-group default flipped to `true`; ellipse default off and
   chi-square instead of `stat_ellipse(type="t")`; "Automatic IDs" writes an
   all-null `anid`; merge requires identical column lists; hosted
   `GET /preferences` returns a flat map the client cannot parse.

## Missing (parity scope per §3, not retired in §13)

Imputation (`imputation_not_gated`; golden 05 untested); XLSX/XLS/ODS/TSV
import (advertised, parsed as comma CSV; `calamine` unused); append mode and
type harmonization; sub-group selection; add group column/value; UI for
transformation save/delete, ratio builder, merge/delete groups; multi-group
workspace; PCA loadings plot and contribution table; LDA vector plot; UMAP/LDA
as Visualize sources; Visualize theme/symbols/label toggle/multi-value filter;
multiplot X/Y selectors; PNG plot export with size/DPI (per-panel SVG via
`<a download>` only, not routed through the Tauri save dialog); membership
export; user-named exports; XLSX/TSV export; Group Sizes table; §3.1 hosted
guest workflow; resend-verification; reset session; auto-reopen last project.

## Delivery state

- Hosted: `bin/hosted` mounts only health/auth/preferences/projects/files
  routers (`hosted.rs:85-106`); `root_router` (analysis, import, groups,
  ordination, cluster, exports) is used only by tests and
  `examples/local.rs`; nothing serves the React bundle.
- Desktop: `tauri.conf.json` has `bundle.active=false`, `csp: null`, only
  `core:default` capability; CI builds/launches the debug binary only;
  `AppShell.tsx:151-157` hides Home/Help/Terms/Privacy/Account until a project
  is opened.

## Documentation drift

`help.md:44-85` describes imputation, transformation naming/delete, reset,
add column, clear workspace and a Data Manager export section that do not
exist; `privacy.md:34` says CSRF cookie `SameSite=Strict` (code: `Lax`);
consent links `/legal/*` vs routes `/info/*`; `lib.rs:1260` and
`contracts/src/lib.rs:383` cite §7.1/§8.4 for behavior those sections
contradict; `10-hosted-http-api.md:16` logout cookie expiry not implemented.
IMPLEMENTATION.md's Phase 3 gate (procedures 1–5) and Phase 4 visual-parity
gate are not supported by the code.

## Recommended order

1. Fix divergences 1–3 and add a log+ratio parity test.
2. Wire the active transformation into Explore/Ordination/Visualize.
3. Scope the 1,000-row cap (exempt k-means and top-k Euclidean).
4. Build Data Manager UI over existing services; decide append/multi-group.
5. Implement or formally retire XLSX/TSV import, imputation, PNG export.
6. Restore ordination views; add UMAP/LDA Visualize sources.
7. Small defaults/bugs (profile plot, within-group, ellipse, symbols, anid,
   preference shape, logout cookie, consent paths).
8. Hosted: mount `root_router` + static assets, or restate hosted scope.
9. Desktop: enable bundle, CSP, dialog permissions, `tauri build` in CI.
10. Correct help.md, privacy.md and the phase-gate statements.

## Re-check against GitHub master `82eca8c` (2026-10-14)

The five commits after `cb4c520` (hosted transformation-definition catalog
and routes, `ci.yml` removed in favour of `scripts/verify-local.sh` +
`deploy/compose/verify.yml`, tombstone sweep, client 202 fix) change none of
the findings above. Two consequences: the three-platform numerical-parity and
macOS/Windows desktop/launch-smoke jobs no longer run anywhere, and the local
`~/ArchaeodashDesktop` checkout is on pre-rewrite history five commits behind
(`git fetch && git reset --hard origin/master` after saving local changes).

## Limits

Static review only: `cargo test`/`pnpm test` were not executed (no package
registry access in the review environment); no desktop launch; no browser e2e;
`inst/app/www/` and `tests/testthat/*.R` not read. Confirm locally with
`cargo test -p archaeodash-analysis -p archaeodash-parity --all-targets`,
`cargo test --workspace`, `pnpm -r test`.
