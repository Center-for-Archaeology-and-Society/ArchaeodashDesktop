import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ANALYSIS_SETUP_NAME, analysisInput, newSetup, setupSummary, setupTransforms, unionElements } from './setup.ts';

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

test('summary and element union read naturally', () => {
  const all = ['na', 'fe', 'k'];
  assert.equal(setupSummary(null, all), 'All 3 elements · no transform');
  assert.equal(setupSummary({ ...newSetup(['na', 'fe']), transform_method: 'log10' }, all), '2 of 3 elements · Log10');
  assert.deepEqual(unionElements([['na', 'fe'], ['fe', 'k'], []]), ['na', 'fe', 'k']);
});
