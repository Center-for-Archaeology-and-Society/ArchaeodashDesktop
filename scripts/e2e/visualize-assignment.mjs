// Real-browser regression for Visualize & Assign's Plotly lasso and persisted move.
// Requires the local API on :8787, Vite preview on :4173, and Playwright.
import assert from 'node:assert/strict';
import { chromium } from 'playwright';

const base = process.env.VISUALIZE_ASSIGNMENT_BASE_URL ?? 'http://127.0.0.1:4173';
const tag = `visualize-assignment-${Date.now()}`;

async function api(path, body, method = 'POST') {
  const response = await fetch(`${base}/api/v1/${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await response.text();
  let value;
  try {
    value = JSON.parse(text);
  } catch {
    value = text;
  }
  assert.ok(response.ok, `${path}: ${JSON.stringify(value)}`);
  return value;
}

async function chartRows(page) {
  return page.locator('.visualize-scatter').evaluate((graph) =>
    (graph.data ?? []).reduce((sum, trace) => sum + (trace.x?.length ?? 0), 0),
  );
}

async function chartUuids(page) {
  return page.locator('.visualize-scatter').evaluate((graph) =>
    (graph.data ?? []).flatMap((trace) =>
      (trace.customdata ?? []).map((row) => String(row?.[0] ?? '')),
    ),
  );
}

const csv = [
  'ANID,Group,a,b',
  'S_MOVE,Source,5,5',
  'S_KEEP_A,Source,1,9',
  'S_KEEP_B,Source,9,1',
  'T_KEEP_A,Target,1,1',
  'T_KEEP_B,Target,9,9',
].join('\n') + '\n';
const uploaded = await fetch(`${base}/api/v1/files?path=${tag}.csv`, {
  method: 'POST',
  headers: { 'content-type': 'text/csv' },
  body: csv,
});
assert.ok(uploaded.ok, await uploaded.text());
const imported = await api('imports/commit', {
  source: `${tag}.csv`,
  group_column: 'Group',
  visible_id_column: 'ANID',
  elemental_columns: ['a', 'b'],
  destination_dir: tag,
});
assert.equal(imported.groups.length, 2);
const source = imported.groups.find((group) => group.group_name === 'Source');
const target = imported.groups.find((group) => group.group_name === 'Target');
assert.ok(source?.path);
assert.ok(target?.path);

const sourceBefore = await api(`groups/rows?path=${encodeURIComponent(source.path)}`, undefined, 'GET');
const targetBefore = await api(`groups/rows?path=${encodeURIComponent(target.path)}`, undefined, 'GET');
const movedBefore = sourceBefore.rows.find((row) => row.visible_id === 'S_MOVE');
assert.ok(movedBefore, 'fixture contains the known source point');

const browser = await chromium.launch({
  headless: true,
  ...(process.env.BROWSER_EXECUTABLE ? { executablePath: process.env.BROWSER_EXECUTABLE } : {}),
});
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const pageErrors = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));
  await page.goto(`${base}/visualize`);
  await page.getByLabel('Dataset').selectOption(source.path);

  const holder = page.locator('.visualize-scatter');
  await page.waitForFunction(() => {
    const graph = document.querySelector('.visualize-scatter');
    return graph?.dataset.renderState === 'complete' && graph.querySelector('.main-svg');
  });
  const holderBox = await holder.boundingBox();
  assert.ok(holderBox, 'Plotly holder has a visible layout box');
  assert.ok(holderBox.height >= 400, `expected a stable scatter height, received ${holderBox.height}px`);
  assert.equal(await chartRows(page), sourceBefore.rows.length);
  const assignmentControl = await page.getByLabel('Target group').boundingBox();
  assert.ok(assignmentControl, 'target-group control is visible');
  assert.ok(
    assignmentControl.y >= holderBox.y + holderBox.height,
    'assignment controls begin below the plot holder',
  );

  // Read only Plotly's coordinate transforms to place the real pointer gesture
  // over the fixture's known point. Selection itself must come from mouse input.
  const point = await holder.evaluate((graph, uuid) => {
    const pointIndex = graph.data.findIndex((trace) =>
      (trace.customdata ?? []).some((row) => row?.[0] === uuid),
    );
    const trace = graph.data[pointIndex];
    const index = trace.customdata.findIndex((row) => row?.[0] === uuid);
    const fullLayout = graph._fullLayout;
    const xaxis = fullLayout.xaxis;
    const yaxis = fullLayout.yaxis;
    const rect = graph.getBoundingClientRect();
    const x = trace.x[index];
    const y = trace.y[index];
    return {
      x: rect.left + xaxis._offset + xaxis.l2p(x),
      y: rect.top + yaxis._offset + yaxis.l2p(y),
    };
  }, movedBefore.analytical_uuid);
  const dragRadius = 9;
  const polygon = [
    [point.x - dragRadius, point.y - dragRadius],
    [point.x + dragRadius, point.y - dragRadius],
    [point.x + dragRadius, point.y + dragRadius],
    [point.x - dragRadius, point.y + dragRadius],
    [point.x - dragRadius, point.y - dragRadius],
  ];
  await page.mouse.move(...polygon[0]);
  await page.mouse.down();
  for (const [x, y] of polygon.slice(1)) {
    await page.mouse.move(x, y, { steps: 3 });
  }
  await page.mouse.up();

  await page.getByText('1 selected — double-click the plot to clear.', { exact: true }).waitFor();
  const selectedTable = page.locator('table.data-table tbody tr');
  assert.equal(await selectedTable.count(), 1);
  assert.match(await selectedTable.first().innerText(), /S_MOVE/);
  assert.ok(!(await page.locator('body').innerText()).includes(movedBefore.analytical_uuid));
  await page.getByLabel('Target group').selectOption(target.path);
  await page.getByRole('button', { name: 'Assign 1 unit', exact: true }).click();
  await page.getByText('Assignment committed.', { exact: true }).waitFor();
  assert.ok(!(await page.locator('body').innerText()).includes(movedBefore.analytical_uuid));

  const sourceAfter = await api(`groups/rows?path=${encodeURIComponent(source.path)}`, undefined, 'GET');
  const targetAfter = await api(`groups/rows?path=${encodeURIComponent(target.path)}`, undefined, 'GET');
  const groupsAfter = await api('groups', undefined, 'GET');
  assert.equal(sourceAfter.rows.length, sourceBefore.rows.length - 1);
  assert.equal(targetAfter.rows.length, targetBefore.rows.length + 1);
  assert.equal(sourceAfter.rows.filter((row) => row.analytical_uuid === movedBefore.analytical_uuid).length, 0);
  const movedAfter = targetAfter.rows.filter((row) => row.analytical_uuid === movedBefore.analytical_uuid);
  assert.equal(movedAfter.length, 1, 'the selected analytical unit moved to the existing target exactly once');
  assert.deepEqual(movedAfter[0], movedBefore, 'the moved analytical unit keeps its UUID and entire row payload');
  const byUuid = (rows) => [...rows].sort((left, right) => left.analytical_uuid.localeCompare(right.analytical_uuid));
  assert.deepEqual(
    byUuid(sourceAfter.rows),
    byUuid(sourceBefore.rows.filter((row) => row.analytical_uuid !== movedBefore.analytical_uuid)),
    'the source contains exactly its prior rows except the moved unit',
  );
  assert.deepEqual(
    byUuid(targetAfter.rows),
    byUuid([...targetBefore.rows, movedBefore]),
    'the target contains exactly its prior rows plus the moved unit',
  );
  const targetMetadata = groupsAfter.find((candidate) => candidate.path === target.path)?.group;
  assert.equal(targetMetadata?.group_name, 'Target', 'the destination file retains its target group metadata');
  assert.equal(targetMetadata?.row_count, targetAfter.rows.length);

  // Reload and revisit both group files to verify the committed membership is durable.
  await page.reload();
  await page.getByLabel('Dataset').selectOption(source.path);
  await page.waitForFunction((expected) => {
    const graph = document.querySelector('.visualize-scatter');
    return graph?.dataset.renderState === 'complete'
      && graph.querySelector('.main-svg')
      && (graph.data ?? []).reduce((sum, trace) => sum + (trace.x?.length ?? 0), 0) === expected;
  }, sourceAfter.rows.length);
  assert.ok(!(await chartUuids(page)).includes(movedBefore.analytical_uuid));
  assert.ok(!(await page.locator('body').innerText()).includes(movedBefore.analytical_uuid));
  await page.getByLabel('Dataset').selectOption(target.path);
  await page.waitForFunction((uuid) => {
    const graph = document.querySelector('.visualize-scatter');
    return graph?.dataset.renderState === 'complete'
      && (graph.data ?? []).some((trace) => (trace.customdata ?? []).some((row) => row?.[0] === uuid));
  }, movedBefore.analytical_uuid);
  assert.ok((await chartUuids(page)).includes(movedBefore.analytical_uuid));
  assert.ok(!(await page.locator('body').innerText()).includes(movedBefore.analytical_uuid));
  assert.deepEqual(pageErrors, []);

  console.log(JSON.stringify({
    status: 'passed',
    procedure: 'visualize-lasso-assignment',
    source_path: source.path,
    target_path: target.path,
    source_rows_before: sourceBefore.rows.length,
    source_rows_after: sourceAfter.rows.length,
    target_rows_before: targetBefore.rows.length,
    target_rows_after: targetAfter.rows.length,
    plot_holder_height_px: holderBox.height,
    assignment_controls_below_plot: true,
    mouse_selected_visible_id: movedBefore.visible_id,
    moved_uuid_count_in_target: movedAfter.length,
    moved_row_payload_preserved: true,
    reload_preserved_move: true,
  }, null, 2));
} finally {
  await browser.close();
}
