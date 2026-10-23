/** Explore route unit tests: renderToString over mocked transports. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import type { ExploreService, ExportsService, GroupsService } from '@archaeodash/client';
import { CrosstabCountTable, ExplorePage, DataTable, MissingView, VIRTUALIZE_FROM, profileTraces, type ExploreDeps } from './ExplorePage.tsx';
import { CellAutosave } from './autosave.ts';
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
    <DataTable files={[{ path: rows.path, groupName: 'Baca', data: rows }]} deps={deps} autosave={new CellAutosave(async () => { throw new Error('unused'); }, new Map())} onReload={() => {}} />,
  );
  assert.ok(tableHtml.includes('anid'), 'visible id column header');
  assert.ok(tableHtml.includes('A1'), 'visible id cell');
  assert.ok(!tableHtml.includes('0197aaaa'), 'analytical_uuid never rendered');
  assert.ok(tableHtml.includes('locked-col'), 'elemental columns marked read-only');
  const missingId = { ...rows, rows: rows.rows.map(row => ({ ...row, visible_id: null })) };
  const accessible = renderToString(
    <DataTable files={[{ path: rows.path, groupName: 'Baca', data: missingId }]} deps={deps} autosave={new CellAutosave(async () => { throw new Error('unused'); }, new Map())} onReload={() => {}} />,
  );
  assert.ok(!accessible.includes('0197aaaa'), 'missing visible IDs never expose internal UUIDs in accessible labels');
  assert.ok(accessible.includes('for row 1'));
  // The synthetic group column shows each row's group and is read-only.
  assert.ok(tableHtml.includes('<th>Group</th>'));
  assert.ok(tableHtml.includes('<td class="locked-col">Baca</td>'));
  // Autosave replaces the save button: undo/redo and a status line instead.
  assert.ok(tableHtml.includes('All changes saved'));
  assert.ok(tableHtml.includes('>Undo</button>') && tableHtml.includes('>Redo</button>'));
  assert.ok(!tableHtml.includes('Save descriptive edits'));
});

test('explore missing view renders band table rows from loaded rows, as percentages', () => {
  const row = (elemental: (number | null)[]) => ({ analytical_uuid: 'u', descriptive: [], elemental });
  const html = renderToString(
    <MissingView
      combined={{ visibleIdColumn: 'anid', legacyRowidColumn: 'rowid', descriptiveColumns: ['Group'], elementalColumns: ['Ti', 'Sr'], groupColumn: 'Group', rows: [row([1, null]), row([2, 3])], groupOf: ['A', 'A'], pathOf: ['p', 'p'], groupNames: ['A'] }}
      columns={['Ti', 'Sr']}
    />,
  );
  // Sr first (1 missing of 2 = 50.0%, Bad), then Ti (0%, Good).
  assert.match(html, /<tr data-band="Bad"><td>Sr<\/td><td>1<\/td><td>50.0<\/td><td>Bad<\/td><\/tr><tr data-band="Good"><td>Ti<\/td>/);
});

test('dataset table offers Excel and CSV export', async () => {
  const deps = makeDeps();
  const rows = await deps.groups.rows('groups/Baca.parquet');
  const html = renderToString(
    <DataTable files={[{ path: rows.path, groupName: 'Baca', data: rows }]} deps={deps} autosave={new CellAutosave(async () => { throw new Error('unused'); }, new Map())} onReload={() => {}} />,
  );
  assert.ok(html.includes('Export to Excel'));
  assert.ok(html.includes('Export CSV'));
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

test('large dataset tables draw only the rows in view', async () => {
  const deps = makeDeps();
  const base = await deps.groups.rows('groups/Baca.parquet');
  const template = base.rows[0]!;
  const many = {
    ...base,
    rows: Array.from({ length: 5000 }, (_, i) => ({ ...template, analytical_uuid: `uuid-${i}`, visible_id: `S${i}` })),
  };
  const html = renderToString(
    <DataTable files={[{ path: many.path, groupName: 'Baca', data: many }]} deps={deps} autosave={new CellAutosave(async () => { throw new Error('unused'); }, new Map())} onReload={() => {}} />,
  );
  const drawn = html.match(/data-row="true"/g)?.length ?? 0;
  assert.ok(drawn > 0 && drawn < 200, `only a window of rows is drawn (${drawn})`);
  assert.match(html, /aria-rowcount="5001"/, 'screen readers still get the full row count');
  assert.match(html, /class="table-spacer"/, 'a spacer stands in for the rows below');
  assert.match(html, /5,000 rows; scroll the table to see more/);
  assert.ok(5000 > VIRTUALIZE_FROM);
  // Small tables render every row, as before.
  const small = renderToString(
    <DataTable files={[{ path: base.path, groupName: 'Baca', data: base }]} deps={deps} autosave={new CellAutosave(async () => { throw new Error('unused'); }, new Map())} onReload={() => {}} />,
  );
  assert.equal(small.match(/data-row="true"/g)?.length, base.rows.length);
  assert.doesNotMatch(small, /table-spacer/);
});
