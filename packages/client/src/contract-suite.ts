/**
 * Shared transport contract suite (Section 9.2): the same behavioral tests run
 * against the HTTP and Tauri adapters. Payload-shape specifics (HTTP bodies,
 * query strings, Tauri camelCase invoke args) are asserted per adapter; this
 * suite pins the endpoint/command mapping, pass-through, and error
 * normalization common to both delivery modes.
 */
import assert from 'node:assert/strict';
import type { Transport } from './transport.ts';
import type {
  AppInfo,
  GetPreferencesResponse,
  GroupCandidate,
  PcaResponse,
  SaveTransformationResponse,
  StagedFile,
  TransformationListResponse,
  UmapResponse,
} from '@archaeodash/contracts';

export interface RecordedCall {
  /** HTTP: `METHOD /path`. Tauri: `invoke <command>`. */
  label: string;
  /** HTTP: parsed JSON body (undefined when none). Tauri: invoke args object. */
  payload?: unknown;
}

export interface Harness {
  transport: Transport;
  calls: RecordedCall[];
  /** Queue the next successful response (JSON body, or 204/undefined for void). */
  respond(result: unknown): void;
  /** Queue the next failure; both adapters surface `code` per their backend. */
  fail(err: { code: string; message: string }): void;
}

export type TestFn = (name: string, fn: () => Promise<void> | void) => void;

const appInfo: AppInfo = {
  app: 'archaeodash',
  version: '0.1.0',
  transport: 'http',
  ready: true,
};

const candidates: GroupCandidate[] = [
  {
    path: 'groups/Baca.parquet',
    ready: true,
    group: {
      path: 'groups/Baca.parquet',
      group_id: '0197aaaa-bbbb-7ccc-ddee-ffff00000001',
      group_name: 'Baca',
      revision_id: 'rev-1',
      row_count: 3,
      source_path: null,
      source_sha256: null,
      elemental_columns: ['Ti', 'Sr'],
      descriptive_columns: ['Group'],
    },
  },
];

const pca: PcaResponse = {
  path: 'groups/Baca.parquet',
  revision_id: 'rev-1',
  column_names: ['Ti', 'Sr'],
  score_names: ['PC1', 'PC2'],
  sdev: [1.4, 0.6],
  explained_variance: [0.84, 0.16],
  cumulative_variance: [0.84, 1.0],
  center: [0.1, 0.2],
  scale: null,
  rotation: [
    [0.7, -0.7],
    [0.7, 0.7],
  ],
  scores: [
    [1.0, 0.0],
    [-1.0, 0.0],
  ],
};

const umap: UmapResponse = {
  path: 'groups/Baca.parquet',
  revision_id: 'rev-1',
  column_names: ['Ti', 'Sr'],
  score_names: ['V1', 'V2'],
  embedding: [
    [0.1, 0.2],
    [0.3, 0.4],
  ],
  seed: 20260914,
  n_neighbors: 15,
  n_epochs: 500,
  a: 1.577,
  b: 0.895,
  warnings: [],
};

export function registerTransportContractTests(
  test: TestFn,
  makeHarness: () => Harness,
  labels: Record<string, string>,
): void {
  test('appInfo reaches the health surface and passes the envelope through', async () => {
    const h = makeHarness();
    h.respond(appInfo);
    assert.deepEqual(await h.transport.appInfo(), appInfo);
    assert.equal(h.calls[0]?.label, labels.appInfo);
    assert.equal(h.calls[0]?.payload, undefined);
  });

  test('groups.scan lists candidates', async () => {
    const h = makeHarness();
    h.respond(candidates);
    assert.deepEqual(await h.transport.groups.scan(), candidates);
    assert.equal(h.calls[0]?.label, labels.groupsScan);
  });

  test('preferences round-trip: get entries, put one key (void)', async () => {
    const h = makeHarness();
    const prefs: GetPreferencesResponse = {
      preferences: [{ key: 'theme', value: 'dark' }],
    };
    h.respond(prefs);
    assert.deepEqual(await h.transport.preferences.get(), prefs);
    assert.equal(h.calls[0]?.label, labels.preferencesGet);
    h.respond(undefined);
    await h.transport.preferences.put('theme', 'dark');
    assert.equal(h.calls[1]?.label, labels.preferencesPut);
  });

  test('transformations save/list pass through', async () => {
    const h = makeHarness();
    const list: TransformationListResponse = {
      transformations: [
        {
          name: 'log10-default',
          created_at_unix_secs: 1_760_000_000,
          transform_method: 'log10',
          imputation_method: 'none',
          ratio_count: 0,
        },
      ],
    };
    h.respond(list);
    assert.deepEqual(await h.transport.transformations.list(), list);
    assert.equal(h.calls[0]?.label, labels.transformationsList);
    const saved: SaveTransformationResponse = {
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
    };
    h.respond(saved);
    assert.deepEqual(
      await h.transport.transformations.save({ definition: saved.definition }),
      saved,
    );
    assert.equal(h.calls[1]?.label, labels.transformationsSave);
  });

  test('ordination pca/umap pass through numeric matrices', async () => {
    const h = makeHarness();
    h.respond(pca);
    const pcaOut = await h.transport.ordination.pca({
      path: 'groups/Baca.parquet',
      columns: ['Ti', 'Sr'],
    });
    assert.deepEqual(pcaOut, pca);
    assert.equal(h.calls[0]?.label, labels.ordinationPca);
    h.respond(umap);
    const umapOut = await h.transport.ordination.umap({
      path: 'groups/Baca.parquet',
      columns: ['Ti', 'Sr'],
      seed: 20260914,
    });
    assert.deepEqual(umapOut, umap);
    assert.equal(h.calls[1]?.label, labels.ordinationUmap);
  });

  test('files.upload passes bytes and returns the staged record', async () => {
    const h = makeHarness();
    const staged: StagedFile = {
      file_id: 'f-1',
      path: 'sources/INAA_test.csv',
      size_bytes: 3,
      sha256: 'abc',
      format: 'csv',
      parse_state: 'parsed',
      parse_error: null,
      deleted: false,
    };
    h.respond(staged);
    const out = await h.transport.files.upload(
      'sources/INAA_test.csv',
      new Uint8Array([1, 2, 3]),
    );
    assert.deepEqual(out, staged);
    assert.equal(h.calls[0]?.label, labels.filesUpload);
  });

  test('errors normalize into TransportError with backend code and message', async () => {
    const h = makeHarness();
    h.fail({ code: 'validation_error', message: 'bad group file' });
    await assert.rejects(
      () => h.transport.groups.validate('groups/Baca.parquet'),
      (err: unknown) => err instanceof Error && 'envelope' in err,
    );
    assert.equal(h.calls[0]?.label, labels.groupsValidate);
  });
}
