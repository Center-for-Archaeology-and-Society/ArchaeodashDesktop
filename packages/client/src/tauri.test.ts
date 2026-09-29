import assert from 'node:assert/strict';
import { test } from 'node:test';
import { TauriTransport } from './tauri.ts';
import { TransportError, type Transport } from './transport.ts';
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
