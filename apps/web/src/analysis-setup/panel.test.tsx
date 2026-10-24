import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import type { TransformationSummary } from '@archaeodash/contracts';
import { AnalysisSetupForm, RatioPreviewTable, SavedSetups, ratioPreviewDefinition, savedSummary } from '../data-manager/AnalysisSetupPanel.tsx';
import { namedSetups } from './AnalysisSetupContext.tsx';
import { ANALYSIS_SETUP_NAME, newSetup } from './setup.ts';

const summary = (name: string, extra: Partial<TransformationSummary> = {}): TransformationSummary => ({
  name,
  created_at_unix_secs: 1,
  transform_method: 'none',
  imputation_method: 'none',
  ratio_count: 0,
  ...extra,
});

test('setup form lists ratio variables with the mode and a preview when available', () => {
  const setup = { ...newSetup(['na', 'fe']), ratios: [{ numerator: 'na', denominator: 'fe' }, { numerator: 'k', denominator: 'fe', output_name: 'k_per_fe' }], ratio_mode: 'only' as const };
  const html = renderToStaticMarkup(
    <AnalysisSetupForm elements={['na', 'fe', 'k']} setup={setup} onSave={async () => {}} onCancel={() => {}} onPreview={async () => null} />,
  );
  assert.match(html, /Ratio variables \(2\)/);
  assert.match(html, /<strong>na_fe<\/strong> = na ÷ fe/);
  assert.match(html, /<strong>k_per_fe<\/strong> = k ÷ fe/);
  assert.match(html, /aria-label="Remove k_per_fe"/);
  assert.match(html, /name="setup-ratio-mode" checked="" value="only"/);
  assert.match(html, /Preview values/);
  assert.match(html, /Numerator\(s\)/);
  assert.match(html, /<select[^>]*multiple=""/);
});

test('setup form without ratios or a preview service shows only the builder', () => {
  const html = renderToStaticMarkup(<AnalysisSetupForm elements={['na', 'fe']} setup={null} onSave={async () => {}} onCancel={() => {}} />);
  assert.match(html, /Ratio variables \(0\)/);
  assert.match(html, /name="setup-ratio-mode" checked="" value="append"/);
  assert.doesNotMatch(html, /Preview values/);
  assert.doesNotMatch(html, /Remove /);
});

test('ratio preview shows the first rows of raw ratios, NA for missing', () => {
  const rows = Array.from({ length: 8 }, (_, i) => [i === 1 ? null : 1 / 3 + i]);
  const html = renderToStaticMarkup(<RatioPreviewTable result={{ path: 'g', revision_id: 'r', columns: ['na_fe'], rows, non_finite_to_zero: 0 }} />);
  assert.match(html, /First 5 of 8 rows/);
  assert.match(html, /<th scope="col">na_fe<\/th>/);
  assert.match(html, /<td>0.3333<\/td>/);
  assert.match(html, /<td>NA<\/td>/);
  assert.doesNotMatch(html, /7.333/);
  const definition = ratioPreviewDefinition([{ numerator: 'na', denominator: 'fe' }]);
  assert.equal(definition.ratio_mode, 'only');
  assert.equal(definition.transform_method, 'none');
  assert.equal(definition.imputation_method, 'none');
  assert.deepEqual(definition.elemental_columns, []);
});

test('saved setups list offers Save as, Load and Delete', () => {
  const html = renderToStaticMarkup(
    <SavedSetups
      saved={[summary('Log set', { transform_method: 'log10', ratio_count: 2 }), summary('Imputed', { imputation_method: 'pmm' })]}
      disabled={false}
      onSaveAs={async () => {}}
      onLoad={async () => {}}
      onDelete={async () => {}}
    />,
  );
  assert.match(html, /Saved setups/);
  assert.match(html, /Save as…/);
  assert.match(html, /<strong>Log set<\/strong> <small>2 ratios · Log10<\/small>/);
  assert.match(html, /<strong>Imputed<\/strong> <small>PMM imputation · no transform<\/small>/);
  assert.equal(html.match(/>Load</g)?.length, 2);
  assert.equal(html.match(/>Delete</g)?.length, 2);
  const empty = renderToStaticMarkup(<SavedSetups saved={[]} disabled onSaveAs={async () => {}} onLoad={async () => {}} onDelete={async () => {}} />);
  assert.match(empty, /No saved setups/);
  assert.equal(savedSummary(summary('x', { ratio_count: 1 })), '1 ratio · no transform');
});

test('named setups exclude the reserved active setup and sort by name', () => {
  const names = namedSetups([summary('b'), summary(ANALYSIS_SETUP_NAME), summary('A'), summary('analysis setup')]).map((s) => s.name);
  assert.deepEqual(names, ['A', 'b']);
});
