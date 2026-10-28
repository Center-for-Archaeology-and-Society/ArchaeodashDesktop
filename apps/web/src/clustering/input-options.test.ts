import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupRowsResponse, TransformationDefinition } from '@archaeodash/client';
import {
  analysisColumns, defaultIdColumn, groupEligibility, pcCountOptions, projectionLabels, sourceFeatureCount, sourceOptions, uniqueIdColumns,
} from './input-options.ts';
const data: GroupRowsResponse = { path: 'g', revision_id: 'r', visible_id_column: 'ID', legacy_rowid_column: 'rowid', elemental_columns: ['a', 'b'], descriptive_columns: ['Group'], rows: ['B', 'A', 'B', null].map(g => ({ analytical_uuid: 'hidden', elemental: [1, 2], descriptive: [g] })) };

test('source requests carry only applicable ordination controls', () => {
  assert.deepEqual(sourceOptions('pca', 3, 'Group', 12), { source: 'pca', pc_count: 3, source_group_column: null, umap_seed: null });
  assert.equal(sourceOptions('lda', 3, 'Group', 12).source_group_column, 'Group');
  assert.equal(sourceOptions('umap', 3, 'Group', 12).umap_seed, 12);
  assert.equal(sourceOptions('umap', 5, 'Group', 12).pc_count, 5, 'UMAP dimension count');
  assert.equal(sourceOptions('elements', 5, 'Group', 12).pc_count, null);
});
test('projection labels exclude missing groups and deduplicate without exposing identities', () => {
  assert.deepEqual(projectionLabels(data, 'Group'), ['A', 'B']);
  assert.deepEqual(projectionLabels(data, 'absent'), []);
});
test('ratio-only input controls use output names and append includes required operands', () => {
  const definition: TransformationDefinition = { name: 'ratios', transform_method: 'none', imputation_method: 'none', elemental_columns: ['a'], descriptive_columns: [], ratios: [{ numerator: 'a', denominator: 'b' }], ratio_mode: 'only' };
  assert.deepEqual(analysisColumns(data, definition), ['a_b']);
  assert.deepEqual(analysisColumns(data, { ...definition, ratio_mode: 'append' }), ['a', 'a_b']);
});

test('ID choices keep only columns with a unique value per row, falling back to the visible ID', () => {
  const rows = (ids: (string | null)[], other: (string | null)[]) => ids.map((id, i) => ({ analytical_uuid: 'h', visible_id: id, elemental: [], descriptive: [other[i] ?? null, String(i)] }));
  const base = { ...data, descriptive_columns: ['Site', 'anid'] };
  assert.deepEqual(uniqueIdColumns({ ...base, rows: rows(['a', 'b', 'c'], ['x', 'x', 'y']) }), ['ID', 'anid']);
  const dup = { ...base, rows: rows(['a', 'a', null], ['x', 'y', 'z']) };
  assert.deepEqual(uniqueIdColumns(dup), ['Site', 'anid']);
  assert.equal(defaultIdColumn(dup, uniqueIdColumns(dup)), 'anid');
  const none = { ...data, rows: rows(['a', 'a'], ['x', 'x']).map(r => ({ ...r, descriptive: ['x'] })) };
  assert.deepEqual(uniqueIdColumns(none), ['ID']);
});

test('group eligibility follows getEligible with blank cells counting as a level', () => {
  const labels = [...Array<string>(7).fill('A'), ...Array<string>(6).fill('B'), 'C', null];
  const rows = { ...data, rows: labels.map(g => ({ analytical_uuid: 'h', elemental: [], descriptive: [g] })) };
  // Four levels (A, B, C, blank), 2 features: eligible when n > max(2, 4) + 1 = 5.
  assert.deepEqual(groupEligibility(rows, 'Group', 2), [
    { group: 'A', count: 7, eligible: true }, { group: 'B', count: 6, eligible: true }, { group: 'C', count: 1, eligible: false },
  ]);
  assert.deepEqual(groupEligibility(rows, 'Group', 6).map(g => g.eligible), [false, false, false]);
  assert.deepEqual(groupEligibility(rows, 'Group', 5).map(g => g.eligible), [true, false, false]);
  assert.deepEqual(groupEligibility(rows, 'absent', 2), []);
  assert.equal(sourceFeatureCount('elements', 7, 3, 4), 7);
  assert.equal(sourceFeatureCount('pca', 7, 3, 4), 3);
  assert.equal(sourceFeatureCount('umap', 7, 3, 4), 3);
  assert.equal(sourceFeatureCount('lda', 7, 3, 4), 3);
});

test('PC-count choices read First n PCs with variance and cumulative variance', () => {
  assert.deepEqual(pcCountOptions(2, [0.6234, 0.3], [0.6234, 0.9234]), [
    { value: 1, label: 'First 1 PCs (62.3%/62.3%)' }, { value: 2, label: 'First 2 PCs (30.0%/92.3%)' },
  ]);
  assert.deepEqual(pcCountOptions(2).map(o => o.label), ['First 1 PCs', 'First 2 PCs']);
});
