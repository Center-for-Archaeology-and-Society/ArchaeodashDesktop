import type { AnalysisJobsService, AnalysisJobRequest, AnalysisJobResult, AnalysisJobSnapshot } from '@archaeodash/client';

function delay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise(resolve => {
    if (signal.aborted) { resolve(); return; }
    const done = () => { clearTimeout(timer); signal.removeEventListener('abort', done); resolve(); };
    const timer = setTimeout(done, ms);
    signal.addEventListener('abort', done, { once: true });
  });
}

/** Submit exactly once. Cancellation also covers an abort while submission is in flight. */
export async function runAnalysisJob(
  jobs: AnalysisJobsService,
  analysis: AnalysisJobRequest,
  options: { signal: AbortSignal; onProgress: (job: AnalysisJobSnapshot) => void; pollMs?: number },
): Promise<AnalysisJobResult> {
  if (options.signal.aborted) throw new DOMException('Analysis cancelled', 'AbortError');
  let job = await jobs.submit({ analysis });
  let cancellationSent = false;
  let finished = false;
  let unsubscribe: (() => void) | undefined;
  if (jobs.subscribe && !options.signal.aborted) {
    try { unsubscribe = jobs.subscribe(job.id, event => {
      if (!finished && event.id === job.id) options.onProgress({ ...job, state: event.state, stage: event.stage, progress: event.progress, error: event.error });
    }); } catch { /* Polling remains authoritative if events are unavailable. */ }
  }
  try {
    for (;;) {
      if (options.signal.aborted && !cancellationSent) {
        cancellationSent = true;
        job = await jobs.cancel(job.id);
      }
      options.onProgress(job);
      if (job.state === 'succeeded') {
        if (options.signal.aborted) throw new DOMException('Analysis cancelled', 'AbortError');
        if (!job.result || job.result.kind !== analysis.kind) throw new Error('Analysis returned an unexpected result');
        return job.result;
      }
      if (job.state === 'cancelled') throw new DOMException('Analysis cancelled', 'AbortError');
      if (job.state === 'failed' || job.state === 'timed_out') throw new Error(job.error?.message ?? `Analysis ${job.state}`);
      // After cancellation keep polling until the worker acknowledges a terminal state.
      if (options.signal.aborted) await new Promise(resolve => setTimeout(resolve, options.pollMs ?? 250));
      else await delay(options.pollMs ?? 250, options.signal);
      job = await jobs.get(job.id);
    }
  } catch (error) {
    if (!cancellationSent && (job.state === 'queued' || job.state === 'running')) {
      try { await jobs.cancel(job.id); } catch { /* Preserve the original transport failure. */ }
    }
    throw error;
  } finally {
    finished = true;
    unsubscribe?.();
  }
}
