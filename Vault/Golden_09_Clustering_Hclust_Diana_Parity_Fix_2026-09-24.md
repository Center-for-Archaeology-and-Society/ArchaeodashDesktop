# Golden 09 Clustering — hclust/DIANA Parity Fix (2026-09-24)

## Summary
Fixed the remaining `golden_09_hclust_matches_r` and `golden_09_diana_matches_r`
failures in `crates/analysis/src/cluster.rs` so the Rust ports match
R 4.5.2 (`stats::hclust` ward.D2 → `hclust.f` + `hcass2.f`; `cluster::diana`
→ `twins.c` splyt + R-level merge post-processing, cluster 2.1.8.2) on the
INAA fixture. Verified against Rscript oracles on 8 synthetic probes
(n=2, 3, 8, 12, 17, 40, plus a tie-heavy matrix) for merge/height/order.

## Root causes
1. **hclust dissimilarity layout mismatch**: the squared-Euclidean fill loop
   enumerated R `dist()` order only for n<4. `dist()`/`IOFFST` store the
   row-major upper triangle — `d(I,J)` at `J + (I-1)*N - I*(I+1)/2` for 1-based
   `I<J` — while the fill wrote lower-row-major order `(1,2),(1,3),(2,3),(1,4)…`,
   so every `ioffst()` lookup for n≥4 read the wrong entry (first merge -4 vs
   golden -43). Fix: fill `for i in 1..n { for j in (i+1)..=n }`.
2. **hcass2 recoding read modified arrays**: Fortran computes
   `K=MIN(IA(I),IB(I))` and tests `IA(J).EQ.K` / `IB(J).EQ.K` against the
   *original, never-modified* agglomeration arrays, writing stage refs into the
   `IIA/IIB` copies. The Rust port read `iia[i].min(iib[i])` and tested
   `iia[j] == k`, so entries already replaced by `-stage` poisoned later K
   values and match tests (m12 stage 6: mine [-3,2] vs R [-3,4]).
   Fix: test/compute from `ia`/`ib`, mutate only `iia`/`iib`.
3. **hcass2 order loop usize underflow**: `for i in (0..=(n-3)).rev()` panics
   for n=2 (Fortran `DO I=N-2,1,-1` is a no-op). Fix: `(0..n-2).rev()`.
4. **DIANA distance layout**: `CompactDist::at` used the hclust `IOFFST`
   formula, but `dysta.c`/`ind_2` fill and index the lower column-major
   triangle — `d(l,j)` at `(j-1)(j-2)/2 + l` (entry 0 unused, permanently 0).
   Fix: `at()` now uses the `ind_2` formula (matches the fill loop).
5. **DIANA merge reconstruction**: twins.c marks consumed positions
   `kwan[nj] = -1` and scans `kwan[j] >= 0`; the Rust port marked
   `usize::MAX` but tested `> 0` (always true → re-selected the same split
   every stage), and sorted each merge row `[min,max]` although twins.c emits
   `(l1, l2)` unresolved-in-place (R's `diana()` applies no reordering;
   golden row 1 is `[-43, -49]`, i.e. deliberately unsorted).
   Fix: `kw: Vec<i64>`, condition `>= 0`, mark `-1`, push `[l1, l2]` as-is.

## Outcome
All five `golden_09_clustering` tests pass in release mode
(`cargo test --release -p archaeodash-parity --test golden_09_clustering`).
Reference sources: R-4-5-branch `hclust.f` (contains `HCASS2`), `hclust.R`,
`hclust-utils.c`; cran/cluster 2.1.8.2 `twins.c`, `dysta.c`, `ind_2.h`,
`R/diana.R` (merge matrix passed through from C unchanged).

## Related
- [[Golden_09_Clustering_Kmeans_Pam_Parity_Fix_2026-09-24]]
- [[Cluster_Workflow]]
- [[Interaction_Log_2026-09-24]]
