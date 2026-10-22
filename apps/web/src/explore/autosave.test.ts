import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { PatchDescriptiveValuesRequest, TransactionResponse } from '@archaeodash/client';
import { CellAutosave, cellKey } from './autosave.ts';

/** Fake backend: one file per path, revision bumps per patch. */
function backend(revisions: Record<string, number>) {
  const calls: PatchDescriptiveValuesRequest[] = [];
  const values = new Map<string, string | null>();
  let gate: Promise<void> = Promise.resolve();
  let release: () => void = () => {};
  const patch = async (request: PatchDescriptiveValuesRequest): Promise<TransactionResponse> => {
    calls.push(request);
    await gate;
    const current = revisions[request.path]!;
    if (request.expected_revision !== `r${current}`) {
      throw Object.assign(new Error('revision conflict on file'), { envelope: { code: 'revision_conflict' } });
    }
    revisions[request.path] = current + 1;
    for (const e of request.edits) values.set(cellKey(e.analytical_uuid, e.column), e.value ?? null);
    return {
      transaction_id: 't',
      action: 'patch_descriptive_values',
      deleted_paths: [],
      outputs: [
        {
          path: request.path,
          group_id: 'g',
          group_name: 'g',
          revision_id: `r${current + 1}`,
          row_count: 1,
          elemental_columns: [],
          descriptive_columns: [],
        },
      ],
    };
  };
  return {
    calls,
    values,
    patch,
    hold() {
      gate = new Promise((resolve) => (release = resolve));
    },
    release: () => release(),
  };
}

const edit = (uuid: string, before: string | null, after: string | null, path = 'A') => ({
  path,
  uuid,
  column: 'Site',
  before,
  after,
  label: `Site for ${uuid}`,
});

test('an unchanged value is not saved; empty text saves as missing', async () => {
  const b = backend({ A: 0 });
  const a = new CellAutosave(b.patch, new Map([['A', 'r0']]));
  a.commit(edit('u1', 'Baca', 'Baca'));
  a.commit(edit('u2', null, ''));
  assert.equal(b.calls.length, 0);
  a.commit(edit('u1', 'Baca', ''));
  await a.idle();
  assert.equal(b.values.get(cellKey('u1', 'Site')), null);
});

test('edits made during a save are bundled into the next save at the new revision', async () => {
  const b = backend({ A: 0 });
  const a = new CellAutosave(b.patch, new Map([['A', 'r0']]));
  b.hold();
  a.commit(edit('u1', 'x', 'one'));
  a.commit(edit('u2', 'y', 'two'));
  a.commit(edit('u3', 'z', 'three'));
  assert.equal(a.saving, true);
  b.release();
  await a.idle();
  assert.deepEqual(
    b.calls.map((c) => [c.expected_revision, c.edits.length]),
    [
      ['r0', 1],
      ['r1', 2],
    ],
  );
  assert.equal(a.undoStack.length, 3);
  assert.equal(a.status.size, 0);
});

test('undo restores the previous value and redo reapplies it; a new edit clears redo', async () => {
  const b = backend({ A: 0 });
  const a = new CellAutosave(b.patch, new Map([['A', 'r0']]));
  a.commit(edit('u1', 'Baca', 'Hooper'));
  await a.idle();
  a.undo();
  await a.idle();
  assert.equal(b.values.get(cellKey('u1', 'Site')), 'Baca');
  assert.equal(a.saved.get(cellKey('u1', 'Site')), 'Baca');
  assert.equal(a.redoStack.length, 1);
  a.redo();
  await a.idle();
  assert.equal(b.values.get(cellKey('u1', 'Site')), 'Hooper');
  a.undo();
  await a.idle();
  a.commit(edit('u2', null, 'new'));
  await a.idle();
  assert.equal(a.redoStack.length, 0, 'a fresh edit clears redo');
});

test('saves to different files run independently with their own revisions', async () => {
  const b = backend({ A: 3, B: 7 });
  const a = new CellAutosave(b.patch, new Map([['A', 'r3'], ['B', 'r7']]));
  a.commit(edit('u1', null, 'a', 'A'));
  a.commit(edit('u2', null, 'b', 'B'));
  await a.idle();
  assert.deepEqual(b.calls.map((c) => [c.path, c.expected_revision]).sort(), [
    ['A', 'r3'],
    ['B', 'r7'],
  ]);
});

test('a revision conflict marks the cell, blocks that file, and is not retried blindly', async () => {
  const b = backend({ A: 5 });
  const a = new CellAutosave(b.patch, new Map([['A', 'r0']]));
  a.commit(edit('u1', 'x', 'y'));
  await a.idle();
  assert.equal(a.status.get(cellKey('u1', 'Site')), 'error');
  assert.equal(a.failedCount, 1);
  assert.ok(a.conflicts.has('A'));
  a.retry();
  await a.idle();
  assert.equal(b.calls.length, 1, 'conflicted file is not resent');
  assert.equal(a.undoStack.length, 0, 'failed edits are not undoable');
});

test('a transient failure can be retried', async () => {
  const b = backend({ A: 0 });
  let fail = true;
  const a = new CellAutosave(async (request) => {
    if (fail) {
      fail = false;
      throw new Error('disk busy');
    }
    return b.patch(request);
  }, new Map([['A', 'r0']]));
  a.commit(edit('u1', 'x', 'y'));
  await a.idle();
  assert.equal(a.failedCount, 1);
  a.retry();
  await a.idle();
  assert.equal(a.failedCount, 0);
  assert.equal(b.values.get(cellKey('u1', 'Site')), 'y');
  assert.equal(a.undoStack.length, 1);
});
