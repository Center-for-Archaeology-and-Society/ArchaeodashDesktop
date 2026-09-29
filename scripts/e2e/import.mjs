// Reproducible browser smoke for the Data Manager import flow.
// Run with the local API on 127.0.0.1:8787 and Vite on port 4173.
// PLAYWRIGHT_MODULE may point to an installed playwright/index.mjs.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = process.env.IMPORT_BASE_URL ?? 'http://127.0.0.1:4173';
const browser = await chromium.launch({ headless: true, ...(process.env.BROWSER_EXECUTABLE ? { executablePath: process.env.BROWSER_EXECUTABLE } : {}) });
const scratch = await mkdtemp(join(tmpdir(), 'archaeodash-import-e2e-'));
const csvPaths = [1, 2, 3].map(index => join(scratch, `import-smoke-${index}.csv`));
const csv = 'ANID,Context,Group,Ca,Fe\n' + Array.from({ length: 12 }, (_, i) =>
  `S${i + 1},Context ${i + 1},${i < 6 ? 'North' : 'South'},${i + 2},${(i * 3) % 11 + 1}\n`,
).join('');
await Promise.all(csvPaths.map(path => writeFile(path, csv)));

const page = await browser.newPage();
const pageErrors = [];
page.on('pageerror', error => pageErrors.push(String(error)));
const body = () => page.locator('body').innerText();
const started = Date.now();
try {
  await page.goto(`${base}/data`);
  await page.getByRole('heading', { name: 'Data Manager', level: 1 }).waitFor();
  await page.getByLabel(/Choose a CSV, TSV, or XLSX file/).setInputFiles(csvPaths[0]);
  await page.getByText('12 rows; 6 columns', { exact: true }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'Import groups' }).count(), 0, 'import must wait for a grouping mode');

  // The real file upload, preview, selected role controls, and first commit all pass through the UI.
  await page.getByLabel('Group column').selectOption('Group');
  await page.getByText('North: 6 rows', { exact: true }).waitFor();
  await page.getByText('South: 6 rows', { exact: true }).waitFor();
  await page.getByLabel('Visible ID column').selectOption('ANID');
  for (const column of ['Ca', 'Fe']) await page.getByLabel(column, { exact: true }).check();
  await page.getByRole('button', { name: 'Import groups', exact: true }).click();
  await page.getByText('Imported 2 groups (12 rows).', { exact: true }).waitFor();
  assert.ok(await page.getByText('North', { exact: true }).count() > 0);
  assert.ok(await page.getByText('South', { exact: true }).count() > 0);
  assert.doesNotMatch(await body(), /[0-9a-f]{8}-[0-9a-f-]{27,}/i, 'internal UUIDs must not appear in the UI');

  // Re-upload to stage a second source and exercise the one-named-group mode.
  await page.getByLabel(/Choose a CSV, TSV, or XLSX file/).setInputFiles(csvPaths[1]);
  await page.getByText('12 rows; 6 columns', { exact: true }).waitFor();
  await page.getByLabel('Put every row in one named group').check();
  await page.getByLabel('Group name').fill('All Sites');
  await page.getByRole('button', { name: 'Preview single group' }).click();
  await page.getByText('All Sites: 12 rows', { exact: true }).waitFor();
  await page.getByLabel('Visible ID column').selectOption('ANID');
  await page.getByLabel('Ca', { exact: true }).check();
  await page.getByLabel('Fe', { exact: true }).check();
  await page.getByRole('button', { name: 'Import one group', exact: true }).click();
  await page.getByText('Imported 1 group (12 rows).', { exact: true }).waitFor();
  await page.getByText('All Sites', { exact: true }).waitFor();

  // Blank group names are rejected in the UI before preview/commit can run.
  await page.getByLabel(/Choose a CSV, TSV, or XLSX file/).setInputFiles(csvPaths[2]);
  await page.getByText('12 rows; 6 columns', { exact: true }).waitFor();
  await page.getByLabel('Put every row in one named group').check();
  assert.equal(await page.getByRole('button', { name: 'Preview single group' }).isDisabled(), true);
  assert.equal(await page.getByRole('button', { name: 'Import one group' }).count(), 0);

  // Imported groups are available from the actual Cluster page selector.
  await page.goto(`${base}/cluster`);
  const dataset = page.getByLabel(/^Dataset/);
  await dataset.waitFor();
  const options = await dataset.locator('option').allTextContents();
  for (const name of ['North', 'South', 'All_Sites']) assert.ok(options.some(option => option.includes(name)), `${name} missing from Cluster dataset options: ${options.join(', ')}`);
  await dataset.selectOption({ label: options.find(option => option.includes('North')) });
  assert.doesNotMatch(await body(), /[0-9a-f]{8}-[0-9a-f-]{27,}/i, 'internal UUIDs must not appear in Cluster UI');
  assert.deepEqual(pageErrors, []);
  console.log(JSON.stringify({
    status: 'passed',
    elapsed_ms: Date.now() - started,
    cases: [
      'actual CSV file upload and import preview',
      'column-based two-group preview and commit',
      'visible ID and two selected measured columns',
      'single named-group preview and commit',
      'blank group name cannot preview or commit',
      'imported groups available in Cluster dataset selector',
      'no analytical UUIDs rendered',
      'no browser page errors',
    ],
  }, null, 2));
} catch (error) {
  console.error(await body().catch(() => 'Unable to read page body'));
  throw error;
} finally {
  await browser.close();
  await rm(scratch, { recursive: true, force: true });
}
