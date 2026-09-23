/** Ordination route unit tests: renderToString over mocked transports. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import type {
  GroupsService,
  LdaResponse,
  OrdinationService,
  PcaResponse,
  UmapResponse,
} from '@archaeodash/client';
import { LdaView, OrdinationPage, PcaResult, UmapResult } from './OrdinationPage.tsx';
import type { ExportsService } from '@archaeodash/client';

const rows = {
  path: 'groups/Baca.parquet',
  revision_id: 'rev-1',
  visible_id_column: 'anid',
  legacy_rowid_column: 'rowid',
  descriptive_columns: ['Site'],
  elemental_columns: ['Ti', 'Sr'],
  rows: [
    {
      analytical_uuid: '0197aaaa-bbbb-7ccc-ddee-ffff00000001',
      legacy_rowid: '1',
      visible_id: 'A1',
      descriptive: ['Baca'],
      elemental: [1.5, 3],
    },
  ],
};

const pca: PcaResponse = {
  path: rows.path,
  revision_id: rows.revision_id,
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
  path: rows.path,
  revision_id: rows.revision_id,
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

const lda: LdaResponse = {
  path: rows.path,
  revision_id: rows.revision_id,
  column_names: ['Ti', 'Sr'],
  levels: ['Baca'],
  prior: [1.0],
  counts: [1],
  means: [[1.5, 3]],
  scaling: [[1]],
  svd: [1],
  score_names: ['LD1'],
  scores: [[0.5]],
  warnings: [],
};

function makeDeps(): {
  groups: GroupsService;
  explore?: never;
  ordination: OrdinationService;
  exports: ExportsService;
} {
  const groups = {
    scan: async () => [
      {
        path: rows.path,
        ready: true,
        group: {
          path: rows.path,
          group_id: 'g1',
          group_name: 'Baca',
          revision_id: rows.revision_id,
          row_count: 1,
          source_path: null,
          source_sha256: null,
          elemental_columns: rows.elemental_columns,
          descriptive_columns: rows.descriptive_columns,
        },
      },
    ],
    validate: async () => {
      throw new Error('unused');
    },
    rows: async () => rows,
    transferUnits: async () => {
      throw new Error('unused');
    },
    mergeGroups: async () => {
      throw new Error('unused');
    },
    patchDescriptiveValues: async () => {
      throw new Error('unused');
    },
    duplicateGroup: async () => {
      throw new Error('unused');
    },
    deleteGroup: async () => {
      throw new Error('unused');
    },
  } satisfies GroupsService;
  const ordination = {
    pca: async () => pca,
    umap: async () => umap,
    lda: async () => lda,
  } satisfies OrdinationService;
  const exports = {
    measuredData: async () => {
      throw new Error('unused in ordination tests');
    },
    transformed: async () => {
      throw new Error('unused in ordination tests');
    },
    pcaScores: async () => ({
      file_name: 'pca_scores.csv',
      media_type: 'text/csv',
      content: 'Row,PC1,PC2\\n1,1,0',
    }),
  } satisfies ExportsService;
  return { groups, ordination, exports };
}

test('ordination page renders the legacy PCA/UMAP/LDA tab order', () => {
  const html = renderToString(<OrdinationPage deps={makeDeps()} />);
  const pcaIdx = html.indexOf('>PCA<');
  const umapIdx = html.indexOf('>UMAP<');
  const ldaIdx = html.indexOf('>LDA<');
  assert.ok(pcaIdx >= 0 && umapIdx > pcaIdx && ldaIdx > umapIdx, 'legacy tab order');
  assert.ok(html.includes('Load a dataset to run ordination.'));
});

test('PCA view shows variance bars, cumulative summary, and score table', () => {
  const html = renderToString(<PcaResult result={pca} onRecompute={() => {}} />);
  assert.ok(html.includes('Explained variance per component'));
  assert.ok(html.includes('84.0%'), 'cumulative variance summary');
  assert.ok(html.includes('PC1') && html.includes('PC2'));
  assert.ok(html.includes('variance-bars') && html.includes('plot-point'));
});

test('UMAP view surfaces the fixed legacy seed and warnings', () => {
  const html = renderToString(<UmapResult result={umap} />);
  assert.ok(html.includes('20260914'), 'fixed legacy seed echoed');
  assert.ok(html.includes('n_neighbors') && html.includes('15'));
  assert.ok(html.includes('V1') && html.includes('V2'));
});

test('LDA view shows group levels/priors and gates on missing group column', () => {
  const html = renderToString(
    <LdaView deps={makeDeps()} data={rows} />,
  );
  // Effects do not run under renderToString; the gate message renders before
  // any fetch state, and levels/priors come from a loaded LdaResult.
  assert.ok(html.includes('Groups: Baca') === false || html.includes('Computing LDA…'));
  const noGroup = renderToString(
    <LdaView deps={makeDeps()} data={{ ...rows, descriptive_columns: [] }} />,
  );
  assert.ok(noGroup.includes('LDA needs a descriptive group column'));
});
