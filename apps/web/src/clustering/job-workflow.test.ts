import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { AnalysisJobsService, AnalysisJobRequest, AnalysisJobSnapshot } from '@archaeodash/client';
import { runAnalysisJob } from './job-workflow.ts';
const request: AnalysisJobRequest = { kind: 'cluster_diagnostics', request: { path: 'group.parquet', columns: ['x'], max_k: 2, seed: 1 } };
const queued: AnalysisJobSnapshot = { id: 'job', state: 'queued', stage: null, progress: 0, submitted_at_ms: 1, started_at_ms: null, completed_at_ms: null, result: null, error: null };
const succeeded: AnalysisJobSnapshot = { ...queued, state: 'succeeded', progress: 100, result: { kind: 'cluster_diagnostics', result: { path: 'group.parquet', revision_id: 'r', column_names: ['x'], n_rows: 3, wss: [2, 1], silhouette: [0.5] } } };

test('job runs once and reports lifecycle before yielding typed result', async () => {
  let submissions = 0;
  const states: string[] = [];
  const jobs: AnalysisJobsService = { submit: async () => { submissions++; return queued; }, get: async () => succeeded, cancel: async () => { throw new Error('unexpected cancel'); } };
  const result = await runAnalysisJob(jobs, request, { signal: new AbortController().signal, pollMs: 0, onProgress: j => states.push(j.state) });
  assert.equal(submissions, 1); assert.deepEqual(states, ['queued', 'succeeded']); assert.deepEqual(result, succeeded.result);
});

test('abort during submit cancels once and waits for worker acknowledgement', async () => {
  const controller = new AbortController(); let cancelled = 0; let polls = 0;
  const jobs: AnalysisJobsService = { submit: async () => { controller.abort(); return queued; }, cancel: async () => { cancelled++; return { ...queued, state: 'running' }; }, get: async () => { polls++; return { ...queued, state: 'cancelled' }; } };
  await assert.rejects(runAnalysisJob(jobs, request, { signal: controller.signal, pollMs: 0, onProgress: () => {} }), { name: 'AbortError' });
  assert.equal(cancelled, 1); assert.equal(polls, 1);
});

test('timeout surfaces error and never retries submission', async () => {
  let submissions = 0;
  const jobs: AnalysisJobsService = { submit: async () => { submissions++; return { ...queued, state: 'timed_out', error: { code: 'deadline', message: 'Deadline exceeded' } }; }, get: async () => queued, cancel: async () => queued };
  await assert.rejects(runAnalysisJob(jobs, request, { signal: new AbortController().signal, onProgress: () => {} }), /Deadline exceeded/);
  assert.equal(submissions, 1);
});

test('poll failure cancels orphaned computation and preserves the original error', async () => {
  let cancels = 0;
  const jobs: AnalysisJobsService = { submit: async () => queued, get: async () => { throw new Error('connection lost'); }, cancel: async () => { cancels++; return queued; } };
  await assert.rejects(runAnalysisJob(jobs, request, { signal: new AbortController().signal, pollMs: 0, onProgress: () => {} }), /connection lost/);
  assert.equal(cancels, 1);
});

test('progress subscription is disposed on success and unavailable events do not block polling', async () => {
  let disposed = 0;
  const jobs: AnalysisJobsService = {
    submit: async () => queued, get: async () => succeeded, cancel: async () => queued,
    subscribe: (_id, report) => { report({ id: 'job', state: 'running', stage: 'computing', progress: 15, updated_at_ms: 2, error: null }); return () => { disposed++; }; },
  };
  await runAnalysisJob(jobs, request, { signal: new AbortController().signal, pollMs: 0, onProgress: () => {} });
  assert.equal(disposed, 1);
  jobs.subscribe = () => { throw new Error('events unavailable'); };
  assert.deepEqual(await runAnalysisJob(jobs, request, { signal: new AbortController().signal, pollMs: 0, onProgress: () => {} }), succeeded.result);
});
