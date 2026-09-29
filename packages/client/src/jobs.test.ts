import assert from 'node:assert/strict';
import { test } from 'node:test';
import { HttpTransport } from './http.ts';
import { TauriTransport } from './tauri.ts';
import type { AnalysisJobEvent, AnalysisJobSnapshot, SubmitAnalysisJobRequest } from '@archaeodash/contracts';
import type { EventSourceLike } from './http.ts';
const request: SubmitAnalysisJobRequest = { analysis: { kind: 'cluster_fit', request: { path: 'group.parquet', columns: ['a'], method: 'hclust', source: 'pca', pc_count: 1, metric: 'manhattan', linkage: 'average' } } };
const job: AnalysisJobSnapshot = { id: 'uuid', state: 'queued', stage: null, progress: 0, submitted_at_ms: 1, started_at_ms: null, completed_at_ms: null, result: null, error: null };

test('HTTP job submit/status/cancel preserve typed requests and encode job IDs', async () => {
  const calls: { url: string; method: string; body: unknown }[] = [];
  const transport = new HttpTransport('', async (url, init) => {
    calls.push({ url: String(url), method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : null });
    return new Response(JSON.stringify(job), { status: 200, headers: { 'content-type': 'application/json' } });
  });
  assert.deepEqual(await transport.jobs.submit(request), job);
  await transport.jobs.get('id/other'); await transport.jobs.cancel('id/other');
  assert.deepEqual(calls, [
    { url: '/api/v1/jobs', method: 'POST', body: request },
    { url: '/api/v1/jobs/id%2Fother', method: 'GET', body: null },
    { url: '/api/v1/jobs/id%2Fother/cancel', method: 'POST', body: null },
  ]);
});

test('Tauri jobs use equivalent command payloads', async () => {
  const calls: unknown[] = [];
  const transport = new TauriTransport(async <T>(command: string, args?: Record<string, unknown>) => { calls.push([command, args]); return job as T; });
  assert.deepEqual(await transport.jobs.submit(request), job);
  await transport.jobs.get('uuid'); await transport.jobs.cancel('uuid');
  assert.deepEqual(calls, [['submit_analysis_job', { request }], ['get_analysis_job', { id: 'uuid' }], ['cancel_analysis_job', { id: 'uuid' }]]);
});

test('Tauri project commands return selected project or cancellation without sending a path', async () => {
  const calls: unknown[] = [];
  const project = { path: '/project', name: 'project', generation: 2 };
  const transport = new TauriTransport(async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push([command, args]);
    return (command === 'open_project' ? project : null) as T;
  });
  assert.deepEqual(await transport.projects.open(), project);
  assert.equal(await transport.projects.current(), null);
  assert.deepEqual(calls, [['open_project', undefined], ['current_project', undefined]]);
});

const event: AnalysisJobEvent = {
  id: 'uuid', state: 'running', stage: 'computing', progress: 25,
  updated_at_ms: 123, error: null,
};

test('HTTP job subscription parses progress and closes EventSource on disposal or error', () => {
  let url = '';
  let progress: ((event: { data: string }) => void) | undefined;
  let removed = false;
  let closed = 0;
  const source: EventSourceLike = {
    addEventListener: (type, listener) => { assert.equal(type, 'progress'); progress = listener; },
    removeEventListener: (_type, listener) => { removed = listener === progress; },
    close: () => { closed += 1; },
    onerror: null,
  };
  const transport = new HttpTransport('/base', undefined, (endpoint) => { url = endpoint; return source; });
  const received: AnalysisJobEvent[] = [];
  const dispose = transport.jobs.subscribe!('id/part', (update) => received.push(update));
  assert.equal(url, '/base/api/v1/jobs/id%2Fpart/events');
  const matching = { ...event, id: 'id/part' };
  progress?.({ data: JSON.stringify(matching) });
  progress?.({ data: JSON.stringify({ ...event, id: 'another' }) });
  assert.deepEqual(received, [matching]);
  source.onerror?.({}); // A stream error closes once instead of letting EventSource retry forever.
  dispose();
  dispose();
  assert.equal(removed, true);
  assert.equal(source.onerror, null);
  assert.equal(closed, 1);
});

test('Tauri subscription listens before starting watcher and stops both on cleanup', async () => {
  const calls: string[] = [];
  let handler: ((event: { payload: unknown }) => void) | undefined;
  const order: string[] = [];
  const transport = new TauriTransport(
    async <T>(command: string) => { calls.push(command); return undefined as T; },
    async (name, callback) => { order.push(`listen:${name}`); handler = callback; return () => order.push('unlisten'); },
  );
  const received: AnalysisJobEvent[] = [];
  const dispose = transport.jobs.subscribe!('uuid', (update) => received.push(update));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(order, ['listen:analysis-job-progress']);
  assert.deepEqual(calls, ['watch_analysis_job']);
  handler?.({ payload: event });
  handler?.({ payload: { ...event, id: 'other' } });
  assert.deepEqual(received, [event]);
  dispose();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(order, ['listen:analysis-job-progress', 'unlisten']);
  assert.deepEqual(calls, ['watch_analysis_job', 'stop_analysis_job_watch']);
});

test('Tauri disposal before async listen resolves unregisters without starting native watcher', async () => {
  let resolveListen!: (unlisten: () => void) => void;
  let unlistened = 0;
  const calls: string[] = [];
  const transport = new TauriTransport(
    async <T>(command: string) => { calls.push(command); return undefined as T; },
    () => new Promise((resolve) => { resolveListen = resolve; }),
  );
  const dispose = transport.jobs.subscribe!('uuid', () => {});
  dispose();
  resolveListen(() => { unlistened += 1; });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(unlistened, 1);
  assert.deepEqual(calls, []);
});

test('Tauri disposal during watcher startup stops watcher and unlistens exactly once', async () => {
  let resolveWatch!: () => void;
  let unlistened = 0;
  const calls: string[] = [];
  const transport = new TauriTransport(
    async <T>(command: string) => {
      calls.push(command);
      if (command === 'watch_analysis_job') return new Promise<T>((resolve) => { resolveWatch = () => resolve(undefined as T); });
      return undefined as T;
    },
    async () => () => { unlistened += 1; },
  );
  const dispose = transport.jobs.subscribe!('uuid', () => {});
  await new Promise((resolve) => setTimeout(resolve, 0)); // Listener is installed and watch invoke is pending.
  assert.deepEqual(calls, ['watch_analysis_job']);
  dispose();
  resolveWatch();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(unlistened, 1);
  assert.deepEqual(calls, ['watch_analysis_job', 'stop_analysis_job_watch']);
});
