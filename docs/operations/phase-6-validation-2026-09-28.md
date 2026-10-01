# Phase 6 numerical validation and performance — 2026-09-28

## Scope and decision

This checkpoint re-runs the Section 15.4 procedures 9–11 and 13 that are
represented in the current tree, records a reproducible release benchmark, and
checks the new cooperative cancellation hooks. It is evidence for these
procedures and the supported 1,000-row service limit; it is not a Phase 6 exit
sign-off. The web smoke path passes; Linux native assignment/reopen now passes; acceptance on other desktop platforms and cross-platform performance remains open.

## Reproduction environment

- Linux x86_64, Ubuntu kernel 7.0.0-34-generic; Intel Core i9-13900, 32 online
  logical CPUs.
- Rust `1.98.1` (`48a229cea`, LLVM 22.1.8); Node `v22.22.1`; R `4.5.2`.
- The canonical fixture has 307 data rows plus its header. Its SHA-256 is
  `05b6aa1d7aae45a467f517c81cfee66219c4d8ee578b28ce9a3cfd08de51d4b7`.
- The recorded goldens match their manifest hashes: procedure 9
  `3d8254b2cce8a867922b22a21e849c26ffbfbc29b56c1386f3c69b7b19b9f975`,
  procedure 10
  `2c2bcdef72dd152698b1c25567230d5d4ae786eccf5168cbcad6cc2eb5fafde6`,
  procedure 11
  `8b336a0871dc242b37e16c3fd4c1858dcbf2a3199b581ef8591f02fa937cf91a`, and
  procedure 13
  `d85a0741dd207d6057b34f91d4605f3bf7ea92a163f6d8a172270e8ed4b481a3`.

Run the Rust numerical and R-golden suites with:

```sh
cargo test -p archaeodash-analysis
cargo test -p archaeodash-parity
cargo run --release -p archaeodash-analysis --example phase6_bench
```

The local `pnpm` shim points to a missing Corepack file. This direct invocation
uses the installed package manager and runs the full web suite, including the
procedure 13 deterministic sampling tests:

```sh
node /home/rjbischo/.cache/node/corepack/pnpm/12.4.1/bin/pnpm.mjs --filter @archaeodash/web test
```

## Procedure evidence

| Procedure | Current evidence | Result and limit |
|---|---|---|
| 9, clustering (class T) | `golden_09_clustering.rs` checks k-means, PAM, Ward.D2, DIANA, WSS, and silhouette. `golden_09_metric_linkage_matrix.rs` checks all 16 metric/linkage pairs on tied and untied six-row matrices, including exact merge orientation/order and tolerant heights; PAM and DIANA Euclidean/Manhattan also run on both matrices. | Both R-captured datasets match with exact topology/ordering and heights within `2e-12 * (1 + |R|)`. Reproduce using `Rscript scripts/capture_phase6_metric_matrix.R`; oracle R 4.5.2 / cluster 2.1.8.2. Fixture SHA-256: `3bc9dc3ac230a1d2a6b4dd7fb5d34d67c4c532557917898df6e0b5cf41c9414f`. |
| 10, membership probabilities (class E/T) | `golden_10_membership.rs` checks eligibility, Hotelling T² and Mahalanobis tables against the R golden, including assignment labels and serialized numeric tolerances. | Passes for the recorded INAA/CORE/first-four-chemistry-column baseline. This is one fixed baseline, not a broad distributional study. |
| 11, Euclidean nearest matches (class E) | `golden_11_euclidean.rs` checks ordered identities, groups, and serialized distances against the R golden. | Passes for the recorded INAA baseline, top five, excluding same-group matches. Additional tied/duplicate-ID and alternate projection cases are covered by analysis/application tests, not this single R golden. |
| 13, interactive multiplot sampling (class E) | `multiplot-model.test.ts` reads `fixtures/golden/13_multiplot_interactive_sampling.json`, reconstructs source-row order by group from `fixtures/INAA_test.csv`, and compares every group/facet selected row, selected count, and per-group cap. | Direct R fixture parity passes: 180,000 candidates, 12 facets, 5 groups, cap 1,666, and 99,960 selected. Sampling follows per-group/facet first-row `slice_head` order. The opt-in browser benchmark now verifies all 99,960 points across 12 rendered Plotly panels. |

`cargo test -p archaeodash-parity` passed all 26 parity/support tests: the
registered import, PCA, UMAP, LDA, clustering, membership, Euclidean, Explore,
export, transformation, and group-profile test files. Procedure 13 is in the
web tests, not that Rust suite. Final workspace verification passed 92 web tests and 38 client tests.
`cargo test --workspace` passed, including 39 analysis, 66 application, 22 API, 15 desktop adapter, 12 data-io, 11 contract, 26 parity/support tests, and the native project-state test. Workspace typechecks/build, Rust formatting, and Clippy on affected library crates including the Tauri app passed. The build retains its Plotly chunk-size warning.

## Release benchmark

The new `crates/analysis/examples/phase6_bench.rs` generates deterministic
synthetic 8-column clustering matrices and a 4-column membership/distance
matrix. On the environment above, one release run produced:

| Rows | k-means k=5, 10 starts | PAM Manhattan k=5 | Ward.D2 HCA | Maximum/Complete HCA | DIANA Manhattan |
|---:|---:|---:|---:|---:|---:|
| 100 | <0.001 s | 0.001 s | <0.001 s | <0.001 s | 0.001 s |
| 500 | 0.003 s | 0.030 s | 0.002 s | 0.002 s | 0.075 s |
| 1,000 | 0.005 s | 0.088 s | 0.009 s | 0.009 s | 0.629 s |

At 1,000 rows and four columns, Mahalanobis membership took 0.082 s and
Euclidean top-five took 0.059 s. A pure-analysis 1,500-row, eight-column
Minkowski p=3/Average HCA probe took 0.109 s. The 1,500-row call exceeds the
current application pairwise-cell cap and is only a backend scaling probe.
These timings are local observations, not portable budgets; the harness does
not measure peak memory, disk parsing, IPC, rendering, or the end-to-end app.

Cancellation was requested after 5 ms while the 1,000-row jobs were running.
Observed time from request until each worker returned a cancellation error was
0.198 ms for k-means, 0.392 ms for PAM, 0.039 ms for HCA, 0.050 ms for DIANA,
0.290 ms for membership, and 0.500 ms for Euclidean matching. These are one-run
measurements, subject to scheduler effects. DIANA has checkpoints in its nested
split scans after this probe exposed a roughly 463 ms delay when only its outer
loop checked the token.

## Real browser verification

A headless Chromium run against the production Vite build and disposable local
Rust API project passed 11 cases in 3.375 seconds: PCA HCA Manhattan/Average,
ratio transformation and PCA component bounds, cut/expanded dendrogram, PAM
Manhattan diagnostics, UMAP cancellation, immutable source rows, hidden UUIDs,
LDA membership with projection groups, UMAP nearest matches, partition color
modes, and reviewed two-group recording with exact original row equality.
The partition SVG screenshot was visually inspected. This is a small synthetic
smoke dataset, not a load test or the complete INAA acceptance workflow.

Reproduce from a dependency-installed checkout (separate terminals for the API
and preview; the script writes to the disposable project):

```sh
pnpm exec playwright install chromium
cargo run -p archaeodash-api --example local -- /tmp/archaeodash-phase6-smoke
pnpm --filter @archaeodash/web build
pnpm --filter @archaeodash/web exec vite preview --host 127.0.0.1 --port 4173
PHASE6_BASE_URL=http://127.0.0.1:4173 pnpm test:e2e:phase6
```

The API binds loopback port 8787; preview proxies API requests there. Stop both
processes after the run. A later [desktop test-drive checkpoint](desktop-test-drive.md) exercised the actual native folder picker, import, clustering, cancellation, CSV save dialog, and reopen workflow. Linux native assignment was subsequently verified as described below; other-platform and performance acceptance remain open.

### Procedure 13 browser render benchmark

The sampling plan uses the 12 actual ordered pairs from four elemental columns
as its facet budget. A component regression locks the procedure 13 scenario:
15,000 rows, five groups, 12 facets, 180,000 candidate facet-points and
99,960 selected facet-points (1,666 rows per group/facet).

An opt-in Playwright benchmark generates and imports that 15,000-row fixture
before starting the browser timer, opens it in Visualize, switches to
multiplot, switches to interactive mode, and waits for `Plotly.react` completion
in all 12 panels. Timing begins before multiplot is selected, so it includes
the initial static SVG mount and the switch to Plotly; dataset import, merge,
navigation, and initial data loading are excluded.
It checks each panel has 8,330 points in Plotly's actual trace arrays, checks
the 99,960 aggregate, and fails on page errors. The result reports elapsed
browser render time and browser/platform/Node versions; it does not set a
portable pass/fail timing threshold.

With dependencies installed and the API/Vite processes running as above,
build the app and start preview:

```sh
pnpm --filter @archaeodash/web build
pnpm --filter @archaeodash/web exec vite preview --host 127.0.0.1 --port 4173
```

In another terminal, start the local API on its default loopback port, then
run the separate benchmark after installing Chromium once:

```sh
project_dir="$(mktemp -d /tmp/archaeodash-multiplot-100k.XXXXXX)"
printf 'Disposable project: %s\n' "$project_dir"
cargo run -p archaeodash-api --example local -- "$project_dir"
pnpm exec playwright install chromium
pnpm bench:e2e:multiplot-100k
```

The API project directory is disposable; stop the API and remove the directory printed by `mktemp` after the benchmark. Set `MULTIPLOT_BASE_URL`, `PLAYWRIGHT_MODULE`, or `BROWSER_EXECUTABLE` when using nondefault local endpoints or browser installations. This benchmark is
opt-in because it intentionally renders almost 100,000 points in a real
browser and currently records measurements without an acceptance threshold.

Two local runs on Linux x64 / Node v22.22.1 / headless Chromium 151.0.7922.34
completed in 3,175 ms and 3,045 ms from selecting multiplot until all 12 panels
reported Plotly completion. All panels had 8,330 trace points (99,960 total),
and both runs reported no page errors. These are single-machine observations,
not a portable threshold.

## Remaining gates

- This re-run covers the currently registered Section 15.4 procedure 9–11
  goldens and the available procedure 13 model/component tests. It does not run
  the entire 14-procedure suite through one production-like end-to-end path.
- Native assignment/reopen passes on Linux (see the later checkpoint below); acceptance on other desktop platforms and cross-platform timing/peak-memory remain open. The initial native smoke workflow passed in the linked test-drive checkpoint. Procedure 13 has two local browser measurements above, with no portable timing or peak-memory threshold.
- Filesystem reads, multi-group planning snapshots, project candidate scans, and application import publication now coordinate with store transactions through `.archaeodash/project.lock`. A child-process regression confirms reads wait behind a store-held exclusive lock on Linux. This protects cooperating application/store users; uncoordinated external-writer acceptance remains unresolved, and footer resource preflight does not remove races with a program that ignores the lock.
- Service limits are fixed at 1,000 rows for pairwise clustering workloads;
  the wider backend probe does not change or validate that service policy.

## Repeatable CI evidence (2026-09-30)

The main CI workflow now runs the numerical analysis and golden-parity crates
on Ubuntu, macOS, and Windows. The separate `Phase 6 browser E2E` workflow
builds the API harness and production web app, starts a disposable project,
waits for readiness, and runs the browser acceptance flow on Linux. Its manual
workflow input also enables the 99,960-point multiplot benchmark. Logs are
retained as workflow artifacts, including on failure.

For numerical timing and process peak memory, run:

```sh
python3 scripts/benchmark-phase6.py --output /tmp/phase6-benchmark.json
```

The runner builds the release example before timing it. JSON retains environment,
build status, benchmark output, elapsed time, exit status, and process peak RSS
where available. This measures the complete numerical benchmark process, not
individual algorithms, the native app, file loading, IPC, or browser memory.
Peak RSS can include the forked Python launcher before exec. Cancellation text
is retained verbatim from the example; its error flag is not an independent
assertion of the cancellation error code.
Windows peak memory is explicitly unavailable; no value is inferred from another
platform. The `Phase 6 numerical performance evidence` workflow is manually
triggered and captures a separate artifact on all three desktop CI platforms.

These workflows are infrastructure for evidence collection. Hosted runs on
`4a85d02` are green across all jobs: numerical parity and lock regressions on
Ubuntu/macOS/Windows, Rust fmt/clippy/full workspace tests (with web assets
built first and runner disk freed), TypeScript build/tests, and the browser
E2E flow. A `Desktop workspace tests` matrix now compiles and tests the full
workspace, including the Tauri desktop shell, on macOS and Windows; it exposed
and fixed a macOS canonical-path comparison in the import scan test and a
missing Windows `icon.ico` resource. Timing and memory thresholds have not
been accepted. The native WebView assignment gate is separate; the later Linux
checkpoint below verifies it on this host.

Local integration verification on 2026-09-30 passed the API example build,
production web build, and browser acceptance flow (3,360 ms). The third local
multiplot run rendered all 99,960 points in 3,016 ms. The numerical capture
completed in 1.181 seconds with process peak RSS of 19,386,368 bytes on Linux;
these are observations, not accepted performance budgets. Process success,
nonzero-exit, and failed-launch capture checks also passed.

## Remaining-gate work (2026-09-30)

The application fallback fixture now genuinely requests Hotelling with a missing
measurement and checks effective Mahalanobis method, fallback reason, best-group
selection, and projected-group metadata against an explicit Mahalanobis result.
A twelfth real browser case verifies that fallback notice and distance labels,
while checking unchanged source rows and hidden UUIDs. It passed with the full
browser flow in 3,608 ms. The Rust workspace passed 230 tests; 95 web tests,
client tests, typechecks, formatting, and affected-library Clippy passed before
the subsequent native plot repair.

The cancellation benchmark now recognizes only `analysis_cancelled`; successful
work finishing before cancellation is labelled separately, and unrelated errors
fail the benchmark. This supersedes the earlier caveat about its generic error
flag.

Both performance runners now support explicit caller-supplied budgets:

```sh
python3 scripts/benchmark-phase6.py --output /tmp/phase6-budget.json \
  --max-elapsed-seconds "$PHASE6_MAX_SECONDS" \
  --max-peak-rss-bytes "$PHASE6_MAX_RSS_BYTES"
node scripts/e2e/multiplot-100k.mjs --output /tmp/multiplot-budget.json \
  --max-elapsed-ms "$MULTIPLOT_MAX_MS"
```

Supply positive limits appropriate to the target environment; omitting the
options records `not_configured`, not acceptance. Missing requested memory
measurements fail the budget check. Workflow-dispatch inputs expose the same
options, and browser artifacts now include JSON. Deliberately tiny numerical
and browser limits were tested: both exited nonzero and retained failed checks.
The browser's normal capture still verified all 99,960 points. No default
portable time or memory threshold has been ratified. Windows peak memory remains
unavailable because in-flight samples cannot establish the complete-process peak.

### Native assignment and final local checks

The actual Linux GTK/WebKit Tauri window passed a real lasso-and-move from D1
to D2, then normal close/relaunch and native project reopening. The source had
104→103 rows, destination 50→51, and the selected unit retained its hidden
identity and all 33 measured values. Full Parquet row comparisons verified every
other row unchanged. Reopened Explore tables confirmed absence in the source
and exactly one occurrence in the destination. The [test-drive guide](desktop-test-drive.md)
contains the screenshot and replay steps.

The run exposed a collapsed WebKit scatter holder that covered assignment
controls; explicit height now reserves the canvas space. Awaited Plotly rendering,
current callback references, and visible render errors improve the selection
lifecycle. A separate Chromium pointer-gesture assignment regression now checks
layout, exact row preservation, hidden UUIDs, and reload through browser CI.
After these changes, all 97 web tests and web typechecking passed, as did 66
analysis/parity tests with all targets (including the cancellation example test).
The production web and native binary builds passed. Remaining platform and
performance acceptance is listed in `IMPLEMENTATION.md`; Linux evidence does
not establish Windows/macOS behavior or a portable performance budget.
