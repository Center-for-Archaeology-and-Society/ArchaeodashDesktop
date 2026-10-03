import assert from 'node:assert/strict';
import { test } from 'node:test';
import { HttpTransport } from './http.ts';
import { TransportError, type Transport } from './transport.ts';
import {
  registerTransportContractTests,
  type Harness,
  type RecordedCall,
} from './contract-suite.ts';

function makeHarness(): Harness {
  const calls: RecordedCall[] = [];
  let next: (() => Response) | null = null;
  const transport: Transport = new HttpTransport('', async (input, init) => {
    const url = new URL(String(input), 'http://test.local');
    let payload: unknown;
    if (typeof init?.body === 'string') {
      payload = JSON.parse(init.body);
    } else if (init?.body instanceof Uint8Array) {
      payload = null; // raw bytes; shape asserted in adapter-specific tests
    }
    calls.push({ label: `${init?.method ?? 'GET'} ${url.pathname}${url.search}`, payload });
    if (next === null) throw new Error('no queued response');
    const respond = next;
    next = null;
    return respond();
  });
  const json = (status: number, body: unknown): (() => Response) => {
    const res =
      status === 204
        ? new Response(null, { status })
        : new Response(JSON.stringify(body), {
            status,
            headers: { 'content-type': 'application/json' },
          });
    return () => res;
  };
  return {
    transport,
    calls,
    respond(result) {
      next = json(result === undefined ? 204 : 200, result);
    },
    fail(err) {
      next = json(422, err);
    },
  };
}

const httpLabels = {
  appInfo: 'GET /healthz',
  groupsScan: 'GET /api/v1/groups',
  groupsValidate: 'POST /api/v1/groups/validate',
  groupsRows: 'GET /api/v1/groups/rows?path=groups%2FBaca.parquet',
  groupsBatchTransferUnits: 'POST /api/v1/groups/batch-transfer-units',
  preferencesGet: 'GET /api/v1/preferences',
  preferencesPut: 'PUT /api/v1/preferences',
  transformationsSave: 'POST /api/v1/transformations',
  transformationsList: 'GET /api/v1/transformations',
  ordinationPca: 'POST /api/v1/ordination/pca',
  ordinationUmap: 'POST /api/v1/ordination/umap',
  clusterDiagnostics: 'POST /api/v1/cluster/diagnostics',
  clusterFit: 'POST /api/v1/cluster/fit',
  membershipProbabilities: 'POST /api/v1/membership/probabilities',
  euclideanMatches: 'POST /api/v1/euclidean/matches',
  filesUpload: 'POST /api/v1/files?path=sources%2FINAA_test.csv',
};

registerTransportContractTests(test, makeHarness, httpLabels);

test('http: phase 6 requests use the Rust route paths and snake_case JSON DTOs', async () => {
  const h = makeHarness();
  const request = { path: 'groups/A.parquet', columns: ['Ti'], max_k: 4, seed: 91 };
  h.respond({ path: request.path, revision_id: 'r', column_names: ['Ti'], n_rows: 5, wss: [], silhouette: [] });
  await h.transport.clustering.diagnostics(request);
  assert.equal(h.calls[0]?.label, 'POST /api/v1/cluster/diagnostics');
  assert.deepEqual(h.calls[0]?.payload, request);
});

test('http: group validate sends a bare JSON string body', async () => {
  const h = makeHarness();
  h.respond({
    path: 'g',
    group_id: 'id',
    group_name: 'g',
    revision_id: 'r',
    row_count: 1,
    source_path: null,
    source_sha256: null,
    elemental_columns: [],
    descriptive_columns: [],
  });
  await h.transport.groups.validate('groups/Baca.parquet');
  assert.equal(h.calls[0]?.label, 'POST /api/v1/groups/validate');
  assert.equal(h.calls[0]?.payload, 'groups/Baca.parquet');
});

test('http: group delete passes revision and confirmation as query params', async () => {
  const h = makeHarness();
  h.respond({
    transaction_id: 't',
    action: 'delete_group',
    outputs: [],
    deleted_paths: ['groups/Baca.parquet'],
  });
  await h.transport.groups.deleteGroup({
    path: 'groups/Baca.parquet',
    expected_revision: 'rev-1',
    confirm_path: 'groups/Baca.parquet',
  });
  assert.equal(
    h.calls[0]?.label,
    'DELETE /api/v1/groups/groups/Baca.parquet?expected_revision=rev-1&confirm_path=groups%2FBaca.parquet',
  );
  assert.equal(h.calls[0]?.payload, undefined);
});

test('http: file upload sends raw bytes with the logical path as a query param', async () => {
  const calls: RecordedCall[] = [];
  const bodies: Uint8Array[] = [];
  const transport: Transport = new HttpTransport('', async (input, init) => {
    const url = new URL(String(input), 'http://test.local');
    calls.push({ label: `${init?.method ?? 'GET'} ${url.pathname}${url.search}` });
    if (init?.body instanceof Uint8Array) bodies.push(init.body);
    return new Response(
      JSON.stringify({
        file_id: 'f-1',
        path: 'sources/a.csv',
        size_bytes: 3,
        sha256: 'abc',
        format: 'csv',
        parse_state: 'parsed',
        parse_error: null,
        deleted: false,
      }),
      { status: 200, headers: { 'content-type': 'application/json' } },
    );
  });
  await transport.files.upload('sources/a.csv', new Uint8Array([1, 2, 3]));
  assert.equal(calls[0]?.label, 'POST /api/v1/files?path=sources%2Fa.csv');
  assert.deepEqual(Array.from(bodies[0] ?? []), [1, 2, 3]);
});

test('http: error envelopes pass through; non-JSON errors synthesize http_<status>', async () => {
  const h = makeHarness();
  h.fail({ code: 'validation_error', message: 'bad group file' });
  await assert.rejects(
    () => h.transport.groups.validate('x'),
    (err: unknown) => err instanceof TransportError && err.envelope.code === 'validation_error',
  );
  let raw: (() => Response) | null = () => new Response('<html>gone</html>', { status: 404 });
  const transport: Transport = new HttpTransport('', async () => {
    const respond = raw;
    raw = null;
    return respond ? respond() : new Response('{}', { status: 200 });
  });
  await assert.rejects(
    () => transport.groups.scan(),
    (err: unknown) => err instanceof TransportError && err.envelope.code === 'http_404',
  );
});

test('http: auth service hits Section 10.1 routes and sends the CSRF header on writes', async () => {
  const calls: { url: URL; method: string; headers: Record<string, string>; body: unknown }[] = [];
  const transport: HttpTransport = new HttpTransport('', async (input, init) => {
    const url = new URL(String(input), 'http://test.local');
    let payload: unknown = null;
    if (typeof init?.body === 'string') payload = JSON.parse(init.body);
    calls.push({
      url,
      method: init?.method ?? 'GET',
      headers: (init?.headers ?? {}) as Record<string, string>,
      body: payload,
    });
    return new Response(JSON.stringify({ authenticated: false }), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    });
  });
  // No document in this environment: no CSRF cookie can be read, so writes
  // go out without the header (the browser context supplies it).
  assert.equal(typeof document, 'undefined');

  await transport.auth.consent();
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/consent');

  await transport.auth.register({
    username: 'user',
    email: 'user@example.com',
    password: 'correct horse battery staple',
    consentVersion: '2026-10',
  });
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/register');
  assert.deepEqual(calls.at(-1)?.body, {
    username: 'user',
    email: 'user@example.com',
    password: 'correct horse battery staple',
    consent_version: '2026-10',
  });

  await transport.auth.verify('tok-123');
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/verify');
  assert.deepEqual(calls.at(-1)?.body, { token: 'tok-123' });

  const session = await transport.auth.login({
    identifier: 'user',
    password: 'correct horse battery staple',
    rememberDays: 90,
  });
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/login');
  assert.deepEqual(calls.at(-1)?.body, {
    identifier: 'user',
    password: 'correct horse battery staple',
    remember_days: 90,
  });
  assert.equal(session.authenticated, false);

  await transport.auth.session();
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/session');

  await transport.auth.logout();
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/logout');

  await transport.auth.logoutAll();
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/logout-all');

  await transport.auth.requestPasswordReset('user@example.com');
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/password-reset/request');
  assert.deepEqual(calls.at(-1)?.body, { email: 'user@example.com' });

  await transport.auth.confirmPasswordReset({ token: 'tok', newPassword: 'pw' });
  assert.equal(calls.at(-1)?.url.pathname, '/api/v1/auth/password-reset/confirm');
  assert.deepEqual(calls.at(-1)?.body, { token: 'tok', new_password: 'pw' });

  // Every auth write used POST (state-changing methods carry CSRF when a
  // cookie is present; GETs never do).
  for (const call of calls) {
    if (call.method === 'GET') {
      assert.equal(call.headers['x-csrf-token'], undefined);
    }
  }
});

test('http: CSRF header echoes the archaeodash_csrf cookie on writes', async () => {
  const seen: (string | undefined)[] = [];
  const transport = new HttpTransport('', async (input, init) => {
    const url = new URL(String(input), 'http://test.local');
    seen.push((init?.headers as Record<string, string>)?.['x-csrf-token']);
    return new Response(null, { status: 204 });
  });
  // Simulate the browser cookie jar (non-HttpOnly csrf + HttpOnly session is
  // invisible to document.cookie; only csrf reaches the header).
  const original = globalThis.document;
  (globalThis as { document?: unknown }).document = {
    cookie: 'archaeodash_session=secret; archaeodash_csrf=csrf-token-1',
  } as unknown as typeof document;
  try {
    await transport.preferences.put('theme', 'dark');
    await transport.preferences.get();
  } finally {
    (globalThis as { document?: unknown }).document = original;
  }
  assert.equal(seen[0], 'csrf-token-1');
  assert.equal(seen[1], undefined);
});
