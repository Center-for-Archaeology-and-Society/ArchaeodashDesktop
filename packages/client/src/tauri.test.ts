import assert from 'node:assert/strict';
import { test } from 'node:test';
import { TauriTransport } from './tauri.ts';
import { TransportError, isAbortError, type Transport } from './transport.ts';
import {
  registerTransportContractTests,
  type Harness,
  type RecordedCall,
} from './contract-suite.ts';

function makeHarness(): Harness {
  const calls: RecordedCall[] = [];
  let next: { value?: unknown; error?: string } | null = null;
  const invoke = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ label: `invoke ${cmd}`, payload: args });
    if (next === null) throw new Error('no queued response');
    const pending = next;
    next = null;
    if (pending.error !== undefined) throw pending.error;
    return pending.value as T;
  };
  return {
    transport: new TauriTransport(invoke),
    calls,
    respond(result) {
      next = { value: result };
    },
    fail(err) {
      next = { error: err.message };
    },
  };
}


const tauriLabels = {
  appInfo: 'invoke app_info',
  groupsScan: 'invoke scan_group_candidates',
  groupsValidate: 'invoke validate_group_file',
  groupsRows: 'invoke group_rows',
  groupsBatchTransferUnits: 'invoke batch_transfer_units',
  preferencesGet: 'invoke preferences_get',
  preferencesPut: 'invoke preferences_set',
  transformationsSave: 'invoke save_transformation',
  transformationsList: 'invoke list_transformations',
  ordinationPca: 'invoke ordination_pca',
  ordinationUmap: 'invoke ordination_umap',
  clusterDiagnostics: 'invoke cluster_diagnostics',
  clusterFit: 'invoke cluster_fit',
  membershipProbabilities: 'invoke membership_probabilities',
  euclideanMatches: 'invoke euclidean_matches',
  filesUpload: 'invoke upload_source_file',
};

registerTransportContractTests(test, makeHarness, tauriLabels);

test('tauri: phase 6 commands wrap snake_case DTOs under request', async () => {
  const h = makeHarness();
  h.respond({ path: 'groups/A.parquet', revision_id: 'r', rows: [] });
  const request = {
    path: 'groups/A.parquet', columns: ['Ti'], group_column: 'Group', id_column: 'ID',
    limit: 3, within_group: true,
  };
  await h.transport.clustering.euclideanMatches(request);
  assert.equal(h.calls[0]?.label, 'invoke euclidean_matches');
  assert.deepEqual(h.calls[0]?.payload, { request });
});

test('tauri: save_transformation takes the bare definition, not the save wrapper', async () => {
  const h = makeHarness();
  h.respond({
    definition: {
      name: 'log10-default',
      transform_method: 'log10',
      imputation_method: 'none',
      imputation_seed: null,
      elemental_columns: ['Ti'],
      descriptive_columns: [],
      group_column: null,
      ratios: [],
      ratio_mode: 'append',
    },
    replaced: false,
  });
  await h.transport.transformations.save({
    definition: {
      name: 'log10-default',
      transform_method: 'log10',
      imputation_method: 'none',
      imputation_seed: null,
      elemental_columns: ['Ti'],
      descriptive_columns: [],
      group_column: null,
      ratios: [],
      ratio_mode: 'append',
    },
  });
  const args = h.calls[0]?.payload as Record<string, unknown>;
  assert.equal(h.calls[0]?.label, 'invoke save_transformation');
  assert.deepEqual(Object.keys(args), ['definition']);
});

test('tauri: file command args use camelCase keys over snake_case Rust params', async () => {
  const h = makeHarness();
  h.respond({ file_id: 'f-1', path: 'p', size_bytes: 0, sha256: 's', format: 'csv',
    parse_state: 'parsed', parse_error: null, deleted: false });
  await h.transport.files.metadata('f-1');
  assert.equal(h.calls[0]?.label, 'invoke source_file_metadata');
  assert.deepEqual(h.calls[0]?.payload, { fileId: 'f-1' });
});

test('tauri: source picker passes no path and cancellation stays null', async () => {
  const h = makeHarness();
  h.respond(null);
  assert.equal(await h.transport.files.pickImportSource?.(), null);
  assert.equal(h.calls[0]?.label, 'invoke pick_and_upload_source');
  assert.equal(h.calls[0]?.payload, undefined);
});

test('tauri: string rejections normalize into tauri_error envelopes', async () => {
  const h = makeHarness();
  h.fail({ code: 'validation_error', message: 'bad group file' });
  await assert.rejects(
    () => h.transport.groups.validate('x'),
    (err: unknown) =>
      err instanceof TransportError &&
      err.envelope.code === 'tauri_error' &&
      err.envelope.message === 'bad group file',
  );
});

test('tauri: structured envelope rejections keep their code', async () => {
  const h = makeHarness();
  h.transport; // adapter under test
  const invoke = async () => {
    throw { code: 'validation_error', message: 'nope' };
  };
  const transport = new TauriTransport(invoke);
  await assert.rejects(
    () => transport.preferences.get(),
    (err: unknown) => err instanceof TransportError && err.envelope.code === 'validation_error',
  );
});

const umapRequest = { path: 'groups/A.parquet', extra_paths: [], columns: ['Ti', 'Sr'], transformation: null, seed: 1 };

test('tauri: an aborted ordination rejects at once and cancels its backend run', async () => {
  const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
  let finish: (value: unknown) => void = () => undefined;
  const invoke = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    if (cmd === 'cancel_ordination') return Promise.resolve(undefined as T);
    return new Promise<T>((resolve) => { finish = resolve as (value: unknown) => void; });
  };
  const transport = new TauriTransport(invoke);
  const controller = new AbortController();
  const pending = transport.ordination.umap(umapRequest, { signal: controller.signal });
  controller.abort();
  await assert.rejects(pending, (err: unknown) => isAbortError(err));
  assert.equal(calls[0]?.cmd, 'ordination_umap');
  const token = calls[0]?.args?.token;
  assert.equal(typeof token, 'string');
  assert.deepEqual(calls[0]?.args?.request, umapRequest);
  assert.deepEqual(calls[1], { cmd: 'cancel_ordination', args: { token } });
  // The late backend reply is ignored.
  finish({});
  await Promise.resolve();
  assert.equal(calls.length, 2);
});

test('tauri: a signal that never aborts resolves normally with a token; no signal sends none', async () => {
  const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
  const invoke = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ cmd, args });
    return { score_names: ['PC1'] } as T;
  };
  const transport = new TauriTransport(invoke);
  const out = await transport.ordination.pca(umapRequest, { signal: new AbortController().signal });
  assert.deepEqual(out, { score_names: ['PC1'] });
  assert.equal(typeof calls[0]?.args?.token, 'string');
  await transport.ordination.lda({ ...umapRequest, group_column: 'Site' });
  assert.deepEqual(Object.keys(calls[1]?.args ?? {}), ['request']);
  // Two runs never share a token.
  await transport.ordination.pca(umapRequest, { signal: new AbortController().signal });
  assert.notEqual(calls[2]?.args?.token, calls[0]?.args?.token);
});

test('tauri: an already-aborted signal rejects without invoking', async () => {
  const calls: string[] = [];
  const invoke = async <T>(cmd: string): Promise<T> => {
    calls.push(cmd);
    return undefined as T;
  };
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(
    new TauriTransport(invoke).ordination.umap(umapRequest, { signal: controller.signal }),
    (err: unknown) => isAbortError(err),
  );
  assert.deepEqual(calls, []);
});
