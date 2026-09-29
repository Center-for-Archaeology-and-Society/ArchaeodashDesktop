// Opt-in Section 15.4 procedure 13 browser render benchmark.
// Requires the local Rust API on :8787, Vite preview on :4173, and Playwright.
import assert from 'node:assert/strict';
import os from 'node:os';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');

const base = process.env.MULTIPLOT_BASE_URL ?? 'http://127.0.0.1:4173';
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

const browser = await chromium.launch({
  headless: true,
  ...(process.env.BROWSER_EXECUTABLE ? { executablePath: process.env.BROWSER_EXECUTABLE } : {}),
});
const page = await browser.newPage();
const pageErrors = [];
page.on('pageerror', (error) => pageErrors.push(String(error)));
const startedAt = Date.now();
try {
  await page.goto(`${base}/visualize`);
  await page.getByLabel('Dataset').selectOption(path);
  const timer = await page.evaluate(() => performance.now());
  await page.getByLabel('Plot mode').selectOption('multiplot');
  await page.getByLabel('Render mode').selectOption('interactive');
  await page.waitForFunction(() => {
    const panels = [...document.querySelectorAll('.multiplot-panel-plotly')];
    return panels.length === 12 && panels.every((panel) => panel.dataset.renderState === 'complete');
  }, { timeout: 180_000 });
  const result = await page.locator('.multiplot-panel-plotly').evaluateAll((panels) => ({
    panelCount: panels.length,
    pointCount: panels.reduce((sum, panel) => {
      const graph = panel;
      const plotData = graph.data ?? [];
      return sum + plotData.reduce((traceSum, trace) => traceSum + (trace.x?.length ?? 0), 0);
    }, 0),
    perPanelCounts: panels.map((panel) => (panel.data ?? []).reduce((sum, trace) => sum + (trace.x?.length ?? 0), 0)),
  }));
  const elapsedMs = await page.evaluate((start) => performance.now() - start, timer);
  assert.equal(result.panelCount, 12);
  assert.equal(result.pointCount, 99_960);
  assert.ok(result.perPanelCounts.every((count) => count === 8_330));
  assert.deepEqual(pageErrors, []);
  console.log(JSON.stringify({
    status: 'passed',
    procedure: 13,
    elapsed_ms_including_static_multiplot_mount: Math.round(elapsedMs),
    wall_ms_including_browser_start: Date.now() - startedAt,
    source_rows: 15_000,
    groups: 5,
    facets: result.panelCount,
    rendered_points: result.pointCount,
    per_panel_points: result.perPanelCounts,
    browser: await browser.version(),
    platform: `${os.platform()} ${os.arch()}`,
    node: process.version,
  }, null, 2));
} catch (error) {
  console.error(await page.locator('body').innerText());
  throw error;
} finally {
  await browser.close();
}
