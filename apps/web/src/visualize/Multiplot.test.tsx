/** Multiplot component tests: mode toggle, SSR safety, sampling label, uuid hiding. */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import { Multiplot, type MultiplotProps } from './Multiplot.tsx';

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
  assert.ok(html.includes('Save plots (SVG)'), 'plot save control present');
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
    groupLabels: [],
    rowUuids: [],
  };
  const staticHtml = renderToString(<Multiplot {...sampled} initialMode="static" />);
  assert.ok(!staticHtml.includes('Sampled'), 'static plots preserve all source rows');
  const interactiveHtml = renderToString(<Multiplot {...sampled} initialMode="interactive" />);
  assert.ok(interactiveHtml.includes('Sampled 66,666 of 600,006 interactive points'));
  assert.ok(interactiveHtml.includes('up to 11,111 per group and facet'));
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
  // 8 columns -> 56 ordered pairs; the first chunk of 12 renders, the rest wait.
  // SSR inserts comment nodes between interpolated text — strip them first.
  const flat = html.replace(/<!-- -->/g, '');
  assert.ok(flat.includes('Continue rendering 44 more panels'), 'continue control present');
});
