/** Multiplot component tests: mode toggle, SSR safety, sampling label, uuid hiding. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import { Multiplot, PANEL_CHUNK, panelsPerStep, type MultiplotProps } from './Multiplot.tsx';
import { buildSnapshot, newSnapshotKey, parsePopoutRoute, popoutRoute } from './multiplot-window.ts';

const UUID_A = '0197bbbb-0000-7000-8000-00000000000a';
const UUID_B = '0197bbbb-0000-7000-8000-00000000000b';

function baseProps(): MultiplotProps {
  return {
    columns: ['Ti', 'Sr', 'Zr'],
    values: [
      [1, 2, 3, 4],
      [2, 4, 6, 8],
      [null, 1, 2, 3],
    ],
    groupLabels: ['Baca', 'Baca', 'Other', 'Other'],
    groupNames: ['Baca', 'Other'],
    rowIndices: [0, 1, 2, 3],
    rowUuids: [UUID_A, UUID_B, '0197bbbb-0000-7000-8000-00000000000c', '0197bbbb-0000-7000-8000-00000000000d'],
  };
}

test('multiplot renders the render-mode toggle with static as the default', () => {
  const html = renderToString(<Multiplot {...baseProps()} />);
  assert.ok(html.includes('Render mode'), 'render mode control present');
  assert.ok(html.includes('Static (SVG)'), 'static option present');
  assert.ok(html.includes('Interactive (Plotly)'), 'interactive option present');
  assert.ok(html.includes('multiplot-panel'), 'static SVG panels render by default');
  assert.ok(!html.includes('multiplot-panel-plotly'), 'no plotly placeholders in static mode');
  assert.ok(html.includes('Save PNG') && html.includes('Save SVG'), 'plot save control present');
});

test('interactive mode is SSR-safe: placeholder panels, no window access, no uuid leak', () => {
  const html = renderToString(<Multiplot {...baseProps()} initialMode="interactive" />);
  assert.equal((html.match(/multiplot-panel-plotly/g) ?? []).length >= 6, true, 'one placeholder per ordered pair');
  assert.ok(html.includes('Scatter of Sr by Ti'), 'aria labels name the axes');
  assert.ok(!html.includes('0197bbbb'), 'analytical_uuid never rendered');
  assert.ok(!html.includes('NaN'), 'null cells skipped cleanly');
});

test('interactive mode reports per-group sampling while static mode keeps every row', () => {
  const total = 100_001;
  const props = baseProps();
  const sampled: MultiplotProps = {
    ...props,
    // Rows are missing-valued here, so the test exercises allocation/status without circles.
    rowIndices: Array.from({ length: total }, (_, i) => i),
    values: [[], [], []],
    groupLabels: Array.from({ length: total }, (_, i) => i % 2 === 0 ? 'Baca' : 'Other'),
    rowUuids: [],
  };
  const staticHtml = renderToString(<Multiplot {...sampled} initialMode="static" />);
  assert.ok(!staticHtml.includes('Sampled'), 'static plots preserve all source rows');
  const interactiveHtml = renderToString(<Multiplot {...sampled} initialMode="interactive" />);
  assert.ok(interactiveHtml.includes('Sampled 99,996 of 600,006 interactive points'));
  assert.ok(interactiveHtml.includes('up to 8,333 per group and facet'));
});

test('procedure 13 renders 99,960 points for four columns, five groups, and 15,000 rows', () => {
  const groupLabels = Array.from({ length: 15_000 }, (_, i) => `G${Math.floor(i / 3_000) + 1}`);
  const props: MultiplotProps = {
    columns: ['a', 'b', 'c', 'd'],
    values: Array.from({ length: 4 }, (_, column) =>
      Array.from({ length: 15_000 }, (_, row) => row + column / 10)),
    groupLabels,
    groupNames: ['G1', 'G2', 'G3', 'G4', 'G5'],
    rowIndices: Array.from({ length: 15_000 }, (_, i) => i),
  };
  const html = renderToString(<Multiplot {...props} initialMode="interactive" />);
  assert.equal((html.match(/class="multiplot-panel-plotly"/g) ?? []).length, 12);
  assert.ok(html.includes('Sampled 99,960 of 180,000 interactive points'));
  assert.ok(html.includes('up to 1,666 per group and facet'));
});

test('unsampled data shows no sampling status', () => {
  const html = renderToString(<Multiplot {...baseProps()} />);
  assert.ok(!html.includes('Sampled'), 'no sampling label when the plan keeps every point');
});

test('progressive render chunk control exists with many pairs', () => {
  const props = baseProps();
  const html = renderToString(
    <Multiplot
      {...props}
      columns={['Ti', 'Sr', 'Zr', 'Ba', 'Ca', 'Mn', 'K', 'P']}
    />,
  );
  // 8 columns -> 56 ordered pairs; the first chunk of 12 draws, the rest follow.
  // SSR inserts comment nodes between interpolated text — strip them first.
  const flat = html.replace(/<!-- -->/g, '');
  assert.ok(flat.includes('Drawing panels: 12 of 56'), 'progress shown');
  assert.ok(flat.includes('>Stop<'), 'stop control present');
  assert.equal((html.match(/class="multiplot-panel"/g) ?? []).length, 12);
});

test('X and Y selectors build the legacy Y-by-X facet grid', () => {
  const html = renderToString(<Multiplot {...baseProps()} initialX={['Ti']} initialY={['Sr', 'Zr']} />);
  assert.ok(html.includes('X columns') && html.includes('Y columns'), 'both pickers present');
  // Ti x {Sr, Zr} -> two panels, in Y order.
  assert.equal((html.match(/class="multiplot-panel"/g) ?? []).length, 2);
  assert.ok(html.indexOf('Scatter of Sr by Ti') < html.indexOf('Scatter of Zr by Ti'));
  assert.ok(html.includes('multiplot-legend'), 'shared group legend');
});

test('one X column wraps the panels into a grid with no diagonal gap', () => {
  const props: MultiplotProps = {
    ...baseProps(),
    columns: ['a', 'b', 'c', 'd', 'e'],
    values: Array.from({ length: 5 }, () => [1, 2, 3, 4]),
  };
  const html = renderToString(<Multiplot {...props} initialX={['a']} />);
  // a against b..e: four panels in a 2 x 2 grid, not a single column.
  assert.ok(html.includes('grid-template-columns:repeat(2, max-content)'), html.match(/grid-template-columns:[^;"]*/)?.[0]);
  assert.equal((html.match(/class="multiplot-panel"/g) ?? []).length, 4);
  assert.ok(!html.includes('multiplot-cell-empty'), 'no blank X = Y cell in a wrapped grid');
  const byRow = renderToString(<Multiplot {...props} initialY={['c']} />);
  assert.ok(byRow.includes('grid-template-columns:repeat(2, max-content)'), 'one Y column wraps too');
});

test('static panels draw axis ticks and survive large inputs without spreading', () => {
  const n = 70_000;
  const props: MultiplotProps = {
    columns: ['a', 'b'],
    values: [Array.from({ length: n }, (_, i) => i), Array.from({ length: n }, (_, i) => n - i)],
    groupLabels: Array.from({ length: n }, () => 'G'),
    groupNames: ['G'],
    rowIndices: Array.from({ length: n }, (_, i) => i),
    initialX: ['a'],
    initialY: ['b'],
  };
  const html = renderToString(<Multiplot {...props} />);
  assert.ok(html.includes('>20000<'), 'tick labels rendered');
});

test('drawing steps shrink when each panel holds many points', () => {
  assert.equal(panelsPerStep(4), PANEL_CHUNK);
  assert.equal(panelsPerStep(10_000), 6);
  assert.equal(panelsPerStep(100_000), 1);
});

test('the open-in-new-window control offers one window or one per plot', () => {
  const html = renderToString(<Multiplot {...baseProps()} />);
  assert.ok(html.includes('All plots in one window'));
  assert.ok(html.includes('One window per plot'));
  assert.ok(html.includes('Open in new window'));
  const inWindow = renderToString(<Multiplot {...baseProps()} windowControls={false} />);
  assert.ok(!inWindow.includes('Open in new window'), 'hidden inside a pop-out window');
});

test('pop-out snapshots keep only the plotted columns and filtered rows', () => {
  const props = baseProps();
  const snap = buildSnapshot(
    { ...props, palette: ['#111', '#222'], rowIndices: [1, 3] },
    { x: ['Zr'], y: ['Ti'], mode: 'static', pointSize: 2, height: 900 },
  );
  assert.deepEqual(snap.columns, ['Ti', 'Zr']);
  assert.deepEqual(snap.values, [[2, 4], [1, 3]]);
  assert.deepEqual(snap.groupLabels, ['Baca', 'Other']);
  assert.deepEqual(snap.rowUuids, [UUID_B, '0197bbbb-0000-7000-8000-00000000000d']);
  assert.deepEqual([snap.x, snap.y], [['Zr'], ['Ti']]);
});

test('pop-out routes round-trip and reject anything else', () => {
  const key = newSnapshotKey();
  assert.match(key, /^[a-z0-9]{8,64}$/);
  assert.deepEqual(parsePopoutRoute(popoutRoute(key)), { key, pair: null });
  assert.deepEqual(parsePopoutRoute(popoutRoute(key, { x: 2, y: 0 })), { key, pair: { x: 2, y: 0 } });
  assert.equal(parsePopoutRoute('/visualize'), null);
  assert.equal(parsePopoutRoute(`${popoutRoute(key)}/../x`), null);
});
