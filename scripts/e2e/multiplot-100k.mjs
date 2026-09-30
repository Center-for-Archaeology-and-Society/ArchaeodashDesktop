// Opt-in Section 15.4 procedure 13 browser render benchmark.
// Requires the local Rust API on :8787, Vite preview on :4173, and Playwright.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import os from 'node:os';
import { parseArgs } from 'node:util';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');

const base = process.env.MULTIPLOT_BASE_URL ?? 'http://127.0.0.1:4173';
const { values } = parseArgs({
  options: {
    output: { type: 'string' },
    'max-elapsed-ms': { type: 'string' },
  },
});
const maxElapsedMs = values['max-elapsed-ms'] === undefined ? undefined : Number(values['max-elapsed-ms']);
if (maxElapsedMs !== undefined && (!Number.isFinite(maxElapsedMs) || maxElapsedMs <= 0)) {
  throw new Error('--max-elapsed-ms must be a positive number');
}
async function api(path, body, method = 'POST') {
  const response = await fetch(`${base}/api/v1/${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const value = await response.json();
  assert.ok(response.ok, `${path}: ${JSON.stringify(value)}`);
  return value;
}

const tag = `multiplot-100k-${Date.now()}`;
const startedAt = Date.now();
const result = {
  schema_version: 1,
  status: 'failed',
  procedure: 13,
  elapsed_ms_including_static_multiplot_mount: null,
  wall_ms_including_browser_start: null,
  source_rows: 15_000,
  groups: 5,
  facets: null,
  rendered_points: null,
  per_panel_points: null,
  browser: null,
  platform: `${os.platform()} ${os.arch()}`,
  node: process.version,
  budgets: maxElapsedMs === undefined
    ? { status: 'not_configured', checks: {} }
    : {
      status: 'unavailable',
      checks: { elapsed_ms: { limit: maxElapsedMs, measured: null, status: 'unavailable' } },
    },
};
let browser;
try {
  const header = 'ANID,Group,a,b,c,d';
  const groups = ['G1', 'G2', 'G3', 'G4', 'G5'];
  const rows = Array.from({ length: 15_000 }, (_, i) => {
    const group = groups[Math.floor(i / 3_000)];
    const x = i + 1;
    return `S${x},${group},${x},${(x * 17) % 10007 + 0.1},${(x * 29) % 7919 + 0.2},${(x * 43) % 6151 + 0.3}`;
  });
  const upload = await fetch(`${base}/api/v1/files?path=${tag}.csv`, {
    method: 'POST',
    headers: { 'content-type': 'text/csv' },
    body: `${header}\n${rows.join('\n')}\n`,
  });
  assert.ok(upload.ok, await upload.text());
  const imported = await api('imports/commit', {
    source: `${tag}.csv`,
    group_column: 'Group',
    visible_id_column: 'ANID',
    elemental_columns: ['a', 'b', 'c', 'd'],
    destination_dir: tag,
  });
  assert.equal(imported.groups.length, 5);
  assert.equal(imported.groups.reduce((sum, group) => sum + group.row_count, 0), 15_000);
  const merged = await api('groups/merge', {
    sources: imported.groups.map((group) => group.path),
    new_group_name: 'Combined',
  });
  const path = merged.outputs[0].path;

  browser = await chromium.launch({
    headless: true,
    ...(process.env.BROWSER_EXECUTABLE ? { executablePath: process.env.BROWSER_EXECUTABLE } : {}),
  });
  const page = await browser.newPage();
  const pageErrors = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));
  await page.goto(`${base}/visualize`);
  await page.getByLabel('Dataset').selectOption(path);
  const timer = await page.evaluate(() => performance.now());
  await page.getByLabel('Plot mode').selectOption('multiplot');
  await page.getByLabel('Render mode').selectOption('interactive');
  await page.waitForFunction(() => {
    const panels = [...document.querySelectorAll('.multiplot-panel-plotly')];
    return panels.length === 12 && panels.every((panel) => panel.dataset.renderState === 'complete');
  }, undefined, { timeout: 180_000 });
  const rendered = await page.locator('.multiplot-panel-plotly').evaluateAll((panels) => ({
    panelCount: panels.length,
    pointCount: panels.reduce((sum, panel) => {
      const graph = panel;
      const plotData = graph.data ?? [];
      return sum + plotData.reduce((traceSum, trace) => traceSum + (trace.x?.length ?? 0), 0);
    }, 0),
    perPanelCounts: panels.map((panel) => (panel.data ?? []).reduce((sum, trace) => sum + (trace.x?.length ?? 0), 0)),
  }));
  const elapsedMs = await page.evaluate((start) => performance.now() - start, timer);
  assert.equal(rendered.panelCount, 12);
  assert.equal(rendered.pointCount, 99_960);
  assert.ok(rendered.perPanelCounts.every((count) => count === 8_330));
  assert.deepEqual(pageErrors, []);
  result.status = 'passed';
  result.elapsed_ms_including_static_multiplot_mount = Math.round(elapsedMs);
  result.facets = rendered.panelCount;
  result.rendered_points = rendered.pointCount;
  result.per_panel_points = rendered.perPanelCounts;
  result.browser = await browser.version();
  if (maxElapsedMs !== undefined) {
    const status = elapsedMs <= maxElapsedMs ? 'passed' : 'failed';
    result.budgets = {
      status,
      checks: {
        elapsed_ms: { limit: maxElapsedMs, measured: elapsedMs, status },
      },
    };
    if (status === 'failed') result.status = 'failed';
  }
} catch (error) {
  result.error = String(error);
} finally {
  try {
    if (browser) await browser.close();
  } catch (error) {
    result.status = 'failed';
    result.error = `${result.error ? `${result.error}; ` : ''}browser close: ${String(error)}`;
  }
  result.wall_ms_including_browser_start = Date.now() - startedAt;
  const json = `${JSON.stringify(result, null, 2)}\n`;
  if (values.output) {
    await mkdir(dirname(values.output), { recursive: true });
    await writeFile(values.output, json, 'utf8');
  }
  process.stdout.write(json);
}
if (result.status !== 'passed') process.exitCode = 1;
