import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { DataManager, type DataManagerTransport } from './DataManager.tsx';

test('Data Manager exposes file import and project group validation without internal identifiers', () => {
  const transport = {} as DataManagerTransport;
  const html = renderToStaticMarkup(<DataManager transport={transport} />);
  assert.match(html, /Import data/);
  assert.match(html, /Choose a CSV, TSV, or XLSX file/);
  assert.match(html, /Project groups/);
  assert.doesNotMatch(html, /file_id|group_id|UUID/);
});
