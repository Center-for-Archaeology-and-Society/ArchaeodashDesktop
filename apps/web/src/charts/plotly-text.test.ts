import assert from 'node:assert/strict';
import { test } from 'node:test';
import { plotlyText } from './plotly-text.ts';

test('user text cannot inject Plotly pseudo-HTML or hovertemplate fields', () => {
  assert.equal(plotlyText('<a href="https://x.test">S1</a>'), '&lt;a href="https://x.test"&gt;S1&lt;/a&gt;');
  assert.equal(plotlyText('A<br>B'), 'A&lt;br&gt;B');
  assert.equal(plotlyText('50%{x}'), '50&#37;{x}');
  // An entity typed by the user stays literal text.
  assert.equal(plotlyText('&lt;LOD'), '&amp;lt;LOD');
  assert.equal(plotlyText('Baca jar'), 'Baca jar');
});
