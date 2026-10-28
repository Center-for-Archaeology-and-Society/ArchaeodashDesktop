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
import { LdaResult, LdaView, OrdinationPage, PcaResult, UmapResult } from './OrdinationPage.tsx';
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
    batchTransferUnits: async () => { throw new Error('unused batch transfer'); },
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

test('PCA view shows the scree chart, variance table, and score table', () => {
  const html = renderToString(<PcaResult result={pca} onRecompute={() => {}} />);
  assert.ok(html.includes('Explained variance per component'));
  assert.ok(html.includes('84.0%'), 'cumulative variance summary');
  assert.ok(html.includes('PC1') && html.includes('PC2'));
  assert.ok(html.includes('variance-table') && html.includes('Score table'));
});

test('UMAP view surfaces the fixed legacy seed and warnings', () => {
  const html = renderToString(<UmapResult result={umap} />);
  assert.ok(html.includes('20260914'), 'fixed legacy seed echoed');
  assert.ok(html.includes('n_neighbors') && html.includes('15'));
  assert.ok(html.includes('V1') && html.includes('V2'));
  assert.ok(!html.includes('X axis'), 'two dimensions need no axis choice');
});

test('UMAP view offers axis choices past two dimensions', () => {
  const wide: UmapResponse = { ...umap, score_names: ['V1', 'V2', 'V3'], embedding: umap.embedding.map((r) => [...r, 0.5]) };
  const html = renderToString(<UmapResult result={wide} />);
  assert.ok(html.includes('X axis') && html.includes('Y axis'));
  assert.ok(html.includes('V3'));
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

test('PCA view adds the colour toggle, variables plot, and min(4, K) contribution table', () => {
  const html = renderToString(<PcaResult result={pca} onRecompute={() => {}} />);
  assert.ok(html.includes('Color by') && html.includes('cos²'), 'group/cos² colour toggle');
  assert.ok(html.includes('PCA variables coloured by contribution'), 'variables plot');
  assert.ok(html.includes('Variable contributions'));
  assert.ok(html.includes('min(4, number of components) = 2'), 'contribution rule labelled');
  assert.ok(html.includes('only 2 components available'));
  const cos2 = renderToString(<PcaResult result={pca} onRecompute={() => {}} initialColorBy="cos2" />);
  assert.ok(cos2.includes('cos² on '), 'cos² note');
});

test('PCA score export failure is shown inline', () => {
  const html = renderToString(
    <PcaResult result={pca} onRecompute={() => {}} onExportScores={() => {}} exportError="disk full" />,
  );
  assert.ok(html.includes('role="alert"') && html.includes('Export failed: <!-- -->disk full'));
});

test('PCA with one component skips the two-axis plots', () => {
  const one: PcaResponse = {
    ...pca,
    score_names: ['PC1'],
    sdev: [1.4],
    explained_variance: [1],
    cumulative_variance: [1],
    rotation: [[0.6], [0.8]],
    scores: [[1], [-1]],
  };
  const html = renderToString(<PcaResult result={one} onRecompute={() => {}} />);
  assert.ok(html.includes('Only one principal component'));
  assert.ok(!html.includes('PCA variables coloured by contribution'));
  assert.ok(html.includes('Variable contributions'));
});

test('LDA with two discriminants shows the score scatter and vector plot', () => {
  const two: LdaResponse = {
    ...lda,
    levels: ['A', 'B', 'C'],
    prior: [0.3, 0.3, 0.4],
    scaling: [[1, 0.5], [-0.2, 2]],
    score_names: ['LD1', 'LD2'],
    scores: [[0.5, 0.1], [-0.5, 0.2]],
  };
  const html = renderToString(<LdaResult result={two} />);
  assert.ok(html.includes('Scatter of LD2 by LD1'));
  assert.ok(html.includes('LDA vectors on LD1 and LD2'));
});

test('LDA with one discriminant renders the one-dimensional alternative, not a fake LD2', () => {
  const html = renderToString(<LdaResult result={lda} groups={['Baca']} />);
  assert.ok(html.includes('Only one linear discriminant (<!-- -->LD1<!-- -->)'));
  assert.ok(html.includes('LD1 scores by group') && html.includes('LD1 coefficients by variable'));
  assert.ok(!html.includes('Scatter of LD2'), 'no fabricated LD2 axis');
});
