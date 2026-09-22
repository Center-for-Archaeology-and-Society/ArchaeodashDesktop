import assert from 'node:assert/strict';
import { test } from 'node:test';
import { normalizeTheme, themeCssVariables, themes } from './index.ts';

test('the three legacy themes are present with legacy palette values', () => {
  assert.deepEqual(themes.map((t) => t.name), ['light', 'simple', 'dark']);
  const dark = themes.find((t) => t.name === 'dark');
  assert.equal(dark?.background, '#0f1b1e');
  assert.equal(dark?.accent, '#47c997');
});

test('normalizeTheme falls back to simple like the legacy JS loader', () => {
  assert.equal(normalizeTheme('dark'), 'dark');
  assert.equal(normalizeTheme('nope'), 'simple');
  assert.equal(normalizeTheme(undefined), 'simple');
});

test('themeCssVariables emits the legacy custom-property names', () => {
  const light = themes[0];
  assert.ok(light);
  const vars = themeCssVariables(light);
  assert.equal(vars['--bg'], '#eef6f3');
  assert.equal(vars['--side-bg-1'], '#b9ebd4');
  assert.equal(vars['--bg-grad-2'], '#deefe8');
});
