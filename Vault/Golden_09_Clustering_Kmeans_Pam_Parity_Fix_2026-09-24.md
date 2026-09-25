# Golden 09 Clustering — kmeans/PAM Parity Fix (2026-09-24)

## Summary
Fixed three statistical-parity bugs in `crates/analysis/src/cluster.rs` so the Rust
Hartigan-Wong kmeans and PAM ports match R 4.5.2 (`stats::kmns.f`, `cluster::pam.c`
2.1.8.2) bit-for-bit on the INAA fixture (procedure 9, class T).

## Root causes
1. **RNG seeding off by one LCG step** (`RMt19937::new`): R's `set.seed` fills
   `i_seed[0..624]` (slot 0 = `mti`, overwritten to 624 by `FixupSeeds`), so
   `mt[i] = LCG^(52+i)`. The port used `LCG^(51+i)`, shifting the whole uniform
   stream by one draw — every `sample.int` nstart draw diverged from R.
2. **MT uniform scaling**: R's `MT_genrand` returns `y * 2^-32` (`d2_32`), not
   `y / (2^32 - 1)` (`i2_32m1`, used only by other RNG kinds + the 0/1 fixup).
3. **kmns.f port** (`optra`/`qtran`): Fortran `GO TO 90/60` on skip still runs the
   `INDX == M` / `ICOUN == M` termination check; Rust `continue` bypassed it, and
   `LIVE(L1) = M + I` on transfer was ported as `i + 1` instead of `n + i + 1`.
4. **PAM swap loop** (`bswap`, pamonce=0): acceptance must be
   `dzsky < -16 * DBL_EPSILON * |sky|` with `dzsky` from the `T_{i,h}` dysma/dysmb
   recurrence and `sky` accumulated per swap; the port recomputed the objective and
   accepted deltas `> -1e-12`, which oscillates forever. BUILD's first medoid must
   use `ammax = 0.0` init with last-wins `<=` over the `beter` sums seeded from
   `dysma = s` (`s = 1.1 * max_dist + 1`), not first-wins argmin of row sums.

## Outcome
`golden_09_kmeans_matches_r_hartigan_wong`, `golden_09_pam_matches_r`, and
`golden_09_silhouette_series_matches_r` pass in release mode. `golden_09_hclust`
was failing concurrently (owned by another work stream). Reference sources:
R-4-5-branch `kmns.f`/`kmeans.R`/`RNG.c` and cluster 2.1.8.2 `pam.c`.

## Related
- [[Cluster_Workflow]]
- [[Interaction_Log_2026-09-24]]
