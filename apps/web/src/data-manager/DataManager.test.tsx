import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { GroupCandidate, GroupSummary } from '@archaeodash/contracts';
import { AddColumnForm, DataManager, GroupDetails, GroupFileTree, columnNameProblem, folderNodes, groupsWithColumn, loadGroupFiles, renamedExclusions, type DataManagerTransport, type GroupFileEntry } from './DataManager.tsx';
import { ImportWizard } from './ImportWizard.tsx';
import { DEFAULT_VALUE_OPTIONS, ImportValueOptions, recipeFromOptions } from './ImportValueOptions.tsx';
import { ImportProgress, elapsedLabel, importStepLabel } from './ImportProgress.tsx';
import { ImportDialog } from './ImportDialog.tsx';
import { AnalysisSetupForm, AnalysisSetupPanel } from './AnalysisSetupPanel.tsx';
import { newSetup } from '../analysis-setup/setup.ts';
import { exclusionFor, includedAmong, invertedAmong, shownByFilter } from '../datasets/GroupInclusion.tsx';
import { applySelection, pruneSelection } from './selection.ts';
import { GROUPS_CHANGED_EVENT, notifyGroupsChanged, onGroupsChanged } from './events.ts';

const summary = (name: string, rows: number): GroupSummary => ({
  path: `groups/${name}.csv`,
  group_id: `id-${name}`,
  group_name: name,
  revision_id: `rev-${name}`,
  row_count: rows,
  source_path: 'sources/INAA.csv',
  elemental_columns: ['Ca', 'Fe', 'Na'],
  descriptive_columns: ['Site'],
});

const entries: GroupFileEntry[] = [
  { path: 'groups/D1.csv', name: 'D1', summary: summary('D1', 12), revision: 'rev-D1', error: '' },
  { path: 'groups/D2.csv', name: 'D2', summary: null, revision: null, error: 'missing analytical_uuid column' },
];

test('Import wizard exposes the native source picker on desktop', () => {
  const transport = {} as DataManagerTransport;
  const html = renderToStaticMarkup(<ImportWizard transport={{ ...transport, kind: 'tauri', files: { pickImportSource: async () => null } } as DataManagerTransport} />);
  assert.match(html, /Choose a source file/);
  assert.doesNotMatch(html, /type="file"/);
  assert.doesNotMatch(html, /file_id|group_id|UUID/);
});

test('Import wizard keeps the browser file input for HTTP transport', () => {
  const transport = {} as DataManagerTransport;
  const html = renderToStaticMarkup(<ImportWizard transport={{ ...transport, kind: 'http' } as DataManagerTransport} />);
  assert.match(html, /Choose a CSV, TSV, or XLSX file/);
  assert.match(html, /type="file"/);
  assert.doesNotMatch(html, /Choose a source file/);
});

test('import dialog is an accessible modal', () => {
  const html = renderToStaticMarkup(<ImportDialog title="Import data" onClose={() => {}}><p>body</p></ImportDialog>);
  assert.match(html, /role="dialog"/);
  assert.match(html, /aria-modal="true"/);
  assert.match(html, /aria-labelledby="[^"]+"/);
  assert.match(html, /Import data/);
  assert.match(html, /aria-label="Close"/);
});

test('file manager sidebar renders the header, toolbar and groups folder', () => {
  const transport = { kind: 'http', groups: { scan: async () => [] } } as unknown as DataManagerTransport;
  const html = renderToStaticMarkup(<DataManager transport={transport} projectName="Sample" />);
  assert.match(html, /Sample/);
  assert.match(html, /role="toolbar"/);
  for (const label of ['Import…', 'Merge', 'Delete']) assert.ok(html.includes(label), `missing ${label}`);
  // Duplicating inside a folder would repeat samples; Copy to… replaces it.
  assert.doesNotMatch(html, />Duplicate</);
  assert.match(html, /aria-label="Refresh"/);
  assert.match(html, /role="tree"/);
  assert.match(html, /aria-multiselectable="true"/);
  // Nothing selected yet: selection actions are disabled.
  assert.match(html, /<button type="button" class="fm-tool" disabled="" aria-pressed="false">Merge<\/button>/);
  assert.match(html, /Select a file to see its details/);
  assert.doesNotMatch(html, /role="dialog"/, 'import dialog is closed initially');
  // No export service in this transport: no Export button.
  assert.doesNotMatch(html, /Export…/);
});

test('file manager offers Export… when the transport can export', () => {
  const transport = { kind: 'http', groups: { scan: async () => [] }, exports: { measuredData: async () => { throw new Error('unused'); } } } as unknown as DataManagerTransport;
  const html = renderToStaticMarkup(<DataManager transport={transport} projectName="Sample" />);
  // Nothing loaded yet, so there is nothing to export.
  assert.match(html, /<button type="button" class="fm-tool" disabled="" aria-pressed="false" title="[^"]*Excel or CSV">Export…<\/button>/);
});

test('group file tree renders one treeitem per file with rows, selection and validation warnings', () => {
  const html = renderToStaticMarkup(<GroupFileTree entries={entries} selected={['groups/D1.csv']} focusId="groups/D1.csv" />);
  assert.match(html, /role="treeitem" aria-expanded="true" aria-level="1"/);
  assert.match(html, /role="group"/);
  assert.equal(html.match(/aria-level="2"/g)?.length, 2);
  assert.match(html, /aria-selected="true"[^>]*tabindex="0"/);
  assert.match(html, /aria-selected="false"[^>]*aria-invalid="true"[^>]*tabindex="-1"/);
  assert.match(html, />D1</);
  assert.match(html, /12 rows/);
  assert.match(html, /title="Needs attention: missing analytical_uuid column"/);
  assert.match(html, /aria-label="Needs attention: missing analytical_uuid column"/);
  assert.doesNotMatch(html, /group_id|rev-D1/);
});

test('collapsed groups folder hides its rows', () => {
  const html = renderToStaticMarkup(<GroupFileTree entries={entries} selected={[]} folderOpen={false} />);
  assert.match(html, /aria-expanded="false"/);
  assert.doesNotMatch(html, /aria-level="2"/);
});

test('details panel summarizes a single file or a multi-selection', () => {
  const one = renderToStaticMarkup(<GroupDetails entries={[entries[0]!]} />);
  assert.match(one, /groups\/D1.csv/);
  assert.match(one, /3 columns/);
  assert.match(one, />1 column</);
  assert.match(one, /INAA.csv/);
  const many = renderToStaticMarkup(<GroupDetails entries={entries} />);
  assert.match(many, /2 files selected/);
});

test('loadGroupFiles validates each candidate and keeps failures with their error', async () => {
  const candidates: GroupCandidate[] = [
    { path: 'groups/D1.csv', ready: true, group: summary('D1', 12) },
    { path: 'groups/broken.csv', ready: false, group: null },
  ];
  const groups = {
    scan: async () => candidates,
    validate: async (path: string) => {
      if (path.includes('broken')) throw new Error('bad header');
      return summary('D1', 12);
    },
  } as unknown as DataManagerTransport['groups'];
  const loaded = await loadGroupFiles(groups);
  assert.deepEqual(loaded.map((entry) => [entry.name, entry.revision, entry.error]), [
    ['D1', 'rev-D1', ''],
    ['broken', null, 'bad header'],
  ]);
});

test('selection: click selects one, toggle adds/removes, shift selects a range', () => {
  const order = ['a', 'b', 'c', 'd'];
  let state = applySelection(order, { selected: [], anchor: null }, 'b');
  assert.deepEqual(state, { selected: ['b'], anchor: 'b' });
  state = applySelection(order, state, 'd', { toggle: true });
  assert.deepEqual(state.selected, ['b', 'd']);
  state = applySelection(order, state, 'b', { toggle: true });
  assert.deepEqual(state.selected, ['d']);
  // Toggling moves the anchor; a plain click resets it.
  state = applySelection(order, state, 'd');
  state = applySelection(order, state, 'a', { range: true });
  assert.deepEqual(state.selected, ['a', 'b', 'c', 'd']);
  assert.equal(state.anchor, 'd');
  assert.deepEqual(applySelection(order, state, 'b', { range: true }).selected, ['b', 'c', 'd']);
  assert.deepEqual(pruneSelection(['a', 'c'], state), { selected: ['a', 'c'], anchor: null });
});

test('groups-changed event helpers dispatch on window', () => {
  const target = new EventTarget();
  const previous = (globalThis as { window?: unknown }).window;
  (globalThis as { window?: unknown }).window = target;
  try {
    let calls = 0;
    const off = onGroupsChanged(() => { calls += 1; });
    notifyGroupsChanged();
    target.dispatchEvent(new Event(GROUPS_CHANGED_EVENT));
    off();
    notifyGroupsChanged();
    assert.equal(calls, 2);
    assert.equal(GROUPS_CHANGED_EVENT, 'archaeodash:groups-changed');
  } finally {
    (globalThis as { window?: unknown }).window = previous;
  }
});

test('import progress shows an indeterminate bar with the current step', () => {
  const html = renderToStaticMarkup(<ImportProgress step="importing" startedAt={Date.now()} groups={3} />);
  assert.match(html, /role="progressbar"/);
  assert.match(html, /aria-valuetext="Writing 3 group files…"/);
  assert.match(html, /Writing 3 group files…/);
  assert.match(html, /0 s/);
});

test('import step labels and elapsed time read naturally', () => {
  assert.equal(importStepLabel('importing', 1), 'Writing 1 group file…');
  assert.equal(importStepLabel('importing'), 'Writing group files…');
  assert.equal(importStepLabel('reading'), 'Reading columns and rows…');
  assert.equal(elapsedLabel(0, 4_900), '4 s');
  assert.equal(elapsedLabel(0, 125_000), '2 min 5 s');
  assert.equal(elapsedLabel(10_000, 0), '0 s');
});

test('import wizard shows no Close button before an import finishes', () => {
  const transport = {} as DataManagerTransport;
  const html = renderToStaticMarkup(<ImportWizard transport={{ ...transport, kind: 'http' } as DataManagerTransport} onClose={() => {}} />);
  assert.doesNotMatch(html, />Close</);
  assert.doesNotMatch(html, /role="progressbar"/);
});

test('analysis setup form lists elements and transforms, preselecting the saved setup', () => {
  const setup = { ...newSetup(['fe']), transform_method: 'log10' as const };
  const html = renderToStaticMarkup(<AnalysisSetupForm elements={['na', 'fe']} setup={setup} onSave={async () => {}} onCancel={() => {}} />);
  assert.match(html, /Elements used in analyses and plots \(1 of 2\)/);
  assert.match(html, /type="checkbox" checked=""[^>]*\/> fe/);
  assert.doesNotMatch(html, /type="checkbox" checked=""[^>]*\/> na/);
  assert.match(html, /checked="" value="log10"/);
  assert.match(html, /Zeros, negative and missing values have no logarithm/);
  assert.match(html, /name="setup-imputation" checked="" value="none"/);
  assert.match(html, /Weighted predictive mean matching/);
  assert.doesNotMatch(html, /Seed/);
});

test('analysis setup form shows the replayable seed when imputing', () => {
  const setup = { ...newSetup(['fe']), imputation_method: 'pmm' as const, imputation_seed: 42 };
  const html = renderToStaticMarkup(<AnalysisSetupForm elements={['fe']} setup={setup} onSave={async () => {}} onCancel={() => {}} />);
  assert.match(html, /name="setup-imputation" checked="" value="pmm"/);
  assert.match(html, /type="number"[^>]*value="42"/);
  assert.match(html, /same seed gives the same imputed values/);
});

test('analysis setup form offers background precompute of PCA, UMAP, and LDA', () => {
  const setup = { ...newSetup(['fe']), precompute: ['umap' as const] };
  const html = renderToStaticMarkup(<AnalysisSetupForm elements={['fe']} setup={setup} onSave={async () => {}} onCancel={() => {}} />);
  assert.match(html, /Precompute in the background/);
  assert.match(html, /type="checkbox"\/> PCA/);
  assert.match(html, /type="checkbox" checked=""\/> UMAP/);
  assert.match(html, /LDA needs three or more groups/);
});

test('analysis setup panel stays hidden without a setup provider', () => {
  const html = renderToStaticMarkup(<AnalysisSetupPanel elements={['na']} />);
  assert.equal(html, '');
});

test('inclusion helpers keep project order and default new groups to included', () => {
  const ready = ['g/A', 'g/B', 'g/C'];
  assert.deepEqual(includedAmong(ready, new Set()), ready);
  assert.deepEqual(includedAmong(ready, new Set(['g/B'])), ['g/A', 'g/C']);
  assert.deepEqual(exclusionFor(ready, ['g/C', 'g/A']), ['g/B']);
  // A group imported later is not in the excluded set, so it is included.
  assert.deepEqual(includedAmong([...ready, 'g/D'], new Set(exclusionFor(ready, ['g/A']))), ['g/A', 'g/D']);
});

test('group file tree shows inclusion boxes separately from the selection highlight', () => {
  const html = renderToStaticMarkup(
    <GroupFileTree entries={entries} selected={['groups/D1.csv']} included={new Set<string>()} focusId="groups/D1.csv" />,
  );
  // D1 is highlighted (selected) but not included; D2 needs attention and cannot be included.
  assert.match(html, /aria-selected="true" aria-checked="false"/);
  assert.match(html, /is-selected is-excluded/);
  assert.match(html, /class="fm-check fm-include is-disabled"/);
  assert.match(html, /title="Include in analyses \(double-click: only this group\)"/);
  const on = renderToStaticMarkup(<GroupFileTree entries={entries} selected={[]} included={new Set(['groups/D1.csv'])} />);
  assert.match(on, /aria-checked="true"/);
  assert.match(on, /class="fm-check fm-include is-on"[^>]*>✓/);
  assert.doesNotMatch(on, /is-excluded/);
});

test('import value handling defaults match the original ArchaeoDash loader', () => {
  const html = renderToStaticMarkup(<ImportValueOptions value={DEFAULT_VALUE_OPTIONS} onChange={() => {}} />);
  assert.match(html, /<legend>Value handling<\/legend>/);
  assert.match(html, /checked=""\/> Replace empty\/NA non-element fields with \[blank\]/);
  assert.match(html, /checked=""\/> Treat zero values as NA/);
  assert.match(html, /checked=""\/> Treat negative values as NA/);
  assert.match(html, /type="checkbox"\/> Replace NA values with 0/);
  assert.deepEqual(recipeFromOptions(DEFAULT_VALUE_OPTIONS), {
    zero_as_na: true,
    negative_as_na: true,
    na_as_zero: false,
    blank_non_element_label: '[blank]',
  });
  assert.equal(recipeFromOptions({ ...DEFAULT_VALUE_OPTIONS, blankNonElement: false }).blank_non_element_label, null);
});

test('file manager shows folder actions when the transport supports folders', () => {
  const transport = {
    kind: 'tauri',
    groups: { scan: async () => [], folders: async () => [], createFolder: async () => ({ path: 'x', name: 'x', group_count: 0 }), renameFolder: async () => ({ path: 'x', name: 'x', group_count: 0 }), deleteFolder: async () => {}, copyGroups: async () => { throw new Error('unused'); } },
  } as unknown as DataManagerTransport;
  const html = renderToStaticMarkup(<DataManager transport={transport} projectName="Sample" />);
  for (const label of ['New folder…', 'Rename…', 'Copy to…', 'Move to…']) assert.ok(html.includes(label), `missing ${label}`);
});

test('the tree shows one row per folder with its groups, and the analysis-folder switch', () => {
  const many: GroupFileEntry[] = [
    { path: 'Site/A.parquet', name: 'A', summary: summary('A', 3), revision: 'r', error: '' },
    { path: String.raw`Clusters\K1.parquet`, name: 'K1', summary: summary('K1', 2), revision: 'r', error: '' },
  ];
  const folders = folderNodes(many, [{ path: 'Empty', name: 'Empty', group_count: 0 }]);
  assert.deepEqual(folders.map((f) => [f.path, f.entries.map((e) => e.name)]), [['Clusters', ['K1']], ['Empty', []], ['Site', ['A']]]);
  const html = renderToStaticMarkup(<GroupFileTree entries={many} folders={folders} selected={[]} activeFolder="Site" included={new Set(['Site/A.parquet'])} />);
  assert.equal(html.match(/aria-level="1"/g)?.length, 3);
  assert.match(html, /aria-label="Site, 1 file, used in analyses"/);
  assert.match(html, /In analyses/);
  assert.match(html, /Empty folder/);
  // A group in another folder is not counted as included.
  assert.equal(html.match(/fm-include is-on/g)?.length, 1);
  assert.match(html, /fm-include is-other-folder/);
});

test('inclusion helpers invert and filter the tree', () => {
  const included = new Set(['Site/A.parquet']);
  assert.deepEqual(invertedAmong(['Site/A.parquet', 'Site/B.parquet'], included), ['Site/B.parquet']);
  assert.equal(shownByFilter('Site/A.parquet', 'included', included, true), true);
  assert.equal(shownByFilter('Site/B.parquet', 'included', included, true), false);
  assert.equal(shownByFilter('Site/B.parquet', 'excluded', included, true), true);
  assert.equal(shownByFilter('Other/C.parquet', 'excluded', included, false), false, 'other folders only under all');
  assert.equal(shownByFilter('Other/C.parquet', 'all', included, false), true);
  const html = renderToStaticMarkup(<GroupFileTree entries={[]} folders={[{ path: 'Site', name: 'Site', entries: [] }]} selected={[]} filtered />);
  assert.match(html, /No groups match the filter/);
});

test('add-column helpers find existing and refused column names', () => {
  const folderEntries: GroupFileEntry[] = [
    { path: 'Site/A.parquet', name: 'A', summary: { ...summary('A', 3), path: 'Site/A.parquet', descriptive_columns: ['Site', 'Phase'] }, revision: 'rev-1', error: '' },
    { path: 'Site/B.parquet', name: 'B', summary: { ...summary('B', 2), path: 'Site/B.parquet' }, revision: 'rev-1', error: '' },
  ];
  assert.deepEqual(groupsWithColumn(folderEntries, 'Site', ' Phase ').map((e) => e.name), ['A']);
  assert.equal(groupsWithColumn(folderEntries, 'Other', 'Phase').length, 0);
  assert.match(columnNameProblem(folderEntries, 'Site', 'Fe'), /measured column in A/);
  assert.match(columnNameProblem(folderEntries, 'Site', 'analytical_uuid'), /reserved/);
  assert.match(columnNameProblem(folderEntries, 'Site', ' '), /Enter a column name/);
  assert.equal(columnNameProblem(folderEntries, 'Site', 'Ware'), '');
  assert.deepEqual(renamedExclusions(['Site/A.parquet', 'Site/B.parquet'], 'Site/A.parquet', 'Site/A2.parquet'), ['Site/A2.parquet', 'Site/B.parquet']);
});

test('add-column form asks a second time before overwriting', () => {
  const noop = () => {};
  const first = renderToStaticMarkup(<AddColumnForm pending={{ folder: 'Site', column: 'Phase', value: 'Late', confirmOverwrite: false }} problem="" existing={['A']} groupCount={2} onChange={noop} onSubmit={noop} onBack={noop} onCancel={noop} />);
  assert.match(first, /aria-label="Add column"/);
  assert.match(first, /you will be asked before its values are replaced/);
  assert.doesNotMatch(first, /Replace values/);
  const confirm = renderToStaticMarkup(<AddColumnForm pending={{ folder: 'Site', column: 'Phase', value: 'Late', confirmOverwrite: true }} problem="" existing={['A']} groupCount={2} onChange={noop} onSubmit={noop} onBack={noop} onCancel={noop} />);
  assert.match(confirm, /role="alert"/);
  assert.match(confirm, /already exists in A/);
  assert.match(confirm, /Replace values/);
  const refused = renderToStaticMarkup(<AddColumnForm pending={{ folder: 'Site', column: 'Fe', value: '', confirmOverwrite: false }} problem="“Fe” is a measured column" existing={[]} groupCount={2} onChange={noop} onSubmit={noop} onBack={noop} onCancel={noop} />);
  assert.match(refused, /<button type="submit" disabled="">/);
});

test('toolbar offers Add column… and Rename… when the transport supports them', () => {
  const transport = { kind: 'http', groups: { scan: async () => [], folders: async () => [], copyGroups: async () => { throw new Error('unused'); }, renameFolder: async () => { throw new Error('unused'); }, renameGroup: async () => { throw new Error('unused'); }, addColumn: async () => { throw new Error('unused'); } } } as unknown as DataManagerTransport;
  const html = renderToStaticMarkup(<DataManager transport={transport} projectName="Sample" />);
  assert.match(html, /disabled="" aria-pressed="false" title="Add a descriptive column[^"]*">Add column…<\/button>/);
  assert.match(html, /title="Rename the selected folder or the one highlighted group \(F2\)">Rename…<\/button>/);
});
