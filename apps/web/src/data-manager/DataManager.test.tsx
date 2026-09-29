import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { DataManager, type DataManagerTransport } from './DataManager.tsx';

test('Data Manager exposes the native source picker on desktop', () => {
  const transport = {} as DataManagerTransport;
  const html = renderToStaticMarkup(<DataManager transport={{ ...transport, kind: 'tauri', files: { pickImportSource: async () => null } } as DataManagerTransport} />);
  assert.match(html, /Import data/);
  assert.match(html, /Choose a source file/);
  assert.doesNotMatch(html, /type="file"/);
  assert.match(html, /Project groups/);
  assert.doesNotMatch(html, /file_id|group_id|UUID/);
});

test('Data Manager keeps the browser file input for HTTP transport', () => {
  const transport = {} as DataManagerTransport;
  const html = renderToStaticMarkup(<DataManager transport={{ ...transport, kind: 'http' } as DataManagerTransport} />);
  assert.match(html, /Choose a CSV, TSV, or XLSX file/);
  assert.match(html, /type="file"/);
  assert.doesNotMatch(html, /Choose a source file/);
});
