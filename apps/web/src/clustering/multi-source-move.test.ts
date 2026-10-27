import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { BatchTransferUnitsRequest, TransactionResponse, TransferUnitsRequest } from '@archaeodash/client';
import { includedAfterMove, moveAcrossSources } from './multi-source-move.ts';

function recorder() {
  const calls: { kind: string; request: TransferUnitsRequest | BatchTransferUnitsRequest }[] = [];
  let n = 0;
  const tx = (paths: string[]): TransactionResponse => ({
    transaction_id: `t${++n}`,
    action: 'move',
    deleted_paths: [],
    outputs: paths.map((path) => ({
      path,
      group_id: path,
      group_name: path,
      revision_id: `${path}#${n}`,
      row_count: 1,
      elemental_columns: [],
      descriptive_columns: [],
    })),
  });
  return {
    calls,
    groups: {
      transferUnits: async (request: TransferUnitsRequest) => {
        calls.push({ kind: 'single', request });
        return tx([request.source_path, request.destination_path]);
      },
      batchTransferUnits: async (request: BatchTransferUnitsRequest) => {
        calls.push({ kind: 'batch', request });
        return tx([request.source_path, ...request.targets.map((t) => t.destination_path)]);
      },
    },
  };
}

const context = {
  sourceOf: new Map([
    ['a1', 'A'],
    ['a2', 'A'],
    ['b1', 'B'],
    ['c1', 'C'],
  ]),
  sources: ['A', 'B', 'C'],
  revisions: new Map([
    ['A', 'A#0'],
    ['B', 'B#0'],
    ['C', 'C#0'],
  ]),
};

test('a manual move is split per source and skips units already in the destination', async () => {
  const r = recorder();
  const out = await moveAcrossSources(
    r.groups,
    {
      action: 'move',
      source_path: 'A',
      destination_path: 'B',
      selected_uuids: ['a1', 'b1', 'c1'],
      expected_source_revision: 'A#0',
    },
    context,
  );
  assert.equal(out.moved, 2);
  assert.deepEqual(
    r.calls.map((c) => [(c.request as TransferUnitsRequest).source_path, (c.request as TransferUnitsRequest).selected_uuids]),
    [
      ['A', ['a1']],
      ['C', ['c1']],
    ],
  );
});

test('a new destination is named once, then addressed at its new revision', async () => {
  const r = recorder();
  await moveAcrossSources(
    r.groups,
    {
      source_path: 'A',
      expected_source_revision: 'A#0',
      targets: [
        { destination_path: 'N', destination_group_name: 'New', expected_destination_revision: null, selected_uuids: ['a1', 'c1'] },
        { destination_path: 'B', expected_destination_revision: 'B#0', selected_uuids: ['a2'] },
      ],
    },
    context,
  );
  const batches = r.calls.map((c) => c.request as BatchTransferUnitsRequest);
  assert.deepEqual(batches.map((b) => b.source_path), ['A', 'C']);
  // First batch creates N and moves a2 into B.
  assert.equal(batches[0]!.targets[0]!.destination_group_name, 'New');
  assert.equal(batches[0]!.targets.length, 2);
  // Second batch reuses N without a name, at the revision the first step returned.
  assert.equal(batches[1]!.targets.length, 1);
  assert.equal(batches[1]!.targets[0]!.destination_group_name, undefined);
  assert.equal(batches[1]!.targets[0]!.expected_destination_revision, 'N#1');
});

test('a failure part-way reports how many units already moved', async () => {
  const r = recorder();
  let count = 0;
  const groups = {
    ...r.groups,
    transferUnits: async (request: TransferUnitsRequest) => {
      if (++count === 2) throw new Error('conflict');
      return r.groups.transferUnits(request);
    },
  };
  await assert.rejects(
    moveAcrossSources(
      groups,
      { action: 'move', source_path: 'A', destination_path: 'Z', selected_uuids: ['a1', 'b1'], expected_source_revision: 'A#0' },
      context,
    ),
    /conflict \(1 units had already moved\.\)/,
  );
});

test('a move into a new group reports it as created, and the group is included', async () => {
  const r = recorder();
  const out = await moveAcrossSources(
    r.groups,
    {
      action: 'move',
      source_path: 'A',
      destination_path: 'N',
      destination_group_name: 'New',
      selected_uuids: ['a1', 'c1'],
      expected_source_revision: 'A#0',
    },
    context,
  );
  // Created once, though two sources moved into it.
  assert.deepEqual(out.createdGroups, ['N']);
  assert.deepEqual(includedAfterMove(['A', 'B', 'C', 'N'], ['A', 'C'], out.createdGroups), ['A', 'C', 'N']);
});

test('a move into an existing unchecked group leaves it unchecked', async () => {
  const r = recorder();
  const out = await moveAcrossSources(
    r.groups,
    { action: 'move', source_path: 'A', destination_path: 'B', selected_uuids: ['a1'], expected_source_revision: 'A#0' },
    context,
  );
  // The transaction rewrites B, but B already existed.
  assert.ok(out.transactions[0]!.outputs.some((o) => o.path === 'B'));
  assert.deepEqual(out.createdGroups, []);
  // B was unchecked (only A and C included) and stays unchecked.
  assert.deepEqual(includedAfterMove(['A', 'B', 'C'], ['A', 'C'], out.createdGroups), ['A', 'C']);
  // A checked destination stays checked.
  assert.deepEqual(includedAfterMove(['A', 'B', 'C'], ['A', 'B'], out.createdGroups), ['A', 'B']);
});

test('a batch move includes only the new destinations, not existing unchecked ones', async () => {
  const r = recorder();
  const out = await moveAcrossSources(
    r.groups,
    {
      source_path: 'A',
      expected_source_revision: 'A#0',
      targets: [
        { destination_path: 'N', destination_group_name: 'New', expected_destination_revision: null, selected_uuids: ['a1', 'c1'] },
        { destination_path: 'B', expected_destination_revision: 'B#0', selected_uuids: ['a2'] },
      ],
    },
    context,
  );
  assert.deepEqual(out.createdGroups, ['N']);
  assert.deepEqual(includedAfterMove(['A', 'B', 'C', 'N'], ['A', 'C'], out.createdGroups), ['A', 'C', 'N']);
});

test('created groups match scanned paths whatever the separator', () => {
  assert.deepEqual(
    includedAfterMove(['g\\A.parquet', 'g\\B.parquet', 'g\\New.parquet'], ['g\\A.parquet'], ['g/New.parquet']),
    ['g\\A.parquet', 'g\\New.parquet'],
  );
});
