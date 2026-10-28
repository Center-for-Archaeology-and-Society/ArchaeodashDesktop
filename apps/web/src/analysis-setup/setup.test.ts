import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  ANALYSIS_SETUP_NAME,
  analysisInput,
  mergeRatios,
  newSetup,
  ratioCombinations,
  ratioOutputName,
  sanitizeName,
  selectionIndependent,
  setupNameProblem,
  setupStorageKey,
  setupSummary,
  setupTransforms,
  unionElements,
} from './setup.ts';

test('without a setup analyses use every measured column and no transform', () => {
  const input = analysisInput(null, ['na', 'fe', 'k']);
  assert.deepEqual(input.columns, ['na', 'fe', 'k']);
  assert.equal(input.transformation, undefined);
  assert.deepEqual(input.missing, []);
});

test('a column-only setup narrows the columns but sends no transformation', () => {
  const setup = newSetup(['fe', 'na']);
  const input = analysisInput(setup, ['na', 'fe', 'k']);
  // Data order is kept, not the setup's order.
  assert.deepEqual(input.columns, ['na', 'fe']);
  assert.equal(input.transformation, undefined);
  assert.equal(setupTransforms(setup), false);
});

test('a transforming setup is sent with only the columns these groups measure', () => {
  const setup = { ...newSetup(['na', 'fe', 'cs']), transform_method: 'log10' as const };
  const input = analysisInput(setup, ['na', 'fe', 'k']);
  assert.deepEqual(input.columns, ['na', 'fe']);
  assert.deepEqual(input.missing, ['cs']);
  assert.equal(input.transformation?.name, ANALYSIS_SETUP_NAME);
  assert.equal(input.transformation?.transform_method, 'log10');
  assert.deepEqual(input.transformation?.elemental_columns, ['na', 'fe']);
});

test('an imputing setup is sent even without a transform', () => {
  const setup = { ...newSetup(['na', 'fe']), imputation_method: 'rf' as const, imputation_seed: 7 };
  const input = analysisInput(setup, ['na', 'fe']);
  assert.equal(input.transformation?.imputation_method, 'rf');
  assert.equal(input.transformation?.imputation_seed, 7);
});

test('summary and element union read naturally', () => {
  const all = ['na', 'fe', 'k'];
  assert.equal(setupSummary(null, all), 'All 3 elements · no transform');
  assert.equal(setupSummary({ ...newSetup(['na', 'fe']), transform_method: 'log10' }, all), '2 of 3 elements · Log10');
  assert.equal(
    setupSummary({ ...newSetup(['na']), imputation_method: 'pmm', imputation_seed: 1, transform_method: 'log10' }, all),
    '1 of 3 elements · PMM imputation (all groups) · Log10',
  );
  assert.equal(
    setupSummary({ ...newSetup(['na']), imputation_method: 'pmm', imputation_seed: 1, imputation_scope: 'included_groups' }, all),
    '1 of 3 elements · PMM imputation (included groups) · no transform',
  );
  assert.deepEqual(unionElements([['na', 'fe'], ['fe', 'k'], []]), ['na', 'fe', 'k']);
});

test('ratios make the setup transforming and add their outputs to the columns', () => {
  const setup = { ...newSetup(['na', 'fe']), ratios: [{ numerator: 'cs', denominator: 'fe' }] };
  assert.equal(setupTransforms(setup), true);
  const input = analysisInput(setup, ['na', 'fe', 'cs', 'k']);
  // Selected elements plus the ratio output; the unselected source cs stays out.
  assert.deepEqual(input.columns, ['na', 'fe', 'cs_fe']);
  assert.deepEqual(input.elements, ['na', 'fe']);
  assert.deepEqual(input.transformation?.ratios, setup.ratios);
  assert.deepEqual(input.transformation?.elemental_columns, ['na', 'fe']);
  const only = analysisInput({ ...setup, ratio_mode: 'only' }, ['na', 'fe', 'cs']);
  assert.deepEqual(only.columns, ['cs_fe']);
  assert.deepEqual(only.elements, ['na', 'fe']);
});

test('ratios whose sources a group lacks are skipped and reported', () => {
  const setup = { ...newSetup(['na', 'fe']), ratio_mode: 'only' as const, ratios: [{ numerator: 'cs', denominator: 'fe', output_name: 'cs_per_fe' }] };
  const input = analysisInput(setup, ['na', 'fe']);
  // Ratio-only without a usable ratio falls back to the elements, untransformed.
  assert.deepEqual(input.columns, ['na', 'fe']);
  assert.deepEqual(input.missing, ['cs_per_fe']);
  assert.equal(input.transformation, undefined);
});

test('ratio builder pairs every numerator with every denominator', () => {
  assert.deepEqual(ratioCombinations(['na', 'fe'], ['fe', 'k']).map(ratioOutputName), ['na_fe', 'na_k', 'fe_k']);
  // A single pair may be named; names are sanitized like the backend.
  assert.deepEqual(ratioCombinations(['na'], ['fe'], 'Na / Fe'), [{ numerator: 'na', denominator: 'fe', output_name: 'Na___Fe' }]);
  assert.deepEqual(ratioCombinations(['na'], ['na']), []);
  // Re-adding a name replaces the old ratio.
  const merged = mergeRatios([{ numerator: 'na', denominator: 'fe', output_name: 'x' }, { numerator: 'k', denominator: 'fe' }], [{ numerator: 'cs', denominator: 'fe', output_name: 'x' }]);
  assert.deepEqual(merged.map((r) => [ratioOutputName(r), r.numerator]), [['k_fe', 'k'], ['x', 'cs']]);
  assert.equal(setupSummary({ ...newSetup(['na']), ratios: merged, ratio_mode: 'only' }, ['na']), '1 of 1 elements · 2 ratios only · no transform');
});

test('saved setup names sanitize like the backend and cannot take the reserved key', () => {
  assert.equal(sanitizeName('Log set #1'), 'Log_set__1');
  assert.equal(setupStorageKey('x'.repeat(40)).length, 32);
  assert.equal(setupNameProblem('Log set'), '');
  assert.match(setupNameProblem('  '), /Enter a name/);
  assert.match(setupNameProblem('analysis setup'), /reserved/);
  assert.match(setupNameProblem(ANALYSIS_SETUP_NAME), /reserved/);
});

test('saved setup names are checked as they are saved (trimmed)', () => {
  // saveAs stores name.trim(); a padded reserved name must still be refused.
  assert.match(setupNameProblem('analysis_setup '), /reserved/);
  assert.match(setupNameProblem(' analysis_setup'), /reserved/);
  // A padded copy of an existing name must collide, so Replace is asked first.
  assert.equal(setupStorageKey('Log set '), setupStorageKey('Log set'));
});

test('values are selection-independent for per-sample transforms with no or all-groups imputation', () => {
  const base = newSetup(['na']);
  assert.equal(selectionIndependent(null), false);
  assert.equal(selectionIndependent({ ...base, transform_method: 'log10' }), true);
  assert.equal(selectionIndependent({ ...base, imputation_method: 'pmm', imputation_seed: 1 }), true);
  const older = { ...base, imputation_method: 'pmm' as const, imputation_seed: 1 };
  delete (older as { imputation_scope?: unknown }).imputation_scope;
  assert.equal(selectionIndependent(older), true, 'saved before the option: all groups');
  assert.equal(selectionIndependent({ ...base, imputation_method: 'pmm', imputation_seed: 1, imputation_scope: 'included_groups' }), false);
  assert.equal(selectionIndependent({ ...base, transform_method: 'zScore' }), false, 'z-score standardizes over the analyzed rows');
});
