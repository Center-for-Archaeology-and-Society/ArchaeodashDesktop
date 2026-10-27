# Ordination: cancel superseded runs, delay reruns (2026-10-27)

Up: [[Index]] · [[ArchaeoDash_Analysis_MOC]]

## Why
- Changing the group selection started a new PCA/UMAP/LDA run right away, but the old run was never stopped. Pages only ignored its result, and the backend gave every ordination a cancel token nothing could trigger. Clicking several groups quickly stacked up full UMAP runs that competed for the CPU.

## What
- **Backend** (`crates/application/src/ordination.rs`): `pca_with_cancel`, `lda_with_cancel` and `umap_with_cancel` take a `CancellationToken`; the old methods delegate with a fresh token. PCA and LDA check before reading, before computing, and after waiting on another caller's cache lock. UMAP uses its own checkpoints (neighbour search, every 64 edges per layout epoch) plus new checks around each dense spectral solve in `umap.rs`. A cancelled run never writes a cache entry. `CancellationToken` is re-exported from `archaeodash-application`.
- **Desktop** (`crates/desktop/src/lib.rs`): `DesktopOrdination` keeps a registry of client cancel tokens. `ordination_*_cancellable(req, token)` registers the token, and a drop guard removes it however the run ends. `cancel_ordination(token)` cancels a running token. A cancel that arrives before its run starts is remembered for 60 s, so the run starts already cancelled. Tokens must be 1–128 characters.
- **Tauri** (`apps/desktop/src-tauri/src/lib.rs`): the `ordination_pca`, `ordination_lda` and `ordination_umap` commands take an optional `token`; a new `cancel_ordination` command.
- **Client** (`packages/client`): `OrdinationService` methods take `CallOptions { signal }`. With a signal, Tauri sends a fresh token (`newCancelToken`); abort rejects at once with an `AbortError` and sends `cancel_ordination`. Without a signal the call is unchanged. HTTP passes the signal to `fetch`; the hosted server has no per-request cancel route. `abortError` and `isAbortError` are exported.
- **Pages** (`apps/web`): `ordination/rerun.ts` holds `RERUN_DELAY_MS = 400`, `waitUnlessAborted`, and `supersedingRuns` (which replaces the removed `requestGate`).
  - Ordination page (`useOrdination`): a new run aborts the previous one, and unmount aborts too. The first run starts at once; reruns from changed inputs wait 400 ms; Recompute is immediate.
  - Visualize: the scores effect aborts on cleanup; a rerun after the rows key (group selection) changes waits 400 ms.
  - Data Manager precompute: a change aborts the running job and waits 400 ms before starting again.

## Limits
- The dense spectral solve for a connected component under 2048 rows (kept for R parity) cannot be interrupted. A cancel waits for at most one component's solve, which can take a few seconds for components near 2048 rows; the cost in release builds wasn't measured.
- The hosted web build only aborts the fetch; the server finishes the computation.

## Verification
- Rust: `check.sh rust archaeodash-analysis archaeodash-application archaeodash-desktop` (63/117/18 tests), `check.sh parity`, and `run.sh clippy-desktop` all pass.
  - New tests: UMAP cancelled mid-run stops early (`cancelling_mid_run_stops_umap_early`); desktop early-cancel, token reuse, registry cleanup and invalid tokens for UMAP/PCA/LDA.
  - The first version of the UMAP test (three 1000-row blobs) showed cancellation waiting about 77 s in a debug build on uninterruptible dense solves. That led to the extra checkpoints.
- Web: `check.sh web` passes (282 tests): rerun helpers, and precompute passes its signal to every call. Client tests (54) were run directly on home2: Tauri abort sends `cancel_ordination` with the run's token, a pre-aborted signal makes no call, and HTTP passes the signal through.
- Not tried in the running app; Windows exe not rebuilt. Committed (no push).

Related: [[Interaction_Log_2026-10-27]], [[Membership_Shared_Covariance_Parallel_2026-10-27]]
