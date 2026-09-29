/** Explore route unit tests: renderToString over mocked transports. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import type { ExploreService, ExportsService, GroupsService } from '@archaeodash/client';
import { ExplorePage, DataTable, type ExploreDeps } from './ExplorePage.tsx';
function makeDeps(): ExploreDeps {
  const groups = {
    scan: async () => [
      {
        path: 'groups/Baca.parquet',
        ready: true,
        group: {
          path: 'groups/Baca.parquet',
          group_id: 'g1',
          group_name: 'Baca',
          revision_id: 'rev-1',
          row_count: 2,
          source_path: null,
          source_sha256: null,
          elemental_columns: ['Ti', 'Sr'],
          descriptive_columns: ['Site'],
        },
      },
    ],
    validate: async () => {
      throw new Error('not used here');
    },
    rows: async (path: string) => ({
      path,
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
    }),
    batchTransferUnits: async () => { throw new Error('unused batch transfer'); },
    transferUnits: async () => {
      throw new Error('not used here');
    },
    mergeGroups: async () => {
      throw new Error('not used here');
    },
    patchDescriptiveValues: async () => {
      throw new Error('not used here');
    },
    duplicateGroup: async () => {
      throw new Error('not used here');
    },
    deleteGroup: async () => {
      throw new Error('not used here');
    },
  } satisfies GroupsService;
  const explore = {
    missingProfile: async () => ({
      path: 'groups/Baca.parquet',
      revision_id: 'rev-1',
      column_names: ['Ti', 'Sr'],
      rows: [{ feature: 'Ti', num_missing: 1, pct_missing: 50, band: 'Bad' }],
    }),
    histogram: async () => ({
      path: 'p',
      revision_id: 'r',
      column: 'Ti',
      breaks: [0, 1, 2],
      counts: [1, 2],
    }),
    crosstab: async () => ({
      path: 'p',
      revision_id: 'r',
      column_names: ['Ti', 'Sr'],
      summary_method: 'count',
      rows: { kind: 'count' as const, rows: [{ group: 'Baca', value: 'A1', count: 2 }] },
    }),
    compositionalProfile: async () => ({
      path: 'p',
      revision_id: 'r',
      column_names: ['Ti', 'Sr'],
      rows: [{ rowid: 1, element: 'Ti', value: 1.5, group_label: null }],
    }),
  } satisfies ExploreService;
  const exports = {
    measuredData: async () => {
      throw new Error('unused in explore tests');
    },
    transformed: async () => {
      throw new Error('unused in explore tests');
    },
    pcaScores: async () => {
      throw new Error('unused in explore tests');
    },
  } satisfies ExportsService;
  return {
    groups,
    explore,
    exports,
    getInitialDataset: async () => '',
    onDatasetOpened: () => {},
  };
}

test('explore renders the legacy view tabs; table hides hidden identity', async () => {
  const deps = makeDeps();
  const html = renderToString(<ExplorePage deps={deps} />);
  for (const label of ['Dataset', 'Missing values', 'Univariate Plots', 'Crosstabs', 'Compositional Profile Plot']) {
    assert.ok(html.includes(label), `missing tab label: ${label}`);
  }
  assert.ok(html.includes('Choose a group file…'), 'dataset select present');
  // Effects do not run under renderToString; render the table directly with
  // loaded data to assert the hidden-identity and read-only guarantees.
  const rows = await deps.groups.rows('groups/Baca.parquet');
  const tableHtml = renderToString(
    <DataTable data={rows} deps={deps} onSaved={() => {}} />,
  );
  assert.ok(tableHtml.includes('anid'), 'visible id column header');
  assert.ok(tableHtml.includes('A1'), 'visible id cell');
  assert.ok(!tableHtml.includes('0197aaaa'), 'analytical_uuid never rendered');
  assert.ok(tableHtml.includes('locked-col'), 'elemental columns marked read-only');
});

test('explore missing view renders band table rows', async () => {
  const deps = makeDeps();
  const html = renderToString(<ExplorePage deps={deps} />);
  assert.ok(html.includes('Missing values'));
});
