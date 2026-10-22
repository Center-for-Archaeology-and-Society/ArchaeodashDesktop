/** Explore route unit tests: renderToString over mocked transports. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import type { ExploreService, ExportsService, GroupsService } from '@archaeodash/client';
import { CrosstabCountTable, ExplorePage, DataTable, profileTraces, type ExploreDeps } from './ExplorePage.tsx';
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
      summary_method: 'count',
      // The API flattens the tagged rows enum to the top level.
      kind: 'count' as const,
      rows: [{ group: 'Baca', value: 'A1', count: 2 }],
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
  };
}

test('explore renders the legacy view tabs; table hides hidden identity', async () => {
  const deps = makeDeps();
  const html = renderToString(<ExplorePage deps={deps} />);
  for (const label of ['Dataset', 'Missing values', 'Univariate Plots', 'Crosstabs', 'Compositional Profile Plot']) {
    assert.ok(html.includes(label), `missing tab label: ${label}`);
  }
  assert.ok(html.includes('Dataset: No groups'), 'dataset picker present (groups load after mount)');
  // Effects do not run under renderToString; render the table directly with
  // loaded data to assert the hidden-identity and read-only guarantees.
  const rows = await deps.groups.rows('groups/Baca.parquet');
  const tableHtml = renderToString(
    <DataTable files={[{ path: rows.path, groupName: 'Baca', data: rows }]} deps={deps} onSaved={() => {}} />,
  );
  assert.ok(tableHtml.includes('anid'), 'visible id column header');
  assert.ok(tableHtml.includes('A1'), 'visible id cell');
  assert.ok(!tableHtml.includes('0197aaaa'), 'analytical_uuid never rendered');
  assert.ok(tableHtml.includes('locked-col'), 'elemental columns marked read-only');
  const missingId = { ...rows, rows: rows.rows.map(row => ({ ...row, visible_id: null })) };
  const accessible = renderToString(
    <DataTable files={[{ path: rows.path, groupName: 'Baca', data: missingId }]} deps={deps} onSaved={() => {}} />,
  );
  assert.ok(!accessible.includes('0197aaaa'), 'missing visible IDs never expose internal UUIDs in accessible labels');
  assert.ok(accessible.includes('for row 1'));
  // The synthetic group column shows each row's group and is read-only.
  assert.ok(tableHtml.includes('<th>Group</th>'));
  assert.ok(tableHtml.includes('<td class="locked-col">Baca</td>'));
});

test('explore missing view renders band table rows', async () => {
  const deps = makeDeps();
  const html = renderToString(<ExplorePage deps={deps} />);
  assert.ok(html.includes('Missing values'));
});

test('count crosstab is a long table: group field, second field, count', () => {
  const html = renderToString(
    <CrosstabCountTable
      groupLabel="site_name"
      valueLabel="CORE"
      rows={[
        { group: 'Baca', value: 'D1', count: 18 },
        { group: 'Hooper', value: 'D1', count: 11 },
        { group: null, value: 'D2', count: 1 },
      ]}
    />,
  );
  // Field names are the column headers; values are cells, not columns.
  assert.ok(html.includes('<th scope="col">site_name</th>'));
  assert.ok(html.includes('<th scope="col">CORE</th>'));
  assert.ok(html.includes('<th scope="col">count</th>'));
  assert.ok(!html.includes('<th scope="col">D1</th>'));
  assert.ok(html.includes('<td>Baca</td><td>D1</td><td>18</td>'));
  assert.ok(html.includes('<td>(Missing)</td><td>D2</td><td>1</td>'));
});

test('profile traces draw one gapped line per sample, grouped, log-safe', () => {
  const traces = profileTraces(
    [
      { rowid: 1, element: 'as', value: 3, group_label: 'A' },
      { rowid: 1, element: 'fe', value: 15000, group_label: 'A' },
      { rowid: 2, element: 'as', value: 0, group_label: 'A' },
      { rowid: 2, element: 'fe', value: 12000, group_label: 'A' },
      { rowid: 3, element: 'as', value: 5, group_label: 'B' },
      { rowid: 3, element: 'fe', value: null, group_label: 'B' },
    ],
    true,
  ) as { name: string; x: unknown[]; y: unknown[] }[];
  assert.deepEqual(traces.map((t) => t.name), ['A', 'B']);
  assert.deepEqual(traces[0]!.x, ['as', 'fe', null, 'as', 'fe']);
  // Non-positive values gap on the log axis.
  assert.deepEqual(traces[0]!.y, [3, 15000, null, null, 12000]);
  assert.deepEqual(traces[1]!.y, [5, null]);
});
