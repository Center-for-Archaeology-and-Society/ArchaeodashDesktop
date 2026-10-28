import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { ResultGrid } from './ResultGrid.tsx';

const columns = [{ label: 'ID' }, { label: 'Value', digits: 1 }, { label: 'Secret', hiddenByDefault: true }];
const rows = Array.from({ length: 120 }, (_, i) => [`S${i}`, i / 3, `hidden-${i}`]);

test('grid renders sortable headers, filters, the column menu and compact mode, hiding default-hidden columns', () => {
  const html = renderToStaticMarkup(<ResultGrid tableId="t" columns={columns} rows={rows} />);
  assert.match(html, /<button type="button" class="result-grid-sort">ID/);
  assert.match(html, /aria-label="Filter ID"/);
  // Number columns filter by comparison or range, not text.
  assert.match(html, /aria-label="Filter Value: comparison"/);
  assert.match(html, />Between</);
  assert.match(html, /aria-label="Sort by"/);
  assert.match(html, /<option value="all">All<\/option>/);
  assert.match(html, /aria-expanded="false"[^>]*>Columns<\/button>/);
  assert.match(html, /class="data-table result-grid-table is-compact"/);
  assert.match(html, /<td class="num">0.3<\/td>/);
  assert.doesNotMatch(html, /hidden-0|>Secret</);
  assert.match(html, /S99</);
  assert.doesNotMatch(html, /S100</);
  assert.match(html, /Show 100 more rows/);
  assert.doesNotMatch(html, /type="checkbox" aria-label="Select/);
});

test('a saved view shows hidden columns and the checkbox column follows the key set', () => {
  const html = renderToStaticMarkup(<ResultGrid tableId="t" columns={columns} rows={rows.slice(0, 3)} keys={['k0', null, 'k2']}
    selection={{ selected: new Set(['k2', 'stale']), onChange: () => {} }} initialView={{ visibility: { Secret: true, Value: false }, compact: false }} />);
  assert.match(html, /hidden-0/);
  assert.doesNotMatch(html, /aria-label="Filter Value/);
  assert.match(html, /class="data-table result-grid-table"/);
  assert.match(html, /aria-label="Select S0"/);
  assert.doesNotMatch(html, /aria-label="Select S1"/);
  assert.match(html, /aria-label="Select S2" checked=""/);
  assert.match(html, /aria-label="Select all 2 rows"/);
  assert.match(html, /<tr class="is-selected">/);
  assert.doesNotMatch(html, /k0|k2|stale/);
});
