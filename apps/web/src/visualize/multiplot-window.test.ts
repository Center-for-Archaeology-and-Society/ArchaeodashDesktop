import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MIN_TILE, fitColumns, tileRects } from './multiplot-window.ts';

const screen = { left: 0, top: 0, width: 1920, height: 1040 };

test('windows of a full X by Y matrix keep their grid slots', () => {
  // 2 × 2 with the X = Y diagonal skipped: slots (0,1) and (1,0).
  const rects = tileRects(2, screen, [{ row: 0, col: 1 }, { row: 1, col: 0 }]);
  assert.deepEqual(rects, [
    { left: 960, top: 0, width: 960, height: 520 },
    { left: 0, top: 520, width: 960, height: 520 },
  ]);
});

test('without slots the windows fill a grid that fits the screen', () => {
  const rects = tileRects(6, screen);
  assert.equal(fitColumns(6, screen), 3);
  assert.equal(rects.length, 6);
  assert.deepEqual(rects[0], { left: 0, top: 0, width: 640, height: 520 });
  assert.deepEqual(rects[5], { left: 1280, top: 520, width: 640, height: 520 });
  // Three windows on a wide screen sit side by side.
  assert.equal(fitColumns(3, screen), 3);
  assert.equal(fitColumns(1, screen), 1);
});

test('tiles stop shrinking at the minimum size and overlap evenly instead', () => {
  const rects = tileRects(36, screen);
  for (const r of rects) {
    assert.ok(r.width >= MIN_TILE.width && r.height >= MIN_TILE.height);
    assert.ok(r.left >= 0 && r.left + r.width <= screen.width);
    assert.ok(r.top >= 0 && r.top + r.height <= screen.height);
  }
  const lefts = new Set(rects.map((r) => r.left));
  assert.ok(lefts.has(0) && lefts.has(screen.width - rects[0]!.width));
});

test('the screen offset is kept and colliding slots fall back to a fitted grid', () => {
  const second = { left: 1920, top: 30, width: 1280, height: 1000 };
  assert.deepEqual(tileRects(1, second), [{ left: 1920, top: 30, width: 1280, height: 1000 }]);
  const rects = tileRects(2, screen, [{ row: 0, col: 0 }, { row: 0, col: 0 }]);
  assert.notDeepEqual(rects[0], rects[1]);
  assert.deepEqual(tileRects(0, screen), []);
});
