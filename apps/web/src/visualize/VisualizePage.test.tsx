/** Visualize & Assign tests: pure model + SSR page structure over a stubbed transport. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import type {
  ExportsService,
  GroupRowsResponse,
  GroupsService,
  OrdinationService,
} from '@archaeodash/client';
import { SelectedRowsTable, VisualizePage, transposeElementalRows, type VisualizeDeps } from './VisualizePage.tsx';
import { Multiplot } from './Multiplot.tsx';
import { chiSquare2, symbolFor } from './visualize-model.ts';

const rows: GroupRowsResponse = {
  path: 'groups/Baca.parquet',
  revision_id: 'rev-1',
  visible_id_column: 'anid',
  legacy_rowid_column: 'rowid',
  descriptive_columns: ['Site', 'Group'],
  elemental_columns: ['Ti', 'Sr'],
  rows: [
    {
      analytical_uuid: '0197aaaa-0000-7000-8000-000000000001',
      legacy_rowid: '1',
      visible_id: 'A1',
      descriptive: ['Baca', 'Baca'],
      elemental: [1.5, 3.0],
    },
    {
      analytical_uuid: '0197aaaa-0000-7000-8000-000000000002',
      legacy_rowid: '2',
      visible_id: 'A2',
      descriptive: ['Other', 'Baca'],
      elemental: [2.5, 4.0],
    },
  ],
};

function makeDeps(): VisualizeDeps {
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
          row_count: 2,
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
    pca: async () => {
      throw new Error('PCA tested through OrdinationPage');
    },
    umap: async () => {
      throw new Error('unused');
    },
    lda: async () => {
      throw new Error('unused');
    },
  } satisfies OrdinationService;
  const exports = {
    measuredData: async () => {
      throw new Error('unused');
    },
    transformed: async () => {
      throw new Error('unused');
    },
    pcaScores: async () => {
      throw new Error('unused');
    },
  } satisfies ExportsService;
  return { groups, ordination, exports };
}

test('visualize page renders axis/filter/ellipse/symbol/label/assignment controls', async () => {
  const html = renderToString(<VisualizePage deps={makeDeps()} />);
  for (const label of [
    'Visualize &amp; Assign',
    'Dataset',
    'X axis',
    'Y axis',
    'PCA',
    'Ellipse',
    'Symbols by',
    'Labels',
    'Assign selected units',
    'Target group',
    'Selected rows',
  ]) {
    assert.ok(html.includes(label), `missing control label: ${label}`);
  }
  assert.ok(html.includes('selected'), 'selection summary present');
});

test('selected-rows table shows ANID, metadata, and predictors but never the uuid', () => {
  const html = renderToString(
    <SelectedRowsTable
      rows={rows.rows}
      columns={{ visibleId: rows.visible_id_column, legacyRowid: rows.legacy_rowid_column }}
      descriptiveColumns={rows.descriptive_columns}
      elementalColumns={rows.elemental_columns}
    />,
  );
  assert.ok(html.includes('A1'), 'visible id rendered');
  assert.ok(html.includes('Ti') && html.includes('Sr'), 'predictor columns rendered');
  assert.ok(!html.includes('0197aaaa'), 'analytical_uuid never rendered');
  assert.ok(!html.includes('u-'), 'no uuid-shaped cell rendered');
});

test('empty selection renders the empty-table state', () => {
  const html = renderToString(
    <SelectedRowsTable
      rows={[]}
      columns={{ visibleId: 'anid', legacyRowid: 'rowid' }}
      descriptiveColumns={['Site']}
      elementalColumns={['Ti']}
    />,
  );
  assert.ok(html.includes('No rows selected.'));
});

test('ten-symbol map and chi-square ellipse quantiles match the legacy contract', () => {
  assert.equal(symbolFor(0), 'circle');
  assert.equal(symbolFor(10), 'circle');
  assert.ok(Math.abs(chiSquare2(0.95) - 5.991464547107979) < 1e-12);
});

test('multiplot input transposes row-major API data into column-major plot arrays', () => {
  assert.deepEqual(transposeElementalRows(rows.rows, rows.elemental_columns.length), [
    [1.5, 2.5],
    [3.0, 4.0],
  ]);
  const wider = rows.rows.map((row, i) => ({ ...row, elemental: [i + 1, i + 11, i + 21] }));
  assert.deepEqual(transposeElementalRows(wider, 3), [
    [1, 2],
    [11, 12],
    [21, 22],
  ]);
});

test('multiplot renders static SVG panels for all disjoint pairs without interactive sampling status', () => {
  const values: (number | null)[][] = [
    [1, 2, 3, 4],
    [2, 4, 6, 8],
    [null, 1, 2, 3],
  ];
  const html = renderToString(
    <Multiplot
      columns={['Ti', 'Sr', 'Zr']}
      values={values}
      groupLabels={['Baca', 'Baca', 'Other', 'Other']}
      groupNames={['Baca', 'Other']}
      rowIndices={[0, 1, 2, 3]}
    />,
  );
  // 3 columns -> 6 ordered pairs.
  assert.equal((html.match(/multiplot-panel/g) ?? []).length, 6, 'one panel per ordered pair');
  assert.ok(html.includes('Scatter of Sr by Ti'), 'aria labels name the axes');
  assert.ok(html.includes('Save plots (SVG)'), 'plot save control present');
  // Null cells render nothing but panels still exist.
  assert.ok(!html.includes('NaN'));
  assert.ok(!html.includes('Sampled'), 'static mode renders all selected rows');
});

test('multiplot point size and height controls are exposed', () => {
  const html = renderToString(
    <Multiplot
      columns={['Ti', 'Sr']}
      values={[[1, 2], [3, 4]]}
      groupLabels={['All', 'All']}
      groupNames={['All']}
      rowIndices={[0, 1]}
    />,
  );
  assert.ok(html.includes('Point size'), 'point size control');
  assert.ok(html.includes('Height'), 'height control');
});
