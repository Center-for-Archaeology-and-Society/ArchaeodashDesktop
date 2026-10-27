/**
 * Re-running ordinations (PCA, UMAP, LDA) when their inputs change: a newer
 * run stops the older one in the backend straight away, and a rerun waits a
 * moment so a burst of group checks and unchecks starts one run, not one per
 * click.
 */

/** Wait before re-running after inputs change (group selection, setup). */
export const RERUN_DELAY_MS = 400;

/** Resolves `true` after `ms`, or `false` as soon as `signal` aborts. */
export function waitUnlessAborted(ms: number, signal: AbortSignal): Promise<boolean> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve(false);
      return;
    }
    const onAbort = () => {
      clearTimeout(timer);
      resolve(false);
    };
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve(true);
    }, ms);
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

/**
 * One run at a time: `next()` aborts the previous run (which cancels its
 * backend work) and returns the new run's signal; `stop()` aborts the
 * current run, e.g. when the page unmounts.
 */
export function supersedingRuns(): { next: () => AbortSignal; stop: () => void } {
  let current: AbortController | null = null;
  return {
    next() {
      current?.abort();
      current = new AbortController();
      return current.signal;
    },
    stop() {
      current?.abort();
      current = null;
    },
  };
}
