import type { GroupsService, GroupRowsResponse, TransactionResponse, TransferUnitsRequest, BatchTransferUnitsRequest, GroupCandidate } from '@archaeodash/client';

export type MoveOutcome = {
  transaction: TransactionResponse;
  nextPath: string;
  refreshed: { paths: string[]; rows: GroupRowsResponse; candidates: GroupCandidate[] } | null;
  refreshError: string | null;
};

/** A committed transfer must never be retried because a subsequent read failed. */
export async function moveAndReload(groups: GroupsService, request: TransferUnitsRequest): Promise<MoveOutcome> {
  const transaction = await groups.transferUnits(request);
  return reloadCommittedMove(groups, transaction, request.source_path, request.destination_path);
}

export async function batchMoveAndReload(groups: GroupsService, request: BatchTransferUnitsRequest): Promise<MoveOutcome> {
  if (!request.targets.length) throw new Error('Choose at least one destination.');
  const transaction = await groups.batchTransferUnits(request);
  return reloadCommittedMove(groups, transaction, request.source_path, request.targets[0]!.destination_path);
}

async function reloadCommittedMove(groups: GroupsService, transaction: TransactionResponse, sourcePath: string, destinationPath: string): Promise<MoveOutcome> {
  const nextPath = transaction.deleted_paths.includes(sourcePath) ? destinationPath : sourcePath;
  try {
    const candidates = await groups.scan();
    const rows = await groups.rows(nextPath);
    return { transaction, nextPath, refreshed: { paths: candidates.filter(candidate => candidate.ready).map(candidate => candidate.path), rows, candidates }, refreshError: null };
  } catch (error) {
    return { transaction, nextPath, refreshed: null, refreshError: error instanceof Error ? error.message : String(error) };
  }
}
