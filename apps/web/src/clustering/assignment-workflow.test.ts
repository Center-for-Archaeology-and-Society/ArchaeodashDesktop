import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { GroupsService, GroupRowsResponse, TransferUnitsRequest } from '@archaeodash/client';
import { moveAndReload } from './assignment-workflow.ts';

const request: TransferUnitsRequest = { action: 'move', source_path: 'source.parquet', destination_path: 'target.parquet', selected_uuids: ['hidden-uuid'], expected_source_revision: 'analysis-revision' };
const rows: GroupRowsResponse = { path: request.destination_path, revision_id: 'new-revision', visible_id_column: 'ID', legacy_rowid_column: 'rowid', descriptive_columns: [], elemental_columns: [], rows: [] };

test('an emptied source switches to the destination after exactly one transfer', async () => {
  const calls: string[] = [];
  const groups = {
    transferUnits: async (actual: TransferUnitsRequest) => { assert.deepEqual(actual, request); calls.push('move'); return { transaction_id: 'tx', action: 'move', outputs: [], deleted_paths: [request.source_path] }; },
    scan: async () => [{ path: request.destination_path, ready: true }, { path: 'invalid.parquet', ready: false }],
    rows: async (path: string) => { calls.push(path); return rows; },
  } as unknown as GroupsService;
  const outcome = await moveAndReload(groups, request);
  assert.deepEqual(calls, ['move', request.destination_path]);
  assert.equal(outcome.nextPath, request.destination_path);
  assert.deepEqual(outcome.refreshed?.paths, [request.destination_path]);
  assert.equal(outcome.refreshError, null);
});

test('a read failure after commit is a committed outcome, never a transfer retry', async () => {
  let moves = 0;
  const groups = {
    transferUnits: async () => { moves++; return { transaction_id: 'tx', action: 'move', outputs: [], deleted_paths: [] }; },
    scan: async () => { throw new Error('offline'); },
  } as unknown as GroupsService;
  const outcome = await moveAndReload(groups, request);
  assert.equal(moves, 1);
  assert.equal(outcome.transaction.transaction_id, 'tx');
  assert.equal(outcome.nextPath, request.source_path);
  assert.equal(outcome.refreshed, null);
  assert.equal(outcome.refreshError, 'offline');
});

test('revision conflict propagates without refreshing or retrying', async () => {
  let moves = 0;
  const groups = {
    transferUnits: async () => { moves++; throw new Error('revision_conflict'); },
    scan: async () => { assert.fail('must not scan on failed transfer'); },
  } as unknown as GroupsService;
  await assert.rejects(moveAndReload(groups, request), /revision_conflict/);
  assert.equal(moves, 1);
});

test('batch assignment submits exactly once and opens the first target when source is deleted', async () => {
  const { batchMoveAndReload } = await import('./assignment-workflow.ts');
  let batches = 0;
  const batch = { source_path: request.source_path, expected_source_revision: request.expected_source_revision,
    targets: [{ destination_path: 'first.parquet', destination_group_name: 'First', selected_uuids: ['u1'] },
      { destination_path: 'second.parquet', destination_group_name: 'Second', selected_uuids: ['u2'] }] };
  const groups = {
    batchTransferUnits: async (actual: unknown) => { batches++; assert.deepEqual(actual, batch); return { transaction_id: 'batch-tx', action: 'move_units', outputs: [], deleted_paths: [request.source_path] }; },
    transferUnits: async () => { assert.fail('batch must not call individual transfers'); },
    scan: async () => [{ path: 'first.parquet', ready: true }],
    rows: async (path: string) => { assert.equal(path, 'first.parquet'); return { ...rows, path }; },
  } as unknown as GroupsService;
  const outcome = await batchMoveAndReload(groups, batch);
  assert.equal(batches, 1);
  assert.equal(outcome.nextPath, 'first.parquet');
  assert.equal(outcome.refreshed?.candidates[0]?.path, 'first.parquet');
});

test('batch read failure preserves committed outcome and destination conflict is not retried', async () => {
  const { batchMoveAndReload } = await import('./assignment-workflow.ts');
  const batch = { source_path: request.source_path, expected_source_revision: request.expected_source_revision,
    targets: [{ destination_path: 'target.parquet', expected_destination_revision: 'old-target-revision', selected_uuids: ['u1'] }] };
  let batches = 0;
  const groups = {
    batchTransferUnits: async () => { batches++; return { transaction_id: 'batch-tx', action: 'move_units', outputs: [], deleted_paths: [] }; },
    scan: async () => { throw new Error('read unavailable'); },
  } as unknown as GroupsService;
  const outcome = await batchMoveAndReload(groups, batch);
  assert.equal(outcome.transaction.transaction_id, 'batch-tx');
  assert.equal(outcome.refreshError, 'read unavailable');
  assert.equal(batches, 1);
  groups.batchTransferUnits = async () => { batches++; throw new Error('destination revision conflict'); };
  await assert.rejects(batchMoveAndReload(groups, batch), /destination revision conflict/);
  assert.equal(batches, 2);
});
