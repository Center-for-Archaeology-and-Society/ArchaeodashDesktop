import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { GroupCandidate, GroupSummary } from '@archaeodash/contracts';
import { DataManager, GroupDetails, GroupFileTree, loadGroupFiles, type DataManagerTransport, type GroupFileEntry } from './DataManager.tsx';
import { ImportWizard } from './ImportWizard.tsx';
import { ImportProgress, elapsedLabel, importStepLabel } from './ImportProgress.tsx';
import { ImportDialog } from './ImportDialog.tsx';
import { AnalysisSetupForm, AnalysisSetupPanel } from './AnalysisSetupPanel.tsx';
import { newSetup } from '../analysis-setup/setup.ts';
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
  for (const label of ['Import…', 'Duplicate', 'Merge', 'Delete']) assert.ok(html.includes(label), `missing ${label}`);
  assert.match(html, /aria-label="Refresh"/);
  assert.match(html, /role="tree"/);
  assert.match(html, /aria-multiselectable="true"/);
  // Nothing selected yet: selection actions are disabled.
  assert.match(html, /<button type="button" class="fm-tool" disabled="" aria-pressed="false">Duplicate<\/button>/);
  assert.match(html, /Select a file to see its details/);
  assert.doesNotMatch(html, /role="dialog"/, 'import dialog is closed initially');
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
