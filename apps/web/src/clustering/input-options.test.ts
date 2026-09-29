import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupRowsResponse, TransformationDefinition } from '@archaeodash/client';
import { analysisColumns, projectionLabels, sourceOptions } from './input-options.ts';
const data: GroupRowsResponse = { path: 'g', revision_id: 'r', visible_id_column: 'ID', legacy_rowid_column: 'rowid', elemental_columns: ['a', 'b'], descriptive_columns: ['Group'], rows: ['B', 'A', 'B', null].map(g => ({ analytical_uuid: 'hidden', elemental: [1, 2], descriptive: [g] })) };

test('source requests carry only applicable ordination controls', () => {
  assert.deepEqual(sourceOptions('pca', 3, 'Group', 12), { source: 'pca', pc_count: 3, source_group_column: null, umap_seed: null });
  assert.equal(sourceOptions('lda', 3, 'Group', 12).source_group_column, 'Group');
  assert.equal(sourceOptions('umap', 3, 'Group', 12).umap_seed, 12);
});
test('projection labels exclude missing groups and deduplicate without exposing identities', () => {
  assert.deepEqual(projectionLabels(data, 'Group'), ['A', 'B']);
  assert.deepEqual(projectionLabels(data, 'absent'), []);
});
test('ratio-only input controls use output names and append includes required operands', () => {
  const definition: TransformationDefinition = { name: 'ratios', transform_method: 'none', imputation_method: 'none', elemental_columns: ['a'], descriptive_columns: [], ratios: [{ numerator: 'a', denominator: 'b' }], ratio_mode: 'only' };
  assert.deepEqual(analysisColumns(data, definition), ['a_b']);
  assert.deepEqual(analysisColumns(data, { ...definition, ratio_mode: 'append' }), ['a', 'b', 'a_b']);
});
