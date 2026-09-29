# Phase 6 numerical validation and performance — 2026-09-28

## Scope and decision

This checkpoint re-runs the Section 15.4 procedures 9–11 and 13 that are
represented in the current tree, records a reproducible release benchmark, and
checks the new cooperative cancellation hooks. It is evidence for these
procedures and the supported 1,000-row service limit; it is not a Phase 6 exit
sign-off. Browser/desktop end-to-end acceptance and cross-platform performance
acceptance remain open.

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
| 13, interactive multiplot sampling (class E) | The artifact and manifest record 100,000 points, 180,000 candidates, 12 facets, 5 groups, and 99,960 selected points. `multiplot-model.test.ts` checks all rows at the ceiling and deterministic stride-from-zero sampling at 250,000; component tests check the status label. | Unit/model behavior passes. The recorded artifact is not directly consumed by a Rust golden test, and browser rendering at 100,000 points is not measured here. |

`cargo test -p archaeodash-parity` passed all 26 parity/support tests: the
registered import, PCA, UMAP, LDA, clustering, membership, Euclidean, Explore,
export, transformation, and group-profile test files. Procedure 13 is in the
web tests, not that Rust suite. The full web suite passed 87 tests.
`cargo test -p archaeodash-analysis` passed 39 unit tests at the prior
validation checkpoint; the new R matrix adds one parity test.

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

## Remaining gates

- This re-run covers the currently registered Section 15.4 procedure 9–11
  goldens and the available procedure 13 model/component tests. It does not run
  the entire 14-procedure suite through one production-like end-to-end path.
- No browser/desktop end-to-end run or 100,000-point render performance test was
  available in this audit. Cross-platform timing and memory budgets remain
  unmeasured.
- Service limits are fixed at 1,000 rows for pairwise clustering workloads;
  the wider backend probe does not change or validate that service policy.
