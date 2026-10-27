import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { GroupCandidate, GroupSummary } from '@archaeodash/contracts';
import { AddColumnForm, DataManager, GroupDetails, GroupFileTree, RENAME_SETTLE_MS, RenameGroupForm, columnNameProblem, exclusionsAfterRename, folderNodes, groupFileStem, groupNameProblem, groupsWithColumn, loadGroupFiles, pathLike, renameGroupFile, renamedEntry, renamedExclusions, withoutStaleExclusions, type DataManagerTransport, type GroupFileEntry } from './DataManager.tsx';
import { RERUN_DELAY_MS } from '../ordination/rerun.ts';
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
  for (const label of ['New folder…', 'Rename folder…', 'Copy to…', 'Move to…']) assert.ok(html.includes(label), `missing ${label}`);
  // Without a group rename in the transport there is no Rename group… button.
  assert.ok(!html.includes('Rename group…'));
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

test('toolbar offers Add column…, Rename folder… and a separate Rename group… when the transport supports them', () => {
  const transport = { kind: 'http', groups: { scan: async () => [], folders: async () => [], copyGroups: async () => { throw new Error('unused'); }, renameFolder: async () => { throw new Error('unused'); }, renameGroup: async () => { throw new Error('unused'); }, addColumn: async () => { throw new Error('unused'); } } } as unknown as DataManagerTransport;
  const html = renderToStaticMarkup(<DataManager transport={transport} projectName="Sample" />);
  assert.match(html, /disabled="" aria-pressed="false" title="Add a descriptive column[^"]*">Add column…<\/button>/);
  assert.match(html, /disabled="" aria-pressed="false" title="Rename the selected folder \(F2\)">Rename folder…<\/button>/);
  assert.match(html, /disabled="" aria-pressed="false" title="Rename the one highlighted group and its file \(F2\)">Rename group…<\/button>/);
});

const siteEntries: GroupFileEntry[] = [
  { path: 'Site/Baca.parquet', name: 'Baca', summary: { ...summary('Baca', 3), path: 'Site/Baca.parquet' }, revision: 'rev-1', error: '' },
  { path: 'Site/Hooper.parquet', name: 'Hooper', summary: { ...summary('Hooper', 2), path: 'Site/Hooper.parquet' }, revision: 'rev-1', error: '' },
  { path: 'Other/Pueblo.parquet', name: 'Pueblo', summary: { ...summary('Pueblo', 2), path: 'Other/Pueblo.parquet' }, revision: 'rev-1', error: '' },
];

test('rename group: a valid new name passes, including a name used in another folder', () => {
  assert.equal(groupNameProblem(siteEntries, 'Site/Baca.parquet', 'Baca Pueblo'), '');
  assert.equal(groupNameProblem(siteEntries, 'Site/Baca.parquet', 'Pueblo'), '', 'other folders do not clash');
  assert.equal(groupNameProblem(siteEntries, 'Site/Baca.parquet', 'BACA'), '', 'a case-only rename of itself');
  assert.equal(groupFileStem('Baca Pueblo'), 'Baca_Pueblo');
});

test('rename group: invalid names are refused', () => {
  assert.match(groupNameProblem(siteEntries, 'Site/Baca.parquet', '   '), /Enter a group name/);
  for (const bad of ['Site/Baca', 'Site\\Baca', 'Baca:1', 'Baca?', 'A<B', 'tab\tname']) {
    assert.match(groupNameProblem(siteEntries, 'Site/Baca.parquet', bad), /cannot contain/, bad);
  }
  assert.match(groupNameProblem(siteEntries, 'Site/Baca.parquet', '.hidden'), /cannot start with a dot/);
  assert.match(groupNameProblem(siteEntries, 'Site/Baca.parquet', 'CON'), /Windows reserves/);
  assert.match(groupNameProblem(siteEntries, 'Site/Baca.parquet', 'x'.repeat(101)), /at most 100/);
  const form = renderToStaticMarkup(<RenameGroupForm current="Baca" name="Site/Baca" problem={groupNameProblem(siteEntries, 'Site/Baca.parquet', 'Site/Baca')} onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />);
  assert.match(form, /aria-label="Rename group"/);
  assert.match(form, /role="alert"/);
  assert.match(form, /<button type="submit" disabled="">/);
});

test('rename group: a name or file clash in the same folder is refused', () => {
  assert.match(groupNameProblem(siteEntries, 'Site/Baca.parquet', 'hooper'), /“Site” already has a group named “Hooper”/);
  // Same group file as another group, though the name differs.
  const withFile: GroupFileEntry[] = [...siteEntries, { path: 'Site\\Baca_Pueblo.parquet', name: 'BP', summary: null, revision: 'rev-1', error: '' }];
  assert.match(groupNameProblem(withFile, 'Site/Baca.parquet', 'Baca Pueblo'), /already has the group file Baca_Pueblo\.parquet \(BP\)/);
  const form = renderToStaticMarkup(<RenameGroupForm current="Baca" name="Hooper" problem={groupNameProblem(siteEntries, 'Site/Baca.parquet', 'Hooper')} onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />);
  assert.match(form, /already has a group named/);
  assert.match(form, /<button type="submit" disabled="">/);
  // A clash the backend finds (another window) rejects the rename.
  const refuse = async () => { throw new Error('folder “Site” already has a group named “Hooper”'); };
  return assert.rejects(renameGroupFile(refuse, { path: 'Site/Baca.parquet', revision: 'rev-1' }, 'Hooper'), /already has a group named/);
});

test('rename group: success sends the request and updates the entry from the answer, without a rescan', async () => {
  const calls: unknown[] = [];
  const rename = async (request: { path: string; expected_revision: string; new_name: string }) => {
    calls.push(request);
    return { transaction_id: 't1', action: 'rename_group', outputs: [{ ...summary('Baca Pueblo', 3), path: 'Site/Baca_Pueblo.parquet', revision_id: 'rev-2' }], deleted_paths: [request.path] };
  };
  const renamed = await renameGroupFile(rename, { path: 'Site\\Baca.parquet', revision: 'rev-1' }, '  Baca Pueblo ');
  assert.deepEqual(calls, [{ path: 'Site\\Baca.parquet', expected_revision: 'rev-1', new_name: 'Baca Pueblo' }]);
  assert.equal(renamed.path, 'Site\\Baca_Pueblo.parquet');
  assert.equal(renamed.summary?.path, 'Site\\Baca_Pueblo.parquet');
  const entry = renamedEntry({ ...siteEntries[0]!, path: 'Site\\Baca.parquet', renaming: true }, renamed.path, 'Baca Pueblo', renamed.summary);
  assert.deepEqual([entry.path, entry.name, entry.revision, entry.error, entry.renaming], ['Site\\Baca_Pueblo.parquet', 'Baca Pueblo', 'rev-2', '', undefined]);
  const form = renderToStaticMarkup(<RenameGroupForm current="Baca" name="Baca Pueblo" problem="" onChange={() => {}} onSubmit={() => {}} onCancel={() => {}} />);
  assert.match(form, /<button type="submit">/);
  assert.doesNotMatch(form, /role="alert"/);
});

test('rename group: the tree shows the new name while it is saved, and back-to-back renames see it', () => {
  const saving: GroupFileEntry[] = [{ ...siteEntries[0]!, name: 'Baca Pueblo', renaming: true }, siteEntries[1]!];
  const html = renderToStaticMarkup(<GroupFileTree entries={saving} selected={[]} />);
  assert.match(html, /aria-label="Baca Pueblo, 3 rows, renaming"/);
  assert.match(html, /Renaming…/);
  // The next rename cannot take the name or the file of one still being saved.
  assert.match(groupNameProblem(saving, 'Site/Hooper.parquet', 'baca pueblo'), /already has a group named “Baca Pueblo”/);
  assert.match(groupNameProblem(saving, 'Site/Hooper.parquet', 'Baca_Pueblo'), /already has the group file Baca_Pueblo\.parquet/);
  // Its old name is free again.
  assert.equal(groupNameProblem(saving, 'Site/Hooper.parquet', 'Baca2'), '');
});

test('rename group: the group keeps its inclusion in analyses', () => {
  const ready = (paths: string[]) => paths;
  // A group left out of analyses stays left out under its new path; the old
  // path stays excluded too until renaming settles (other pages still list it).
  const unchecked = exclusionsAfterRename(['Site/Baca.parquet', 'Site/Hooper.parquet'], 'Site/Baca.parquet', 'Site/New.parquet', new Set());
  assert.deepEqual(unchecked.excluded.sort(), ['Site/Baca.parquet', 'Site/Hooper.parquet', 'Site/New.parquet']);
  assert.deepEqual(unchecked.stale, ['Site/Baca.parquet']);
  assert.deepEqual(includedAmong(ready(['Site/New.parquet', 'Site/Hooper.parquet']), new Set(unchecked.excluded)), []);
  assert.deepEqual(includedAmong(ready(['Site/Baca.parquet', 'Site/Hooper.parquet']), new Set(unchecked.excluded)), [], 'pages not yet rescanned');
  // Once settled the old path is dropped.
  const settled = withoutStaleExclusions(unchecked.excluded, new Set(unchecked.stale), ['Site/New.parquet', 'Site/Hooper.parquet']);
  assert.deepEqual(settled.sort(), ['Site/Hooper.parquet', 'Site/New.parquet']);
  // An included group stays included: its new path is not excluded.
  const checked = exclusionsAfterRename(['Site/Hooper.parquet'], 'Site/Baca.parquet', 'Site/New.parquet', new Set());
  assert.deepEqual(checked.excluded, ['Site/Hooper.parquet']);
  assert.deepEqual(checked.stale, []);
  assert.deepEqual(includedAmong(['Site/New.parquet', 'Site/Hooper.parquet'], new Set(checked.excluded)), ['Site/New.parquet']);
  // Renamed twice before settling (A → B → C): only C stays excluded.
  const first = exclusionsAfterRename(['Site/A.parquet'], 'Site/A.parquet', 'Site/B.parquet', new Set());
  const second = exclusionsAfterRename(first.excluded, 'Site/B.parquet', 'Site/C.parquet', new Set(first.stale));
  assert.deepEqual(withoutStaleExclusions(second.excluded, new Set(second.stale), ['Site/C.parquet']), ['Site/C.parquet']);
  // An included group renamed onto a path a left-out group just gave up stays included.
  const reused = exclusionsAfterRename(first.excluded, 'Site/X.parquet', 'Site/A.parquet', new Set(first.stale));
  assert.deepEqual(reused.excluded, ['Site/B.parquet']);
  assert.deepEqual(withoutStaleExclusions(['Site/A.parquet'], new Set(['Site/A.parquet']), ['Site/A.parquet']), ['Site/A.parquet'], 'a path in use again is kept');
  // A case-only rename keeps the path and changes nothing.
  assert.deepEqual(exclusionsAfterRename(['Site/A.parquet'], 'Site/A.parquet', 'Site/A.parquet', new Set()), { excluded: ['Site/A.parquet'], stale: [] });
});

test('rename group: renaming settles after a pause before pages refresh', () => {
  assert.ok(RENAME_SETTLE_MS > RERUN_DELAY_MS, 'longer than the rerun delay, so a run of renames starts one refresh');
});

test('a renamed group keeps the separators of the scanned path (Windows)', () => {
  // The backend returns the renamed path with '/', but Windows scans list '\\'.
  assert.equal(pathLike('Site\\A.parquet', 'Site/A2.parquet'), 'Site\\A2.parquet');
  assert.equal(pathLike('Site/A.parquet', 'Site/A2.parquet'), 'Site/A2.parquet');
  assert.deepEqual(renamedExclusions(['Site\\A.parquet', 'Site\\B.parquet'], 'Site\\A.parquet', 'Site/A2.parquet'), ['Site\\A2.parquet', 'Site\\B.parquet']);
});
