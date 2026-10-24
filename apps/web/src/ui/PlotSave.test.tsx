/** Plot-save helpers: file stems, PNG scale, size settings form. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import { DEFAULT_IMAGE_SETTINGS, ImageSettingsForm, MAX_PNG_SIDE, PlotSaveButtons, plotFileStem, pngScale, readImageSettings } from './PlotSave.tsx';

test('plot file stems drop path and control characters', () => {
  assert.equal(plotFileStem('Sr / Ti: "A"'), 'Sr _ Ti_ _A_');
  assert.equal(plotFileStem('   '), 'plot');
});

test('PNG scale follows the chosen dpi and is capped for huge images', () => {
  assert.equal(pngScale(96, 800, 600), 1);
  assert.equal(pngScale(300, 800, 600), 300 / 96);
  assert.equal(pngScale(600, 10000, 600), MAX_PNG_SIDE / 10000);
  assert.equal(pngScale(48, 800, 600), 1, 'never below screen resolution');
});

test('settings default to 300 dpi without storage', () => {
  assert.deepEqual(readImageSettings(), DEFAULT_IMAGE_SETTINGS);
});

test('save buttons offer a size form; size fields only for Plotly charts', () => {
  const html = renderToString(<PlotSaveButtons name="Plot" save={async () => undefined} />);
  assert.ok(html.includes('Save PNG') && html.includes('Size…'));
  const plotly = renderToString(<ImageSettingsForm showSize onClose={() => undefined} />);
  assert.ok(plotly.includes('Width px') && plotly.includes('PNG resolution') && plotly.replace(/<!-- -->/g, '').includes('300 dpi'));
  const svg = renderToString(<ImageSettingsForm showSize={false} onClose={() => undefined} />);
  assert.ok(!svg.includes('Width px') && svg.includes('PNG resolution'));
});
