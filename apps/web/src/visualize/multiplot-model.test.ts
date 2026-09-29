/** Multiplot model tests: deterministic sampling, disjoint pairs, svg data url. */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { allPairs, samplingPlan, samplingStatusText, svgToDataUrl, uuidsFromSelectedEvent } from './multiplot-model.ts';

test('group/facet sampling preserves every row when below the cap', () => {
  const rows = [0, 1, 2, 3];
  const plan = samplingPlan(rows, ['A', 'A', 'B', 'B'], 6, 9);
  assert.equal(plan.sampled, false);
  assert.deepEqual(plan.indices, rows);
  assert.equal(plan.candidateCount, 24);
  assert.equal(plan.selectedCount, 24);
});

test('sampling above the ceiling keeps the first rows per group in source order', () => {
  const labels = Array.from({ length: 30 }, (_, i) => (i % 2 ? 'B' : 'A'));
  const rows = Array.from({ length: 30 }, (_, i) => i);
  const plan = samplingPlan(rows, labels, 2, 2, 20);
  assert.equal(plan.sampled, true);
  assert.equal(plan.perGroupFacet, 5);
  assert.deepEqual(plan.indices, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  assert.equal(plan.candidateCount, 60);
  assert.equal(plan.selectedCount, 20);
  assert.deepEqual(samplingPlan(rows, labels, 2, 2, 20).indices, plan.indices);
});

test('sampling is idempotent for tiny and degenerate inputs', () => {
  assert.deepEqual(samplingPlan([], [], 0, 0).indices, []);
  assert.deepEqual(samplingPlan([2, 5], ['A', 'B'], 1, 1).indices, [2, 5]);
});

test('sampling refuses configurations that cannot fit one row per group under the hard ceiling', () => {
  const plan = samplingPlan([0, 1], ['A', 'B'], 2, 4, 3);
  assert.equal(plan.selectedCount, 0);
  assert.match(samplingStatusText(plan) ?? '', /Too many groups or facets/);
});

test('procedure 13 R fixture matches exact first-per-group/per-facet source rows', () => {
  type Fixture = {
    max_points_total: number;
    candidate_count: number;
    facet_count: number;
    group_count: number;
    max_points_per_group_facet: number;
    selected_count: number;
    selected_index_set: { xvar: string; yvar: string; group: string; ordinal: number; source_rowid: number }[];
  };
  const golden = JSON.parse(readFileSync(new URL('../../../../fixtures/golden/13_multiplot_interactive_sampling.json', import.meta.url), 'utf8')) as Fixture;
  const csv = readFileSync(new URL('../../../../fixtures/INAA_test.csv', import.meta.url), 'utf8').trim().split(/\r?\n/);
  const header = csv[0]!.split(',');
  const groupColumn = header.indexOf('CORE');
  const sourceRows = new Map<string, number[]>();
  for (let row = 1; row < csv.length; row++) {
    const fields = csv[row]!.split(',');
    const group = fields[groupColumn]!;
    const list = sourceRows.get(group) ?? [];
    list.push(row);
    sourceRows.set(group, list);
  }
  const groups = [...sourceRows.keys()].sort();
  const labels: string[] = [];
  const sourceIds: number[] = [];
  for (const group of groups) {
    const rows = sourceRows.get(group)!;
    for (let i = 0; i < 3_000; i++) {
      labels.push(group);
      sourceIds.push(rows[i % rows.length]!);
    }
  }
  const rows = Array.from({ length: labels.length }, (_, i) => i);
  const facetCount = 12;
  const plan = samplingPlan(rows, labels, facetCount, facetCount, golden.max_points_total);
  assert.equal(plan.candidateCount, golden.candidate_count);
  assert.equal(plan.selectedCount, golden.selected_count);
  assert.equal(plan.perGroupFacet, golden.max_points_per_group_facet);
  assert.equal(plan.indices.length, golden.selected_count / facetCount);
  const sampledByGroup = new Map<string, number[]>();
  for (const i of plan.indices) {
    const group = labels[i]!;
    const list = sampledByGroup.get(group) ?? [];
    list.push(sourceIds[i]!);
    sampledByGroup.set(group, list);
  }
  const goldenByPanelGroup = new Map<string, number[]>();
  for (const row of golden.selected_index_set) {
    const key = `${row.xvar}/${row.yvar}/${row.group}`;
    const list = goldenByPanelGroup.get(key) ?? [];
    list.push(row.source_rowid);
    goldenByPanelGroup.set(key, list);
  }
  assert.equal(goldenByPanelGroup.size, facetCount * golden.group_count);
  for (const [key, expected] of goldenByPanelGroup) {
    const group = key.slice(key.lastIndexOf('/') + 1);
    assert.deepEqual(sampledByGroup.get(group), expected, key);
  }
});

test('allPairs enumerates disjoint ordered pairs', () => {
  assert.deepEqual(allPairs(3), [
    { xIndex: 1, yIndex: 0 },
    { xIndex: 2, yIndex: 0 },
    { xIndex: 0, yIndex: 1 },
    { xIndex: 2, yIndex: 1 },
    { xIndex: 0, yIndex: 2 },
    { xIndex: 1, yIndex: 2 },
  ]);
  assert.deepEqual(allPairs(0), []);
});

test('svg serialization yields a downloadable data url', () => {
  const fakeSerializer = {
    serializeToString: (el: Element) => `<svg data-name="${(el as unknown as { id: string }).id}"/>`,
  };
  const el = { id: 'panel-1' } as unknown as Element;
  const url = svgToDataUrl(fakeSerializer, el);
  assert.ok(url.startsWith('data:image/svg+xml;charset=utf-8,'));
  assert.ok(decodeURIComponent(url).includes('panel-1'));
});

test('sampling status text appears only when the ceiling dropped points', () => {
  assert.equal(samplingStatusText(samplingPlan([0, 1], ['A', 'A'], 1, 1)), null);
  const rows = Array.from({ length: 100_001 }, (_, i) => i);
  const plan = samplingPlan(rows, rows.map(() => 'A'), 1, 1);
  assert.equal(samplingStatusText(plan), 'Sampled 100,000 of 100,001 interactive points (up to 100,000 per group and facet)');
});

test('selection uuids are extracted from plotly_selected customdata only', () => {
  const event = {
    points: [
      { customdata: ['uuid-1', 'Baca'] },
      { customdata: ['uuid-2', 'Other'] },
      { customdata: ['', 'skipped'] },
      { x: 1, y: 2 },
    ],
  };
  assert.deepEqual(uuidsFromSelectedEvent(event), ['uuid-1', 'uuid-2']);
  assert.deepEqual(uuidsFromSelectedEvent(null), []);
  assert.deepEqual(uuidsFromSelectedEvent({ points: [] }), []);
});
