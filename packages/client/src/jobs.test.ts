import assert from 'node:assert/strict';
import { test } from 'node:test';
import { HttpTransport } from './http.ts';
import { TauriTransport } from './tauri.ts';
import type { AnalysisJobSnapshot, SubmitAnalysisJobRequest } from '@archaeodash/contracts';
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
