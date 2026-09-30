import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToString } from 'react-dom/server';
import { attachScatterSelectionHandlers, VisualizeScatter } from './VisualizeScatter.tsx';

test('scatter exposes its loading state before Plotly is available', () => {
  const html = renderToString(
    <VisualizeScatter
      traces={[]}
      xLabel="Ti"
      yLabel="Sr"
      dragMode="lasso"
      onSelect={() => {}}
      onClearSelection={() => {}}
    />,
  );
  assert.match(html, /data-render-state="loading"/);
  assert.doesNotMatch(html, /role="alert"/);
});

test('scatter selection handlers use current callbacks and extract hidden uuids', () => {
  const listeners = new Map<string, (eventData: unknown) => void>();
  const removed: string[] = [];
  const gd = {
    removeAllListeners: (event: string) => { removed.push(event); },
    on: (event: string, callback: (eventData: unknown) => void) => { listeners.set(event, callback); },
  };
  const selectedCalls: string[][] = [];
  let cleared = 0;
  let current = {
    onSelect: (uuids: readonly string[]) => { selectedCalls.push([...uuids]); },
    onClearSelection: () => { cleared += 1; },
  };

  attachScatterSelectionHandlers(gd, () => current);
  current = {
    onSelect: (uuids: readonly string[]) => { selectedCalls.push([...uuids, 'latest']); },
    onClearSelection: () => { cleared += 10; },
  };
  listeners.get('plotly_selected')?.({
    points: [{ customdata: ['uuid-a', 'hover text'] }, { customdata: ['uuid-b'] }, { customdata: null }],
  });
  listeners.get('plotly_doubleclick')?.(null);

  assert.deepEqual(removed, ['plotly_selected', 'plotly_doubleclick']);
  assert.deepEqual(selectedCalls, [['uuid-a', 'uuid-b', 'latest']]);
  assert.equal(cleared, 10);
});
