# IMPLEMENTATION Section 13 - Complete current-to-new source disposition

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 13. Complete current-to-new source disposition

Disposition terms: **Port** means preserve behavior in supported runtime; **Replace** means preserve purpose with a new mechanism; **Retire** means deliberately exclude because it is unreachable/generated/prototype; **Archive** means retain only as migration evidence.

### 13.1 Root/package/build files

| Current path | Current purpose | Disposition |
|---|---|---|
| `DESCRIPTION` | R package metadata/dependencies/version | Replace with Cargo/npm manifests, generated product version, SBOM; archive author/description/license metadata |
| `NAMESPACE` | generated R exports/imports | Retire after function mapping below; Rust visibility/OpenAPI replace it |
| `Archaeodash.Rproj` | RStudio project | Retire after migration |
| `.Rbuildignore` | R package exclusions | Retire; replace with explicit container/package contexts |
| `.Renviron.example` | DB/email config example | Replace with sanitized `.env.example`/config schema; retain every semantic setting under renamed names; no real values |
| `.dockerignore` | container context exclusions | Replace and expand for Node/Rust builds, secrets, Vault, caches, fixtures not required at runtime |
| `.gitattributes` | shell LF policy | Keep and extend for Rust/TS/generated/binary rules |
| `.gitignore` | R/runtime secret/cache ignores | Replace/extend for `target`, Node/Tauri artifacts, `.env*`, local projects, migration backups; keep secret exclusions |
| `LICENSE` | MIT license | Keep; surface in desktop/web about UI and distributions |
| `README.md` | R install/test/config overview | Rewrite for monorepo setup, desktop/web runs, migrations, tests, supported formats, security config |
| `Repository_Fogs_Improvements_Security_Todo_2026-05-01.md` | known risks/fogs | Archive as migration input; convert unresolved items into tracked security acceptance criteria |
| `Risk_Reduction_Plan_2026-02-18.md` | currently empty plan placeholder | Retire or redirect to maintained threat model; do not present as active plan |
| `AGENTS.md`, `Vault/**` | repository operating memory | Keep outside shipped artifacts; Vault does not become app content or container input |

### 13.2 R files: every file

| Current path | Disposition and destination |
|---|---|
| `R/runApp.R` | Replace launcher with Vite dev commands, Axum binary, and Tauri run/build commands; preserve verbose option through config |
| `R/homeTab.R` | Port the welcome, help link, hosted URL, data-ownership/account guidance, beta/status wording, MIT/no-warranty notice, issue link, support attribution, and displayed version to the Home route; revise stale claims before release |
| `R/infoTab.R` | Port Help/Terms/Privacy navigation; replace the help iframe/reload JavaScript with native client routing plus an explicit open-separately action |
| `R/connect.R` | Remove runtime MySQL/DBI. Hosted mode uses a bounded SQLx pool for control-plane state and `UserFileStore` for user files; desktop uses project manifests/local files. Legacy MySQL is read-only migrator only |
| `R/DataLoader.R` | Port import/name/ANID/elemental/descriptive/blank/missing/append semantics to `data-io` and the group import wizard; read the selected in-project source in place and atomically emit one profile-valid Parquet file per group |
| `R/columnTypeHints.R` | Port sampled 95%-parse numeric inference with tests |
| `R/datasetLoadTimeout.R` | Replace process time limits with job deadlines/cancellation and stage-specific timeout errors |
| `R/datasetNameHelpers.R` | Retire physical table-name hashing; port user-visible normalized-name/collision policy only; read old names in migrator |
| `R/datasetWorkspaceLoader.R` | Port multi-dataset union as validated multi-group loading with explicit analytical-unit source map in `WorkspaceService` |
| `R/dbTableOps.R` | Replace generic table-name writes/removes with typed group-file/project operations plus hosted control-plane transactions |
| `R/userPreferences.R` | Port typed `theme` and `lastOpenedDataset` plus table preferences to `preferences` repository |
| `R/datainputTab.R` | Split across import, catalog, workspace, selection, ratio, transformation, jobs, and add-column features; preserve every reachable observer/action described above |
| `R/transformationStore.R` | Port snapshot fields to structured definitions and ephemeral results; preserve metadata refresh/group filtering; remove in-memory nullable-list model |
| `R/transformationPersistence.R` | Replace index + six dynamic tables + delimiter encoding with versioned JSON definitions containing inputs/configuration/seeds only; never persist transformed/permuted elemental matrices in group files; legacy decoder remains in migrator |
| `R/tableNameMigration.R` | Archive behavior in legacy inventory/migrator; no runtime table-name migration |
| `R/updateCurrent.R` | Replace database autosave with hidden-UUID-addressed descriptive edits and atomic group Parquet revisions; elemental columns are never edited and calculated values are never written back |
| `R/rowidIntegrity.R` | Port invariant using hidden immutable typed `AnalyticalUuid`; legacy repairs logged in migration report |
| `R/editData.R` | Port assignment propagation as one transactional move/copy-analytical-units command across group files; invalidate ephemeral results explicitly |
| `R/groupAssignmentHelpers.R` | Port existing/new-group choice and stable checked-analytical-unit helpers to client/domain tests; HTML string checkboxes are retired |
| `R/groupSelectionUtils.R` | Port selection preservation/all/selected/unselected/invert semantics |
| `R/restoreState.R` | Replace imperative widget restoration with declarative store hydration from transformation definition |
| `R/exploreTab.R` | Port table, descriptive-field editing, crosstabs, missing plot, histogram, and compositional profile route; elemental cells and hidden UUIDs are not editable/displayed |
| `R/plot_missing.R` | Port count/% profile and band thresholds (Good ≤5%, OK ≤40%, Bad ≤80%, Remove ≤100%); UI may show band legend/tooltips |
| `R/comp.profile.R` | Port ordered long-form profile plot and optional group coloring to Plotly/client transforms |
| `R/visualizeassignTab.R` | Port responsive filters, sources, filters, axes, selection, assignments, multiplots, save dialog, and validation |
| `R/plot.R` | Port Plotly scatter/ellipse/symbol/label/hover and multiplot semantics to client; move heavy sampling/data shaping to Rust |
| `R/ordinationTab.R` | Port PCA/LDA outputs; implement UMAP view to resolve orphaned `umapheader`; remove duplicate reactive run observers |
| `R/pcaHelpers.R` | Port numeric PC sorting, variance/cumulative labels, and count limiting |
| `R/lda.R` | Port validation, LDA fit/scores, and vector plot data as specified |
| `R/cda.R` | Retire from parity release: exported but unreachable from current UI and untested. Preserve a fixture/design note in archive; future CDA requires a new feature ADR and tests |
| `R/clusterTab.R` | Port all five method families, controls, plots, tables, overwrite confirmation, and assignment recording |
| `R/GroupMembership.R` | Port group sizes, controls, source resolution, PC count, result table, check selection, and both assignment actions |
| `R/Group_probs.R` | Port eligibility, Hotelling, fallback, best-group, and robust Mahalanobis semantics |
| `R/calculate_mahalanobis_distance.R` | Port as internal tested formula utility; no new public UI/API unless a use case emerges |
| `R/EuclideanDistance.R` | Port UI/use case and optimize exact distance computation as specified |
| `R/saveexportTab.R` | Port three current result choices and explicit formats; correct PCA source to PCA scores |
| `R/subsetData.R` | Retire: server calls it, but its UI is not mounted and it depends on nonexistent `rvals$df`; filtering/subsetting remains available through workspace/group selections and can become an explicit “Save subset” feature later |
| `R/authEmail.R` | Port URL generation and SMTP/sendmail/dev-sink adapters; tokens/PII never logged; use mode-aware desktop/web links |
| `R/loginServer.R` | Replace Shiny modals/DB/session state with hosted auth routes and React dialogs; desktop has no required auth; preserve registration/verification/reset/remember/logout intent with stronger policy |
| `R/appLogging.R` | Replace option-gated messages with structured tracing and level config |
| `R/timingLogging.R` | Replace default username-bearing lines with metrics/traces and redaction |
| `R/packageChecks.R` | Retire runtime dependency probing; builds fail on missing dependencies and optional algorithms expose compile/config capability metadata |
| `R/quietly.R` | Replace warning swallowing with typed errors/warnings and user-safe problem details/toasts |
| `R/shinyDependencyFixes.R` | Retire; Shiny resource alias workaround has no equivalent need |
| `R/utils-pipe.R` | Retire with R/magrittr |
| `R/zScore.R` | Port exact compositional-percent/column-standardization semantics and rename help label |
| `R/ternary.R` | Retire commented, unreachable prototype; archive concept only, with no implication that ternary plots are shipped |
| `R/test3.R` | Retire commented local-path Naive Bayes/random-forest experiment; do not carry personal paths or unsupported classification into runtime |

### 13.3 Shiny app and static assets

| Current path/group | Disposition |
|---|---|
| `inst/app/ui.R` | Replace with React app shell/routes; preserve page title, sidebar, nav, cookie/privacy entry, themes, and controls |
| `inst/app/server.R` | Replace composition root with Rust application/API/Tauri roots; preserve request-size intent, connection failure behavior, preference hydration, logout/reset, module coverage, and connection cleanup; retire unused `profvis` import |
| `inst/app/www/app.js` | Reimplement sidebar/theme/table-menu/session behavior in React; retire Shiny retries/custom messages and JS-readable auth cookie |
| `inst/app/www/styles.css` | Translate tokens/layout/themes/modals/tables/spinner/responsive rules into design system; remove Bootstrap-specific selectors after visual regression approval |
| `inst/app/www/favicon.ico` | Keep or replace only with approved brand asset; include in desktop/web |
| `inst/app/www/help.md` | Migrate all substantive content into versioned help pages and update mode/statistics wording |
| `inst/app/www/help.html` | Retire generated artifact; generate help during build |
| `inst/app/www/help_files/bootstrap-3.3.5/**` | Retire vendored Bootstrap themes/fonts/glyphicons/JS; no runtime copy |
| `inst/app/www/help_files/jquery-3.6.0/**` | Retire vendored generated-help dependency |
| `inst/app/www/help_files/jqueryui-1.13.2/**` | Retire vendored generated-help dependency, including images/license/author files after dependency no longer ships |
| `inst/app/www/help_files/header-attrs-*`, `navigation-*`, `tocify-*` | Retire generated R Markdown support assets; replace help TOC/code behavior in client |
| `inst/app/www/privacy.md` | Preserve and revise for desktop vs hosted storage, sessions, group/source/result files, retention/deletion, subprocessors, backups, and contact after review |
| `inst/app/www/terms.md` | Preserve and revise account, backup, transformation, warranty, and mode-specific language after review |
| `inst/app/INAA_test.csv` | Move one canonical copy to `fixtures/INAA_test.csv`; preserve exact bytes/checksum as migration fixture |
| `inst/app/tests/testthat/INAA_test.csv` | Deduplicate after tests consume canonical fixture |
| `inst/app/.gitignore` | Retire/merge relevant cache ignore into root ignores |

Phase 0 spot-audits every tracked file under `inst/app/tests/testthat/` (fixtures, helper scripts, recording artifacts) against these dispositions the way the §13.2 R file list was, so the "every tracked application source, UI asset, test, package/deployment file" claim in Section 1 is audited rather than aspirational.

### 13.4 R generated manuals

All `man/*.Rd` files are generated R package documentation. Retire them as runtime/build artifacts after their contracts have been captured. The following topics must not disappear silently:

- app/UI/server topics (`runArchaeoDash`, `dataLoader*`, `datainputTab`, `explore*`, `visualize*`, `ordination*`, `cluster*`, `group*`, `euclidean*`, `saveExport*`, `subset*`, `chooseDF*`, `login*`, `homeTab`, `infoTabUI`): replaced by user help, OpenAPI, Tauri command docs, and architecture docs;
- analysis/helper topics (`calcEDistance`, `calculate_mahalanobis_distance`, `comp.profile`, `getCDA`, `getEligible`, `getBestGroup`, `getLDA`, `getMahalanobis`, `group.mem.probs`, `mainPlot`, `multiplot`, `plotLDAvectors`, `plot_missing`, `profile_missing`, `zScore`): replaced by Rust rustdoc plus statistical-spec tests; `getCDA` explicitly archived;
- state/edit/helper topics (`replaceCell`, `restoreState`, `subsetData`, `updateCurrent`, `quietly`, `mynotification`, pipe): mapped to typed domain/use-case/error docs or retired above.

This statement covers every current file under `man/`; retain the old generated set on the legacy branch for historical lookup.

### 13.5 Quarto/help build cache

Every tracked file under `quarto/.quarto/idx/**`, `quarto/.quarto/xref/**`, and `quarto/.quarto/preview/lock` is generated cache/preview state, not authored product content. Retire all of it and add cache ignores. The referenced `about`, `help`, `index`, `login`, `privacy`, and `terms` topics must be represented by authored client/help pages before deletion; no corresponding authored QMD files exist in the current repository.

### 13.6 Deployment and infrastructure files

| Current path | Disposition |
|---|---|
| `Dockerfile` | Replace with pinned multi-stage Node asset + Rust builder and minimal non-root runtime; no R, editor, Git, or dev packages; mount/configure user-file storage outside the image; generate SBOM and scan image |
| `deploy.sh` | Replace commit/tag/build/deploy coupling with CI release workflow, immutable images, PostgreSQL control-plane/project-metadata/group-profile migration gates, user-file-store compatibility/permission check, environment promotion, health gate, and rollback; releases must not mutate source on host |
| `install.sh` | Retire direct `git pull`/test/install inside running container; use immutable deployment artifact |
| `inst/runDocker.sh` | Retire hard-coded beta build/run; provide maintained Compose developer profile |
| `test_run_archaeodash.sh` | Replace with `pnpm dev`, `cargo run`, `pnpm tauri dev`, and a documented one-command dev orchestration target |
| `.github/workflows/r-tests.yml` | Replace manual-only Windows R workflow with required Rust/client/web/desktop/parity/security matrices and release jobs |
| `security/apache-hardening.conf` | Translate required headers to chosen reverse proxy/Axum; enforce tested CSP rather than report-only; keep HSTS/nosniff/referrer/permissions/frame protection intent |
| `security/live_healthcheck.sh` | Replace HTML grep with `/health/live`, `/health/ready`, and external synthetic workflow |
| `security/beta_runtime_watchdog.sh` | Retire CPU+HTML shell restart heuristic and unsafe sourced state; use orchestrator restart/limits/alerts |
| `security/install_beta_watchdog_timer.sh` | Retire host-mutating watchdog unit installer; use deployment-owned orchestration/alerts |
| `security/install_live_healthcheck_timer.sh` | Retire host-mutating health timer installer; use deployment-owned probes/synthetics, or declaratively package units if systemd is an approved target |
| `security/README.md` | Rewrite as hosted/desktop security operations runbook and validation checklist |

The external `../docker-compose.yml` referenced by current scripts is part of the migration inventory even though it is not tracked here. Capture its services, volumes, ports, proxy routes, environment injection, MySQL data, and restart behavior before cutover. The replacement development stack declares the Rust API, PostgreSQL control plane, and durable local or S3-compatible user-file store explicitly; replace external definitions with checked-in sanitized deployment configuration or document why environment-owned configuration remains external.

### 13.7 Tests: every current test file/group

| Current test path | New coverage |
|---|---|
| `tests/testthat.R`, `inst/app/tests/testthat.R`, `inst/app/tests/testthat/setup-shinytest2.R` | Replace runners/setup with Cargo, Vitest, Playwright, and Tauri driver setup |
| `helper-test-paths.R` | Replace source/installed-path logic with workspace fixture helpers |
| `test-app-logging.R` | structured level/redaction tests |
| `test-calculate_mahalanobis_distance.R` | Rust unit/golden formula tests |
| `test-checkbox-assignment-flows.R` | domain + component tests for stable checked hidden analytical UUIDs and group transfers |
| `test-column-type-hints.R` | Rust import inference unit/property tests |
| `test-crosstab-summary.R` | Rust aggregate golden/error tests + client rendering |
| `test-dataLoader.R` | in-project source selection, import normalization, ANID/default elements, blanks, group splitting/no-group fallback, profile metadata, and atomic publication |
| `test-datainput-prior-datasets.R` | recursive Parquet candidate discovery, metadata-ready badges, full validation-on-add, multi-group load, legacy transformation, timeout, and deferred-auth tests |
| `test-dataset-load-timeout-helpers.R` | job guard/deadline/cancel tests |
| `test-dataset-name-helpers.R` | user-visible naming and legacy table-name migration fixtures; no new dynamic-table assertions |
| `test-db-table-ops.R` | local/hosted `GroupFileStore`, scanner, project metadata, and PostgreSQL control-plane transaction failure injection |
| `test-default-chem-columns.R` | exact INAA default list/fallback tests |
| `test-group-assignment-helpers.R` | client/domain existing/new group, move/copy analytical unit, duplicate/fork identity, and checked-state tests |
| `test-group-selection-utils.R` | exact selection mode transition tests |
| `test-login-security.R` | tokens, hash/rehash, persistent throttles, full schema migrations, sessions, CSRF, verification/reset lifecycle |
| `test-membership-pc-selection.R` | PC sorting/variance/count golden tests |
| `test-multiplot-server.R` | multiplot job validation/success/failure/cancel and loader teardown |
| `test-package-checks.R` | Retire runtime package tests; replace with build feature/config readiness tests |
| `test-plot-mainPlot.R` | Plotly spec/component visual tests for hidden UUID identity, ANID/display labels, themes, ellipses, symbols, multiplots, axes |
| `test-shinytest2-assignment-flows.R` | Playwright web + desktop e2e membership/Euclidean assignment |
| `test-shinytest2-auth-and-load-e2e.R` | hosted Playwright registration/verification/login/import/transform flow |
| `test-shinytest2-logout-e2e.R` | HttpOnly session logout/revocation e2e; no document-cookie assertion |
| `test-shinytest2-multiplot-loader.R` | web/desktop no-X failure and progress cleanup e2e |
| `test-shinytest2-upload-append-e2e.R` | web/desktop append type mismatch e2e |
| `test-subselect-reactivity.R` | client e2e selection preservation/deselect-all |
| `test-transformation-persistence-helpers.R` | definition/seed serialization, project integrity, schema migration, legacy decoder, and proof that transformed/permuted element values never enter group files |
| `test-transformation-store.R` | definition round trip, on-demand recomputation, metadata refresh, group filtering, and ephemeral-result disposal |
| `test-updateCurrent-multi-dataset.R` | atomic multi-group Parquet publication, measured-element checksum invariance, descriptive edits, interrupted-transaction recovery/rollback |
| `test-user-preferences.R` | typed preference upsert/hydration tests |
| `test-visualize-layout-default.R` | responsive layout unit/visual tests |
| `test-zScore.R` | exact numerical Rust golden tests |
| `inst/app/tests/testthat/test-shinytest2.R` | inspect/archive any generated recording; replace assertions with explicit Playwright scenarios |

Keep the intent of skipped browser/DB tests, but make required CI environments deterministic so core e2e/security tests do not silently skip because Chrome or a database is absent.

### 13.8 Dependency disposition

| Current R dependency/group | New mechanism |
|---|---|
| Shiny, bslib, bsicons, shinyjs, DT | React/design system/TanStack Table/Tauri/Axum |
| dplyr, tidyr, tibble, tidyselect, data.table, purrr, magrittr, stringr, glue, janitor | Polars + typed Rust/TS utilities |
| rio | explicit CSV/TSV/Calamine/rust_xlsxwriter/Parquet adapters |
| ggplot2, plotly, factoextra, cowplot, dendextend | Plotly.js plus Rust model/tree/diagnostic outputs |
| cluster | Linfa/kodama/kmedoids/custom parity layers |
| MASS | Rust LDA implementation/adaptor |
| mice, randomForest, ranger | versioned `Imputer` implementations; no silent substitution |
| ICSNP | validated Hotelling T² implementation |
| candisc | no production dependency; CDA archived |
| umap | validated Rust UMAP/KNN/init implementation |
| DBI, RMySQL | SQLx hosted control-plane repository plus filesystem/S3 `UserFileStore`; MySQL only in read-only migration tool |
| sodium | Argon2id/password-hash plus cryptographic RNG and SHA-256/HMAC token digest; verify legacy hashes |
| curl | Rust email adapter (`lettre` or audited equivalent) and HTTP clients where needed |
| later, future, promises | Tokio + Rayon/bounded jobs + progress/cancellation |
| testthat, shinytest2 | Cargo tests, Vitest, Playwright, Tauri e2e |
| devtools, remotes, pak, uvr, roxygen2 | Cargo/pnpm lockfile/toolchain and rustdoc/OpenAPI |
| `profvis` (undeclared runtime dependency: `library(profvis)` in `inst/app/server.R`, absent from `DESCRIPTION`) | Live defect: remove the unused `library()` call from the current app now; do not port a profiler into the new runtime |
| `DataExplorer`, `shinydashboard`, `shinythemes`, `markdown` (not declared in `DESCRIPTION`; verify no source imports them, then drop from disposition) | not current dependencies; no mapped behavior; drop |
| base-R `stats` | not a removable dependency; its statistical semantics are carried by the Section 8 parity spec instead |
| Docker-only extras | remove unless a mapped behavior above proves a direct need |

Pin direct dependencies and toolchains in lockfiles. At implementation kickoff, verify maintained status, license, MSRV, desktop platform support, WASM/native feature effects, and security advisories; this document intentionally does not freeze 2026 version numbers forever.
