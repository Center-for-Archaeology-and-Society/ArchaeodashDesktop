// Run against the loopback API example and Vite using a disposable project.
// PLAYWRIGHT_MODULE may point to an installed playwright/index.mjs.
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = process.env.PHASE6_BASE_URL ?? 'http://127.0.0.1:5173';
async function api(path, body, method = 'POST') {
  const response = await fetch(`${base}/api/v1/${path}`, { method, headers: { 'content-type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  const value = await response.json();
  assert.ok(response.ok, `${path}: ${JSON.stringify(value)}`);
  return value;
}
const tag = `e2e-${Date.now()}`;
const csv = 'ANID,Group,a,b,c\n' + Array.from({ length: 24 }, (_, i) => `S${i},${i < 8 ? 'A' : i < 16 ? 'B' : 'C'},${i + 1},${i * i % 17 + .3 * i + 1},${i * 7 % 19 + 1}\n`).join('');
const upload = await fetch(`${base}/api/v1/files?path=${tag}.csv`, { method: 'POST', body: csv });
assert.ok(upload.ok, await upload.text());
const imported = await api('imports/commit', { source: `${tag}.csv`, group_column: 'Group', visible_id_column: 'ANID', elemental_columns: ['a', 'b', 'c'], destination_dir: tag });
await api('groups/merge', { sources: imported.groups.map(g => g.path), new_group_name: 'Combined' });
const path = imported.groups[0].path;
const original = await api(`groups/rows?path=${encodeURIComponent(path)}`, undefined, 'GET');
const browser = await chromium.launch({ headless: true, ...(process.env.BROWSER_EXECUTABLE ? { executablePath: process.env.BROWSER_EXECUTABLE } : {}) });
const page = await browser.newPage();
const pageErrors = [];
page.on('pageerror', error => pageErrors.push(String(error)));
const begin = Date.now();
try {
  await page.goto(`${base}/cluster`);
  await page.getByLabel(/^Dataset/).selectOption(path);
  await page.getByLabel('Analysis source').selectOption('pca');
  await page.getByLabel('Principal components').fill('2');
  await page.getByLabel(/^Method/).selectOption('hclust');
  await page.getByLabel(/^Distance/).selectOption('manhattan');
  await page.getByLabel('Linkage').selectOption('average');
  await page.getByRole('button', { name: 'Run analysis', exact: true }).click();
  await page.getByRole('region', { name: 'Analysis results', exact: true }).waitFor();
  assert.match(await page.locator('body').innerText(), /Source: pca; columns: PC1, PC2/);
  assert.match(await page.locator('body').innerText(), /Distance: manhattan; linkage: average/);
  await page.getByLabel('Cut into clusters').fill('3');
  await page.getByRole('button', { name: 'Expand plot view' }).click();
  await page.getByRole('button', { name: 'Reduce plot view' }).click();
  assert.ok(await page.locator('svg').count() > 0);
  await page.getByLabel(/^Method/).selectOption('pam');
  await page.getByLabel(/^Distance/).selectOption('manhattan');
  await page.getByLabel('Diagnostic method').selectOption('pam');
  await page.getByLabel('Diagnostic distance').selectOption('manhattan');
  await page.getByRole('button', { name: 'Run cluster diagnostics' }).click();
  await page.getByRole('region', { name: 'Analysis results', exact: true }).waitFor();
  assert.match(await page.locator('body').innerText(), /Diagnostic method: pam/);
  // Exercise real UMAP source and cancellation. The source computation is ephemeral.
  await page.getByLabel('Analysis source').selectOption('umap');
  await page.getByRole('button', { name: 'Run analysis', exact: true }).click();
  await page.getByRole('button', { name: 'Cancel analysis', exact: true }).click();
  await page.getByText('Analysis cancelled.', { exact: true }).waitFor();
  // Cancelled work must not publish analysis results or change the source.
  assert.equal(await page.getByRole('region', { name: 'Analysis results', exact: true }).count(), 0);
  const after = await api(`groups/rows?path=${encodeURIComponent(path)}`, undefined, 'GET');
  assert.deepEqual(after, original);
  for (const row of original.rows) assert.ok(!(await page.locator('body').innerText()).includes(row.analytical_uuid));
  await page.goto(`${base}/probabilities`);
  await page.getByLabel(/^Dataset/).selectOption(path);
  await page.getByLabel('Analysis source').selectOption('lda');
  await page.getByRole('button', { name: 'Run analysis', exact: true }).click();
  await page.getByRole('region', { name: 'Analysis results', exact: true }).waitFor();
  assert.match(await page.locator('body').innerText(), /Source: lda/);
  await page.getByLabel('Projection groups').selectOption(['B']);
  await page.getByRole('button', { name: 'Run analysis', exact: true }).click();
  await page.getByRole('region', { name: 'Analysis results', exact: true }).waitFor();
  assert.match(await page.locator('body').innerText(), /Projection included/);
  await page.goto(`${base}/euclidean`);
  await page.getByLabel(/^Dataset/).selectOption(path);
  await page.getByLabel('Analysis source').selectOption('umap');
  await page.getByLabel('Projection groups').selectOption(['B']);
  await page.getByRole('button', { name: 'Run analysis', exact: true }).click();
  await page.getByRole('region', { name: 'Analysis results', exact: true }).waitFor();
  assert.match(await page.locator('body').innerText(), /Source: umap/);
  // Record a two-way partition through one reviewed transaction.
  await page.goto(`${base}/cluster`);
  await page.getByLabel(/^Dataset/).selectOption(path);
  await page.getByRole('button', { name: 'Run analysis', exact: true }).click();
  await page.getByRole('region', { name: 'Analysis results', exact: true }).waitFor();
  await page.getByLabel('Color points by').selectOption('groups');
  assert.match(await page.getByRole('figure', { name: 'Partition cluster plot' }).innerText(), /color by existing group/);
  if (process.env.PHASE6_SCREENSHOT) await page.getByRole('figure', { name: 'Partition cluster plot' }).screenshot({ path: process.env.PHASE6_SCREENSHOT });
  await page.getByLabel('Assignment mode').selectOption('automatic');
  await page.getByRole('button', { name: 'Select all 24 analytical units' }).click();
  await page.getByLabel(/^Cluster 1 destination/).selectOption('__new__');
  await page.getByLabel(/^Cluster 2 destination/).selectOption('__new__');
  await page.getByRole('button', { name: 'Review assignments' }).click();
  await page.getByRole('button', { name: 'Confirm assignments', exact: true }).click();
  await page.getByText('Moved 24 analytical units.', { exact: false }).waitFor();
  const first = await api(`groups/rows?path=${encodeURIComponent(`${tag}/Cluster_1.parquet`)}`, undefined, 'GET');
  const second = await api(`groups/rows?path=${encodeURIComponent(`${tag}/Cluster_2.parquet`)}`, undefined, 'GET');
  const order = rows => [...rows].sort((a,b) => a.analytical_uuid.localeCompare(b.analytical_uuid));
  assert.deepEqual(order([...first.rows, ...second.rows]), order(original.rows));
  for (const row of original.rows) assert.ok(!(await page.locator('body').innerText()).includes(row.analytical_uuid));
  assert.deepEqual(pageErrors, []);
  console.log(JSON.stringify({ status: 'passed', elapsed_ms: Date.now() - begin, cases: ['PCA HCA Manhattan Average', 'cut and expanded dendrogram', 'PAM Manhattan diagnostics', 'UMAP cancellation', 'immutable source', 'hidden UUIDs', 'LDA membership and projection groups', 'UMAP nearest matches', 'partition color modes', 'reviewed two-group recording with immutable rows'] }, null, 2));
} catch (error) { console.error(await page.locator('body').innerText()); throw error; } finally { await browser.close(); }
