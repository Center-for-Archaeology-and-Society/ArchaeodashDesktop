# ArchaeoDash Node.js/TypeScript + Rust Migration Implementation Plan

Status (2026-10-01): implementation in progress through Phase 6; the Phase 6 exit gates are accepted.

### Current implementation checkpoint

An initial Linux desktop test drive is now prepared: basic Data Manager/import UI, native folder and CSV save dialogs, a disposable INAA sample, and a verified build/launch script. The real native window passed basic import, clustering, cancellation, export, and reopen checks. See the [test-drive guide](docs/operations/desktop-test-drive.md). This is a local development build, not a signed release or phase-exit sign-off.

- Phases 0–4: baseline fixtures, workspace, group-file operations, transformations, ordination, Explore, preferences, and exports have implementation slices recorded in Vault. This is not a claim that every phase exit criterion has passed.
- Phase 5: visualization, assignment, static/interactive multiplots, and plot export are implemented; Linux native end-to-end assignment passes.
- Phase 6: numerical clustering, membership, and Euclidean goldens, bounded shared services, HTTP/Tauri adapters, typed client transports, and analysis controls/result tables, elbow/silhouette plots, and horizontal Ward.D2/DIANA dendrograms with cut coloring and expanded views are implemented. UUID-addressed result selection and confirmed manual moves to existing group files are implemented with source-revision checks. Automatic best/matched-group assignment and multi-group cluster recording now use explicit reviewed destination mappings and one revision-checked batch transaction, with guarded filesystem publication and recovery. The remaining source controls (transformations, PCA/UMAP/LDA and projection groups), HCA metric/linkage matrix, PAM/DIANA metrics, partition plots, bounded cancellable jobs with progress events, and native desktop project opening are implemented. Procedures 9–11 and 13 pass their registered golden checks, including direct multiplot selection parity; 12 real API-backed browser cases plus a real-pointer visualization assignment regression pass. Single-file reads, multi-group planning snapshots, project candidate discovery, and import publication now coordinate through the project lock across cooperating store users. A cross-process reader test confirms lock blocking on Linux; external writers that ignore this advisory lock remain outside the coordination boundary. A real Linux WebKit pointer-lasso assignment and close/reopen check now pass, including full row preservation; the run also fixed collapsed plot sizing that covered assignment controls. Local browser runs verify the 99,960-point fixture; both numerical and browser runners enforce explicitly supplied limits without inventing a default acceptance threshold.
- Phase 7 (hosted auth and operations): implemented and verified locally — see the checkpoint under Phase 7 below. Remaining exit gates are external: hosted CI verification (blocked by the GitHub Actions billing/spending-limit failure requiring owner action), threat-model findings sign-off, and the browser-level security e2e suite. Phase 8's account transition is conditional on an authorized product decision (Section 14.2); the 14.3.1 lifecycle rehearsal and 14.3.2 no-analytical-content schema audit are implemented and passing. Phase 9 (legacy removal) remains gated on the rollback window and archival tag, which require a deployed release.

Cross-platform numerical CI, a separate API-backed browser CI workflow, and the numerical timing/peak-memory capture workflow provide repeatable acceptance evidence. Hosted runs on Ubuntu, macOS, and Windows are green: numerical parity, lock regressions, Rust fmt/clippy/tests, TypeScript, browser E2E, desktop-workspace tests (Tauri shell compiles and tests on macOS/Windows), desktop-launch smoke (the real binary starts and survives startup on macOS/Windows), and three-platform timing/memory captures (a hosted 99,960-point multiplot render measured 7,341 ms). On 2026-10-01 the owner ratified the portable performance budgets (numerical bench ≤10 s elapsed, ≤128 MiB peak RSS where measurable — Windows enforces elapsed only — and multiplot render ≤15 s) as enforced workflow-dispatch defaults, and accepted the native-platform evidence: the interactive pointer-lasso walkthrough on Linux plus Windows/macOS compile, test, and launch-smoke CI. See the [evidence checkpoint](Vault/Phase_6_Repeatable_Acceptance_Evidence_2026-09-30.md), the [hosted cross-platform evidence](Vault/Phase_6_Hosted_Cross_Platform_Evidence_2026-09-30.md), and the [ratified budget note](Vault/Phase_6_Performance_Budgets_Ratified_2026-10-01.md).

See [remaining controls and jobs](Vault/Phase_6_Remaining_Controls_Jobs_2026-09-28.md), [validation report](docs/operations/phase-6-validation-2026-09-28.md), [Phase 6 checkpoint](Vault/Phase_6_Service_Integration_2026-09-28.md) and [automatic assignment checkpoint](Vault/Phase_6_Automatic_Assignment_2026-09-28.md) for validation evidence and remaining work.

Prepared: 2026-09-08

Revised: 2026-09-09 — split into sub-documents under `docs/implementation/` (sections 4–14; numbering and cross-references unchanged); corrected the Section 7.1 Calamine/CSV-TSV scope; added parity classes (Section 8.0) so stochastic-method parity is distributional where the R baseline is unseeded; added the Section 15.4 `INAA_test.csv` baseline benchmark plan with phase re-execution gates; added Section 17.1 recommended best-practice defaults for the Section 17 open questions (numerical-backend and `umap_rs` spikes moved to Phase 0).

Scope amendment: 2026-09-14 — legacy MySQL analytical data is explicitly out of scope and is not inventoried, extracted, reconciled, or migrated. The new runtime has no MySQL dependency. PostgreSQL is limited to necessary identity/contact and authentication-security records; analytical and other user content remains file-first. See [Phase 0 production inventory](docs/operations/phase-0-production-inventory-2026-09-14.md).

Architecture amendments: 2026-09-08 — MySQL is removed; PostgreSQL is hosted control-plane only; each analytical group is a self-describing Apache Parquet file; the opened directory is the project/workspace boundary; and calculated elemental transformations are recomputed on demand rather than written into group files

Scope reviewed: every tracked application source, UI asset, test, package/deployment file, security script, generated document, and known legacy/prototype path in this repository

## 1. Outcome and architectural decision

Rebuild ArchaeoDash as one TypeScript/React client and one shared Rust application core, delivered in two modes:

1. **Desktop:** the React client runs in a Tauri 2 webview. Tauri commands call the Rust application services in-process. The user opens a directory as a project/workspace. Self-describing group Parquet files may live anywhere inside it, and source spreadsheets may remain anywhere inside it; no account or network connection is required.
2. **Hosted web:** the same compiled React client calls an Axum HTTP API. Users authenticate and receive an equivalent private project namespace. Group Parquet files, source spreadsheets, transformation definitions, and explicit result exports live in the user's file store. PostgreSQL stores only authentication, ownership/catalog, active-pointer, quota, preference, job, and audit state—not dataframe rows.

The Node.js requirement is fulfilled by the TypeScript client workspace, package manager, build, test, and developer tooling. The production web client should be compiled to static assets and served by the Rust service or a CDN/reverse proxy. A separate Node.js production server is intentionally **not** part of the default architecture: it would duplicate routing, authentication, deployment, and observability already owned by Axum. If server-side rendering becomes a real requirement, add a Node BFF as a separately justified ADR; do not make desktop depend on it.

The critical design rule is that the desktop and hosted products are adapters over the same Rust use cases—not separate implementations and not a desktop app that talks to a hidden loopback HTTP server.

MySQL is not part of the new runtime. The shared persistence model is file-first: group membership, original measured elemental values, descriptive columns, provenance, and role metadata are stored in an open ArchaeoDash Group Parquet profile. Any metadata-complete Parquet file inside the opened project is discoverable as a candidate; full validation occurs when it is added to the active project selection. A manifest is an index and history aid, never an eligibility gate.

```text
                         packages/client (React + TypeScript)
                          /                            \
               web transport adapter             Tauri IPC adapter
                       |                                 |
                 Axum HTTP API                    Tauri commands
                       \                                 /
                    crates/application (use cases, jobs, policy)
                                  |
              crates/domain + crates/analysis + crates/data-io
                         /                         \
       hosted control/file adapters       desktop project/file adapters
       PostgreSQL + user file store       group Parquet + project metadata
```

This structure is consistent with Tauri's documented Rust/webview message-passing architecture. Its file-system APIs are scope-controlled and its dialog plugin supplies native open/save dialogs. Axum provides composable routing over shared state. The Apache Arrow `object_store` crate supplies one Rust abstraction for durable local files and S3-compatible hosted objects, including atomic and conditional operations. Polars supplies the main Rust dataframe layer, but its Rust API does not natively write Excel; therefore XLSX import/export must use dedicated libraries. See [Tauri architecture](https://v2.tauri.app/concept/architecture/), [Tauri file-system security](https://v2.tauri.app/plugin/file-system/), [Tauri dialogs](https://v2.tauri.app/plugin/dialog/), [Axum Router](https://docs.rs/axum/latest/axum/struct.Router.html), [object_store](https://docs.rs/object_store/latest/object_store/), [Polars I/O](https://docs.pola.rs/user-guide/io/), [Calamine](https://docs.rs/calamine/latest/calamine/), and [rust_xlsxwriter](https://docs.rs/rust_xlsxwriter/latest/rust_xlsxwriter/).

## 2. Non-negotiable migration principles

- Every reachable current behavior receives a parity test and a destination in the new design.
- Every unreachable, obsolete, generated, or prototype item receives an explicit retirement or archive decision in this document.
- The R app remains the behavioral oracle during migration, but no R interpreter or R package is required by the final desktop or hosted runtime.
- Statistical parity means explicitly documented semantics, tolerances, seeds, ordering, and failure behavior. It does not mean assuming two libraries produce byte-identical floating-point results. Every statistical output is assigned exactly one parity class — exact, tolerance, or distributional — per Section 8.0 and the baseline benchmark plan in Section 15.4. Stochastic methods whose current R execution is unseeded (UMAP, MICE imputation) target the documented algorithm and distribution, never a historical run's exact output.
- User data is never identified by a database table name. Stable UUID/ULID identifiers and ownership checks replace user-prefixed dynamic tables and user-controlled storage paths.
- Each group Parquet file is an authoritative, portable working-data unit. It stores only imported measured elemental values, descriptive data, hidden identity, group identity, column roles, units, and provenance. It never stores logged, normalized, standardized, imputed, permuted, ratio-derived, or ordination values.
- Source spreadsheets are optional provenance inputs and may remain at any user-chosen location inside a desktop project. Desktop import never requires or performs a byte-for-byte copy into a special `sources/` directory. Hosted mode is the defined exception: uploading a source spreadsheet necessarily stores one preserved byte-for-byte object in the user's `UserFileStore` (Section 6.4); this principle governs desktop local paths and never prohibits that hosted upload path.
- Arrow and temporary Parquet outside the group-file profile are disposable computation/transport caches. Analysis transformations and permutations are rerun on demand from original measured values plus saved definitions and seeds.
- The desktop application supports full user-directed local file manipulation through Rust commands and native dialogs. Do not expose blanket home-directory access or an unrestricted filesystem API to webview JavaScript.
- Hosted authentication uses server-set `HttpOnly`, `Secure`, `SameSite` cookies and CSRF protection. Bearer tokens are never exposed to client JavaScript.
- Secrets belong in runtime secret stores or environment injection, never Git, client bundles, logs, project files, or Vault notes.
- Long analyses are jobs with progress, cancellation, timeout, and resource limits in both delivery modes.
- Legacy MySQL analytical data is deliberately not migrated. If legacy accounts require transition, handle only an authorized minimal identity/contact transition; do not copy analytical data, preferences, transformations, or result data into PostgreSQL.

## 3. What “parity” includes

The parity baseline includes all currently reachable workflows:

- home, help, terms, privacy, navigation, responsive sidebar, three themes, notices, and loading/error states;
- account registration, email verification, login, remembered login, logout/reset, password reset, and preferences;
- anonymous/local source imports and authenticated/server projects with group files;
- CSV/XLSX import, column selection, ID choice, chemical/predictor selection, blank/zero/negative/NA policies, replace/append, and type harmonization;
- hosted file/project discovery, last-opened preference, multi-select load, merge/rename, overwrite confirmation, delete, and multi-file writeback;
- descriptive/group column selection, subgroup counts and selection controls, numeric column inference, ratios, imputation, transforms, PCA, UMAP, LDA, named transformation save/load/delete, and metadata refresh;
- editable data table, missingness plot, crosstabs, histogram, and compositional profile;
- element/PCA/UMAP/LDA scatterplots, metadata filters, ellipses, symbols, labels, lasso selection, selected-row display, group reassignment, multiplots, and plot export;
- PCA individual/variable/eigenvalue/contribution views and LDA vector view;
- optimal-cluster diagnostics, HCA, divisive hierarchical clustering, k-means, k-medoids, expanded plots, result tables, and recording assignments;
- group sizes, eligibility, Hotelling's T2 membership probabilities, Mahalanobis fallback/distances, PC-count selection, result selection, best-group assignment, and manual reassignment;
- Euclidean nearest matches, source/group/ID/PC-count/within-group/limit controls, result selection, matched-group assignment, and manual reassignment;
- chemical, PCA, and membership-result export;
- row identity propagation, autosave, notifications, verbose/timing logs, timeouts, dependency failures, health checks, and deployment controls.

### 3.1 Guest, desktop, and account behavior

- Desktop projects are durable local data without login.
- Hosted authenticated workspaces are durable server data under an account.
- Hosted guests may retain the current no-account workflow, but their group/source/result files live in an isolated ephemeral namespace with a short, published TTL and hard quotas. The UI displays the deletion time and continuously offers group/result/project download or account creation. Guest data is never discoverable through the authenticated catalog.
- Promoting a guest workspace into an account is an explicit, transactional claim operation after authentication; it does not occur merely because the user logs in.
- If hosted catalog/session storage is unavailable, readiness fails and the service presents a generic outage page. It must not imply that temporary browser memory is durable. Desktop mode remains independent.
- Desktop-to-server sync, collaboration, and shared datasets are outside the parity release. They require a future authorization/conflict/encryption design and cannot happen implicitly.

### 3.2 Intentional corrections rather than bug-for-bug ports

| Current behavior or ambiguity | New decision |
|---|---|
| Explore edits index `importedData` using the displayed `rowid` as a dataframe position | Resolve the hidden immutable `analytical_uuid` through the repository; never positional write |
| `saveexportTab` reads nonexistent `rvals$pcaData` | Export the computed PCA score result (`pcadf` equivalent) |
| Ordination computes `umapheader` but renders no UMAP tab | Render an explicit UMAP tab/view |
| PCA contribution table assumes at least four PCs | Sum the first `min(4, component_count)` and label the rule |
| LDA vector plot assumes LD1 and LD2 | Validate dimensions and render a one-dimensional alternative/error state |
| Runtime dependency checks can make features disappear after installation | Compile/package required capabilities and expose a deterministic capability manifest |
| Broad `rio` formats depend on the installed environment | Publish and enforce an explicit tested format allowlist |
| JavaScript can read the remembered-login bearer token | Server-set HttpOnly cookie; no token in client state/local storage |
| In-process login rate limits reset and do not coordinate | Persistent distributed-safe throttling |
| Dynamic table prefixes infer ownership | Foreign-key ownership and authorization on opaque IDs |
| Group moves/copies can partially rewrite related files | Stage every affected Parquet replacement, validate identities and measured values, journal the transaction, then publish all replacements or none |
| Statistical randomness is not recorded | Every stochastic run records seed, algorithm version, and configuration |
| Errors/warnings are frequently swallowed by `try`, `quietly`, or `silent` | Typed error/warning results with safe user text and structured diagnostics |
| Interactive multiplot silently downsamples | Preserve the ceiling but display deterministic sampling status/count |
| Current server invokes an unreachable legacy subset service | Retire it explicitly; do not create a hidden compatibility endpoint |

## Sub-document index

Sections 4-14 are maintained as standalone sub-documents under `docs/implementation/`. Section numbers are unchanged and internal cross-references remain valid.

| Section | Document | Contents |
|---|---|---|
| 4 | [repository-structure](docs/implementation/04-repository-structure.md) | Monorepo layout, legacy R quarantine |
| 5 | [shared-domain-model](docs/implementation/05-shared-domain-model.md) | Opaque IDs, entities, rvals decomposition |
| 6 | [storage-architecture](docs/implementation/06-storage-architecture.md) | Storage traits, group Parquet profile, concurrency, backup/quota |
| 7 | [import-export-data-semantics](docs/implementation/07-import-export-data-semantics.md) | Formats, import/export contracts |
| 8 | [statistical-parity](docs/implementation/08-statistical-parity.md) | Pipeline order, parity classes, transformations, PCA/UMAP/LDA, clustering |
| 9 | [client-application](docs/implementation/09-client-application.md) | React client, transport abstraction, routes |
| 10 | [hosted-http-api](docs/implementation/10-hosted-http-api.md) | HTTP routes, desktop command surface |
| 11 | [authentication-privacy-security](docs/implementation/11-authentication-privacy-security.md) | Auth, upload/data controls, desktop controls, legal |
| 12 | [jobs-performance-observability](docs/implementation/12-jobs-performance-observability.md) | Jobs, performance, observability |
| 13 | [source-disposition](docs/implementation/13-source-disposition.md) | Every current file: port/replace/retire/archive |
| 14 | [legacy-data-migration](docs/implementation/14-legacy-data-migration.md) | Legacy identity/contact transition and analytical-data non-migration |

## 15. Test and validation strategy

### 15.1 Golden parity harness

Before replacing R code, add an R oracle CLI that accepts a fixture/config and emits canonical JSON/Arrow outputs plus package/session versions. Generate goldens for:

- clean names — including the `janitor::clean_names(case = "none")` compatibility edge cases: duplicate/colliding names, special characters, non-ASCII/Unicode, empty and blank headers, and numeric-looking headers — plus numeric inference, missing flags, blank metadata, append coercion, row mapping;
- ratios and every transformation;
- each imputation method with fixed seeds and edge cases;
- PCA eigenvalues/variance/scores/loadings; UMAP embeddings/neighbor-quality metrics; LDA priors/scaling/scores;
- every clustering method/config and diagnostics;
- membership eligibility, Hotelling, fallback, Mahalanobis, best group;
- Euclidean top-N with same/other groups and duplicate display IDs;
- crosstabs, missing profiles, plot-ready data, and export tables.

Exact comparisons apply to IDs, strings, schemas, counts, filtering, assignments, and deterministic integer outputs. Floating-point tests use algorithm-specific absolute/relative tolerances, sign/permutation alignment, invariant comparisons, and distribution/neighborhood metrics where exact trajectories are inappropriate. Every golden is tagged with its parity class (Section 8.0). The concrete baseline procedure list, fixture, and re-execution gates are defined in Section 15.4.

### 15.2 Test layers

- Rust unit/property tests for all invariants, numerical edge cases, parsers, and serialization.
- Group-profile, scanner, and transaction contract tests run on Windows/macOS/Linux filesystems. Cover arbitrary in-project paths, metadata-only readiness, validation-on-add, unsupported/missing metadata, duplicate group IDs/UUIDs, schema/units, atomic multi-group publication, interrupted writes/recovery, manifest recreation, checksums, reference editability, and cache reconstruction.
- `GroupFileStore`/`UserFileStore` contracts run against local filesystem and the selected S3-compatible test service, including conditional conflicts, multipart abort/cleanup, version IDs/ETags, range reads, soft deletion, and authorization wrappers.
- Hosted control-plane repository contracts run against PostgreSQL and assert that no analytical-unit rows or elemental values are persisted there.
- API contract tests validate auth, authorization, CSRF, errors, Arrow/JSON negotiation, revisions, and jobs.
- Client unit/component tests validate selection transitions, dialogs, accessible names, table state, and transport errors.
- Playwright web e2e covers every parity workflow on desktop and mobile viewports.
- Tauri e2e covers project-directory selection; sources anywhere inside it; recursive group discovery; readiness/validation states; import splitting; no-group import; group moves/copies/duplicates; reference edit enablement; exact measured-value preservation; confirmed file move/rename/delete isolation; local recovery; offline operation; and parity adapter behavior on Windows/macOS/Linux.
- Visual regression covers all three themes, plots, tables, modals, responsive sidebar/filter layout, and loading/error states.
- Security tests cover cross-user forged IDs, session fixation/replay/logout, token expiry/reuse, rate limits, upload bombs/malformed formats/formula injection, path traversal, CSP, CORS, CSRF, and log redaction.
- Performance/soak tests cover job cancellation, repeated on-demand transformations/permutations, concurrent users, candidate scanning, large result pagination, Parquet projection/predicate pushdown, and memory ceilings.

Mandatory storage-invariant tests inspect every group Parquet written by every command and fail if it contains a derived role or values for logs, ratios, normalization, standardization, imputation, permutation, PCA, UMAP, LDA, or clustering. Move/copy/duplicate/edit tests compare canonical measured-element tuples before and after. UI tests assert that `analytical_uuid` is absent from normal tables, selectors, labels, and ordinary exports while identity-dependent selection remains stable.

### 15.3 Release gates

- `cargo fmt --check`, Clippy with warnings denied for workspace code, Rust tests, audit/deny/license checks;
- ESLint, TypeScript strict check, unit/component tests, dependency audit, production build;
- PostgreSQL control-plane, group-profile, and project-metadata migrations, compatibility policy, clean boot, and recovery fixtures;
- parity suite and documented exceptions approved by product/statistical owner;
- required web and desktop e2e with no silent skips;
- container/image and desktop artifact scans, SBOMs, signatures, and provenance;
- accessibility and visual regression;
- backup/restore and, if needed, identity/contact transition rehearsal;
- performance budgets and threat-model review.

### 15.4 Baseline benchmark plan (`INAA_test.csv` oracle)

A fixed benchmark suite runs a selected set of current R procedures against `inst/app/INAA_test.csv` (canonical fixture copy: `fixtures/INAA_test.csv`, SHA-256 recorded in the manifest) **before any replacement code lands**, and the identical procedure list is re-executed against the new architecture at each phase gate and again before cutover. This makes "the new architecture reproduces the old behavior" a measured claim instead of an aspiration.

Baseline capture rules:

- Each procedure runs through the R oracle CLI (Section 15.1) with the recorded R version, package versions (`umap`, `mice`, `MASS`, `cluster`, `factoextra`, `ICSNP`, and others as used), RNG kind, and seeds where applicable.
- Each procedure's captured output, tolerance, and parity class (Section 8.0) are stored under `fixtures/golden/` with a manifest entry.
- `INAA_test.csv` is read-only; baselines are regenerated only from a new tagged R release, never by hand-editing goldens.
- Procedures that `INAA_test.csv` does not exercise (append workflows, non-INAA formats, large-N behavior) require their own fixtures before their phase exit; this suite does not substitute for them.

Baseline procedures (current R entry points in parentheses):

| # | Procedure | Current R entry point | Parity class | Baseline capture |
|---|---|---|---|---|
| 1 | CSV import: numeric inference (1,500-row/95% rule), default INAA elemental list, ANID default, group partition | `DataLoader.R`, `columnTypeHints.R` | E | inferred types, column order, row counts, group partitions |
| 2 | `zScore` (compositional percent + column z-score, 3-decimal rounding) | `zScore.R` | E | full transformed matrix |
| 3 | `log10` / `log` transforms (rounding + non-finite-to-zero, warning counts) | `datainputTab.R` transform block | E | full transformed matrix + warning counts |
| 4 | Ratio construction and application (dedup, zero/null denominator rule) | `datainputTab.R` ratio specs | E | full matrix incl. edge rows |
| 5 | Imputation `none` / `pmm` / `midastouch` / `rf` (seeded) | `mice`-based flow via `datainputTab.R`/`transformationStore.R` | E (`none`), D (others) | imputed matrix + missing-cell map |
| 6 | PCA (`prcomp` defaults), variance/cumulative labels, PC-count limiting | `ordinationTab.R`, `pcaHelpers.R` | T | scores, rotation, sdev, explained/cumulative variance |
| 7 | UMAP embedding (current defaults, unseeded) | `ordinationTab.R` | D | embedding + neighbor/Procrustes metrics vs. seeded re-runs |
| 8 | LDA fit/scores/vector data (3-group minimum rule) | `lda.R`, `Group_probs.R` selection | T | priors, scaling, scores (sign/axis aligned) |
| 9 | Clustering: k-means (centers 1–20, starts 1–100, iter 1–200), k-medoids, HCA linkages, DIANA, WSS/silhouette diagnostics | `clusterTab.R` | T | cluster labels (permutation-aligned), WSS/silhouette series |
| 10 | Membership probabilities: eligibility `n > max(n_features, n_groups) + 1`, Hotelling T² (round 5, ×100), Mahalanobis fallback, `BestGroup` | `Group_probs.R` | E (assignments) / T (values) | full per-row probability table |
| 11 | Euclidean nearest matches (top-N 1–100, within-group toggle, self-exclusion, tie order) | `EuclideanDistance.R` | E | matched pairs and distances |
| 12 | Explore views: missing-value bands (Good ≤5% / OK ≤40% / Bad ≤80%), crosstabs, histogram bins (2–100, default 30), compositional profile | `plot_missing.R`, `exploreTab.R`, `comp.profile.R` | E | counts, labels, band assignments |
| 13 | Multiplot interactive sampling ceiling (100,000 points) determinism | `plot.R` | E | sampled index set |
| 14 | Measured-data export round trip (CSV cell/schema exactness) | `saveexportTab.R` | E | exported table vs. source values |

Re-execution gates:

1. **Phase 0 exit:** all 14 baselines captured from the tagged R release with manifest, versions, seeds, and parity classes approved.
2. **Phase 3 exit (transformations):** procedures 1–5 re-run in Rust and pass within declared classes.
3. **Phase 4 exit (ordination/Explore):** procedures 6–8 and 12 re-run and pass.
4. **Phase 6 exit (cluster/membership/Euclidean):** procedures 9–11 and 13 re-run and pass.
5. **Pre-cutover (Phase 8):** the complete suite re-runs end-to-end on the final architecture; any parity-class exception is documented and signed off per Section 15.3.

## 16. Delivery phases and exit criteria

### Phase 0 — Freeze the behavioral contract

- Bootstrap the developer environment with `install_dev_prereqs.sh` (system packages, Rust/pnpm/sqlx/tauri-cli toolchain, R oracle dependencies; see the vault note [[Vault/Install_Dev_Prereqs_Script_2026-09-10]]). Requires R ≥ 4.4 — the script enables the CRAN apt repository on Ubuntu 24.04.
- Tag the current R baseline; capture R/package/database/deployment versions.
- Inventory actual production formats, dataset sizes, file-store requirements, and external Compose/proxy settings; do not inspect or use MySQL analytical tables.
- Build oracle fixtures and screenshots for every workflow.
- Capture the Section 15.4 `INAA_test.csv` baseline benchmark suite through the R oracle and assign parity classes (Section 8.0) to every procedure.
- Complete the numerical-backend spike (Section 17.1 item 2) and the `umap_rs` spike (Section 17.1 item 3): confirm or overturn each recommended default with a recorded decision, because the Section 15.4 parity gates depend on their outcomes.
- Resolve statistical tolerances and legal text ownership.

Exit: signed feature/disposition matrix, reproducible R oracle, Section 15.4 baseline benchmarks captured and classified, numerical-backend and `umap_rs` spike decisions recorded, sanitized production inventory, and no unidentified tracked app file.

### Phase 1 — Monorepo and domain skeleton

- Create Rust/Node workspaces, toolchain pins, CI, typed IDs/entities/errors, contracts, backend port, design tokens, and empty Tauri/Axum compositions.
- Publish the ArchaeoDash Group Parquet Profile and fixtures; add project-metadata schemas/migrations, local/S3 `GroupFileStore` and `UserFileStore` contracts, PostgreSQL control-plane migrations, and repository contract suites.

Exit: same smoke use case executes through HTTP and Tauri adapters; lockfiles and required CI are green.

### Phase 2 — Group files, local projects, and hosted file catalog

- Implement secure source-format adapters, project-contained source selection, recursive candidate scanning, metadata readiness, full validation-on-add, import preview/partitioning, profile-valid group creation, hosted private logical paths/catalog, group list/load/create/delete/merge/duplicate, reference-group edit options, provenance, caches, and explicit group/result export.
- Add descriptive editing, move/copy/split analytical-unit operations, measured-element immutability enforcement, transaction journals, version conflicts, conditional publication, and interrupted multi-group recovery.

Exit: INAA and every format fixture produce the expected one-file-per-group outputs; any metadata-complete in-project Parquet candidate is discoverable regardless of folder/manifest entry; every added group passes full validation; no import copies a source into a required folder; measured checksums survive all group operations; interrupted publication and authorization tests pass; PostgreSQL contains no analytical-unit rows.

### Phase 3 — Transformation engine

- Port selection/group controls, ratios, none/log/log10/zScore, named definitions, load/delete, metadata refresh, permutation configuration, job progress/cancel, and ephemeral analysis results.
- Implement and gate each imputation method.

Exit: transformation goldens pass; no R runtime dependency; interrupted jobs leave no partial result exports; inspection proves no calculated elemental values were written into group files.

### Phase 4 — Ordination and Explore

- PCA, UMAP, LDA, descriptive-edit table with elemental columns locked, crosstabs, missing/histogram/profile views, preferences, and exports.

Exit: numerical and visual parity accepted on all sources/themes/viewports.

### Phase 5 — Visualize and assignment

- Interactive plot, ellipses/symbols/labels/filter/lasso/selected table, assignment transaction, multiplots and plot saves.

Exit: hidden `analytical_uuid` propagation and atomic analytical-unit transfers pass web/desktop e2e; UUIDs remain absent from normal UI; plot performance budgets pass.

### Phase 6 — Cluster, membership, and Euclidean

- All cluster families/diagnostics/dendrograms, Hotelling/Mahalanobis membership, exact nearest matches, result tables, and assignment paths.

Implementation checkpoint (2026-09-30): source/metric controls, bounded jobs, progress/cancellation, partition and hierarchy plots, membership fallback/projection metadata, reviewed assignments, and desktop project selection are implemented. Registered procedures 9–11 and direct procedure 13 sampling parity pass. See the [validation report](docs/operations/phase-6-validation-2026-09-28.md) for exact coverage and open acceptance gates; Linux native assignment/reopen passes, hosted cross-platform CI (numerical parity, locks, browser E2E, timing/memory capture) is green, and the remaining acceptance work is native macOS/Windows desktop acceptance and ratified portable performance budgets. This is not a cross-platform phase exit sign-off.

Exit: full statistical parity matrix approved; fallback/method metadata visible; large-job limits/cancellation pass.

### Phase 7 — Hosted auth and operations

- Registration, verification, sessions, remember option, password reset, email adapters, persistent throttles, preferences, headers/CSP, observability, backups, deploy/rollback.

Implementation checkpoint (2026-10-03): the hosted auth surface is implemented and verified end to end — auth primitives crate (Argon2id, opaque tokens, normalization, peppered throttles), PostgreSQL control plane (users/sessions/account_tokens/auth_throttles/preferences via SQLx migrations with a read-only readiness gate), all Section 10.1 auth and preference routes with cookies/CSRF/enumeration resistance, email adapters (sendmail/SMTP/gated dev sink), enforced security headers, correlation IDs and PII-free structured request logs, health live/ready, and a `hosted` server binary. Deployment artifacts (`deploy/compose`, multi-stage non-root `deploy/container/Dockerfile`, orchestrated-policy healthcheck), a coordinated backup script with watermark and file-store manifest, and a verified restore drill replace the legacy host-mutating deploy flow. DB-backed HTTP tests (register→verify→login→logout/reset/preferences against `postgres:16-alpine`) pass with a live DB; the restore drill passes and refuses tampered bundles. Rotating desktop logs with `RUST_LOG` level config are wired into the Tauri shell (`apps/desktop/src-tauri/src/logging.rs`), and the Section 14.3.1 auth-lifecycle rehearsal (register → verify + replay rejection → session reflection → remembered login → logout-all revocation → password reset → re-login) passes against live PostgreSQL. Remaining Phase 7 exit gates are external: hosted CI verification (blocked by the GitHub Actions billing/spending-limit failure — owner action), threat-model findings sign-off, and the browser-level security e2e suite (Phase 6 browser CI shares the billing block).

Exit: threat-model findings closed or explicitly accepted; auth lifecycle/security e2e and restore drill pass.

### Phase 8 — Identity/contact transition and cutover

- If legacy accounts must be retained, rehearse the authorized minimal identity/contact transition and reset/verification flow; do not import MySQL analytical data. Update Help/Terms/Privacy, beta desktop installers and hosted environment, user acceptance, final delta/cutover/rollback window.

Exit: any authorized account transition is exact within approved identity/contact rules, signed installers/images published, rollback proven, old app read-only, support runbook active.

### Phase 9 — Remove legacy runtime

- Remove R runtime/package/generated assets/scripts only after the rollback window and archival tag.
- Preserve oracle outputs, statistical specifications, licenses/credits, and migration reports under retention rules.

Exit: no production route, image, installer, or build depends on R/Shiny/MySQL dynamic tables; repository scan finds no secrets or orphaned legacy build artifacts.

## 17. Decisions made now and decisions that require evidence

### Made in this plan

- Tauri 2 desktop + shared React client + Axum hosted API.
- Node toolchain but no default Node production server.
- One Rust application/analysis core shared by IPC and HTTP.
- Standard self-describing ArchaeoDash Group Parquet files are authoritative working data in desktop and hosted modes; the profile is openly documented and fixture-tested.
- The opened directory/hosted namespace is the project boundary. A conforming Parquet file may live anywhere inside it and is discoverable without prior registration; metadata earns a **Ready to add** state and full validation is mandatory on add/load.
- Source spreadsheets may remain anywhere inside the project and are never forcibly copied to `sources/`; any local SQLite/manifest index is a disposable or rebuildable accelerator, not an eligibility gate.
- Hosted mode uses a private filesystem or S3-compatible `UserFileStore`; PostgreSQL is control-plane only and stores no analytical-unit rows or elemental values.
- `analytical_uuid` is the hidden immutable row identity. User language says “analytical unit,” not “artifact,” for analyzed rows.
- Original measured elemental values are immutable inside group files. Logs, ratios, standardization/normalization, imputations, permutations, and analysis outputs are rerun on demand; only definitions/configuration/seeds persist, with separate explicit result exports allowed.
- Arrow and non-profile Parquet are rebuildable computation/transport caches or explicit exports, distinct from canonical group Parquet files.
- Groups imported from other research are read-only references by default but always offer an explicit editable clone and, where permissions allow, in-place edit enablement with retained provenance.
- No R runtime in the final product.
- No dynamic per-user database tables.
- CDA, legacy subset module, ternary/classification prototypes, generated R/Quarto/help dependencies, and Shiny-specific workarounds are retired as detailed above.
- UMAP receives a real Ordination view; PCA export uses PCA scores.
- Broad environment-dependent `rio` format catch-all is replaced by an explicit tested allowlist.

### Must be answered by spikes/inventory, without reducing scope

- Exact current MICE/UMAP/factoextra/cluster defaults and acceptable parity tolerances.
- Best maintained numerical backend for reproducible PCA/LDA and RF imputation across target CPUs.
- Whether `umap_rs` plus chosen KNN/init meets quality, determinism, and panic-safety gates.
- Whether DIANA/Ward.D require custom implementation.
- Actual maximum dataset sizes and acceptable hosted quotas.
- Single-host filesystem or S3-compatible production user-file backend and its object-versioning/lifecycle policy.
- Whether any format beyond CSV/TSV and generated XLSX earns a tested same-format writeback or full-structure round-trip claim.
- Exact footer-metadata encoding/canonical checksum rules and compatibility evolution for the public group profile.
- Supported desktop OS versions and signing/notarization ownership.
- Retention/deletion/restore windows and legal cookie/text decisions.
- Whether SVG/PDF plot export is required at parity launch in addition to tested PNG.
- Control-plane SQL toolkit for the PostgreSQL repository (SQLx vs. tokio-postgres vs. Diesel): already assumed by Sections 6.5 and 13.2 but never named once in the workspace crates.

These are implementation tasks with explicit defaults and gates, not reasons to omit a current workflow.

### 17.1 Recommended defaults for the open questions

Best-practice defaults for each open question above. Each default remains gated by its spike or inventory evidence: a spike either confirms the default or overturns it with a decision and rationale recorded in this section. The numerical-backend (2) and `umap_rs` (3) spikes complete in Phase 0 because the Section 15.4 parity gates depend on their outcomes; the remaining spikes complete by their owning phase exit at the latest.

1. **R defaults and parity tolerances.** Capture defaults mechanically, not from documentation: the Phase 0 oracle run records package versions, RNG kind, default arguments (`mice`, `umap::umap`, `factoextra`, `cluster::diana`, `hclust` linkage parameters) from the tagged R release into each golden's manifest entry. Tolerances follow the Section 8.0 classes: class E is exact including rounded contracts; class T starts at relative 1e-6 same-platform and 1e-4 cross-platform for eigen-based outputs (PCA/LDA/Hotelling/Mahalanobis), 1e-9 absolute for Euclidean distances, and exact-after-permutation-alignment for cluster assignments; class D uses fixed-seed goldens on the new implementation plus distribution metrics against the R oracle (k-NN overlap/trustworthiness), never trajectory equality. Every tolerance is a manifest entry re-approved at each Section 15.4 gate.
2. **Numerical backend.** Decide in Phase 0. Default: a pure-Rust deterministic linear-algebra core (`faer` or `ndarray`-based) for the PCA/LDA/Mahalanobis/Hotelling computations so goldens reproduce across Windows/macOS/Linux without BLAS build variance; multithreaded BLAS is enabled only where a class-T tolerance is re-approved for it. Random-forest imputation uses a pinned, audited pure-Rust regressor (e.g., Linfa RandomForest) with recorded seed and tree/feature defaults, validated distributionally against R `mice rf` goldens. Gate: the same class-T golden passes unchanged on all three CI platforms, with backend and version recorded in every golden.
3. **`umap_rs` adoption.** Time-boxed Phase 0 spike against Section 15.4 procedure 7: caller-supplied exact brute-force KNN for up to the 100,000-row plot ceiling, fixed-seed deterministic initialization, pinned epochs/learning schedule, input validation, and panic isolation at the job boundary. Quality gate: k-NN overlap/trustworthiness against the R `umap` oracle across at least three seeds within a Phase 0-ratified threshold (class D). Fallback: reimplement the UMAP layout on the chosen backend, or hold UMAP behind the experimental-parity flag until it passes; never ship an unvalidated, panicking path.

   **Decision (2026-09-20, Phase 4 slice): `umap-rs` not adopted; hand-ported legacy naive UMAP.** The available crates (`umap-rs` 0.4.x and similar) do not implement R `umap` 0.2.10.0's `method = "naive"` semantics (legacy smooth-knn, fuzzy union, epoch-sampled SGD with the epoch schedule in C `optimize.cpp`, spectral init through RSpectra `eigs(which = "SM")`), and none guarantee the caller-side determinism and validation the gate requires. `crates/analysis/src/umap.rs` instead ports the naive pipeline exactly on `faer` (self-adjoint eigen under `Parallelism::Serial`) with a seeded ChaCha12 stream standing in for R's MT19937 draws (spectral jitter, negative sampling) — documented class-D divergence. Golden 07 ratifies the class-D thresholds: R-vs-R cross-seed calibration band is pdist correlation 0.893-0.974 / 10-NN overlap 0.852-0.863; the Rust port measures 0.900-0.962 / 0.854-0.856 across seeds 20260914-20260916 against `fixtures/golden/07_umap.json`, so the provisional gates corr >= 0.75, overlap >= 0.70 hold with margin, plus a bit-identical same-seed determinism test. Porting note worth keeping: RSpectra `eigs(k = d + 1, which = "SM")` returns the selected eigenpairs in DESCENDING modulus order, so R's `[, seq_len(d)]` keeps the d largest-modulus columns of the d+1 smallest and drops the trivial near-null eigenvector; sorting ascending (as a dense solver naturally does) silently produces a different, much worse init (observed corr 0.70-0.81).
4. **DIANA / Ward.D.** Procedure-9 Phase 0 goldens decide. Default: use maintained crates where parity passes (`kodama` covers Ward linkages and stepwise dendrograms; `kmedoids` covers PAM); DIANA has no maintained Rust equivalent — implement the divisive algorithm in `crates/analysis` (O(n²) dissimilarity matrix plus recursive split) with ultrametric/merge-count property tests and golden dendrogram orderings. No clustering method ships without its procedure-9 golden.
5. **Dataset sizes and hosted quotas.** Phase 0 fixture/performance-ladder measurements and future file-store telemetry set the floor; MySQL analytical table sizes are out of scope. The Section 12 benchmark ladder (10k/100k/1M rows) sets the tested ceiling: 1M rows × roughly 40 elemental/descriptive columns per group file. Enforce explicit import row/cell limits with a clear error before analysis, never OOM mid-job. Hosted tiers: guest namespace hard-quotaed with published TTL per Section 3.1; authenticated default 5 GB, operator-configurable through `storage_quotas`; quota reserve-and-reconcile per Section 6.9. Publish final numbers in Help/Terms at Phase 8.
6. **Hosted user-file backend.** Keep the `object_store`-based `UserFileStore`/`GroupFileStore` traits as the only port. Production default: self-hosted S3-compatible storage (MinIO or cloud S3) with server-side encryption, object versioning on, noncurrent-version lifecycle expiry (~30 days), multipart upload with abort cleanup, and conditional PUTs via ETags. Single-host filesystem remains a supported deployment mode (atomic write-to-temp + rename + fsync, daily coordinated backup of the data volume and PostgreSQL) for small/self-hosted installs. Section 15.2 contract tests run against both backends; the choice is per deployment, never a code fork.
7. **Same-format writeback claims.** Default: XLS/XLSB/ODS are import-only (Calamine reads; edits export to CSV/TSV/XLSX/group Parquet). Same-format writeback promises exist only for CSV/TSV (value round-trip under the documented dialect normalization) and app-generated XLSX (round-trip of app-written structure). The Section 7.1 capability matrix stays authoritative; any addition requires a fixture-tested adapter and a new matrix row.
8. **Group-profile footer metadata and checksums.** Parquet key-value metadata under namespaced keys (`archaeodash.profile.v1`) carrying a canonical JSON payload: explicit `profile_version`, UTF-8, sorted keys, no insignificant whitespace; that serialization is what checksums sign. `measured_elemental_checksum` is SHA-256 over the canonical ordered `(analytical_uuid, column-id, value|null)` tuple stream in fixed schema order — never over raw file bytes. Readers accept known minor versions and fail closed on unknown majors (Section 6.6); profile evolution adds fields only, with a fixture-tested migration per version. The single canonical `analytical_uuid` physical encoding is the fixed 16-byte Parquet UUID (Section 6.6); a lowercase hyphenated string column is accepted only as a one-time migration input, canonicalized to the 16-byte form on read, and never written by any current writer.
9. **Desktop platforms and signing.** Windows 10+ (MSI/NSIS) and macOS 12+ on Intel and Apple silicon (.dmg, hardened runtime, Developer ID notarization) are the signed parity targets; Linux ships an AppImage with published SHA-256 checksums, unsigned by default. CI builds all three via the Tauri action matrix. The Tauri updater ships only with signed release manifests; updater signing keys are generated once, held in CI secret storage, and rotated with a documented migration. Signing ownership and certificate budget are assigned at Phase 1; Section 15.3 keeps the artifact scans and SBOMs.
10. **Retention, deletion, restore, and cookies.** Guest namespaces: 7-day sliding TTL with a visible countdown and continuous download prompts (Section 3.1). Authenticated data: 30-day soft-delete/restore window, then hard delete cascading through object versions, caches, and catalog rows; backups retained ~30 days; explicit export-all and delete-all requests honored within a published window. Cookies: session (HttpOnly, Secure, SameSite=Lax) plus CSRF (SameSite=Strict) plus optional rotating remember-me (30/90 days) — strictly necessary under typical ePrivacy guidance, so a banner is likely unnecessary, but that determination stays with counsel per Section 11.4. Terms/Privacy gain explicit retention and data-residency statements at Phase 8.
11. **SVG/PDF plot export.** PNG is the parity-launch format (matches current behavior). SVG ships at launch only if the chosen renderer emits it deterministically with fixture tests for text encoding and glyph fallback; PDF is deferred post-launch because a headless print pipeline is a separate dependency. Vector formats never block the parity launch; the Section 7.3 capability matrix records the final decision.
12. **Control-plane SQL toolkit.** Standardize on SQLx — compile-time-checked queries, async execution, and its migration workflow — as already assumed by Sections 6.5 and 13.2; name it in the Phase 1 control-plane crate. Adopting tokio-postgres or Diesel instead would require overturning the compile-time-checked-query default with a recorded rationale in this section.

## 18. Definition of done

The migration is complete only when:

- every item in Sections 3 and 13 is marked implemented, replaced, retired, or archived with review evidence;
- every reachable current workflow passes against both web and desktop adapters;
- all statistical methods pass approved parity/invariant suites with recorded algorithm versions and seeds;
- the Section 15.4 `INAA_test.csv` baseline benchmark suite has been re-executed end-to-end on the final architecture and passed within its declared parity classes (Section 8.0);
- local users can import, edit, analyze, save, close, reopen, and export without a server or account;
- local users can open a folder as a project, select a source anywhere inside it, import one group file per group (or one named group without a group column), and discover conforming group files anywhere inside the project;
- metadata-complete candidates show ready status, but only fully validated current revisions load into analyses; a manifest entry is neither necessary nor sufficient;
- local and hosted users can create, select, duplicate, import, edit-enable, split, merge, move, and copy groups/analytical units with provenance and recovery;
- `analytical_uuid` remains stable and hidden, while ANID remains the visible analytical-unit identifier;
- measured elemental values remain unchanged across every persisted group operation; calculated/logged/imputed/permuted values are recomputed on demand and never stored in group files;
- hosted users complete the same workflow with strict ownership enforcement and durable private file storage;
- multi-group edits publish validated Parquet revisions to the correct paths transactionally and recover deterministically after interruption;
- auth tokens are HttpOnly server sessions, verification/reset/rate limiting work across processes, and cross-user tests fail closed;
- secure upload limits, version conflicts, cancellation, recovery, logging redaction, backups/restores, health checks, and rollback are tested;
- no MySQL analytical dataset, transformation, preference, result, or other user content is copied into the new runtime; any authorized legacy account transition is limited to identity/contact records and never labels reconstructed content as an original upload;
- Help, Terms, Privacy, credits, license, issue/support links, and mode differences are current;
- signed desktop artifacts and immutable hosted images are reproducible from locked source with SBOM/provenance;
- no secrets are committed or copied into documentation/Vault; and
- the R/Shiny runtime is removed only after the accepted rollback window.
