import type { GroupsService, GroupRowsResponse, TransactionResponse, TransferUnitsRequest } from '@archaeodash/client';

export type MoveOutcome = {
  transaction: TransactionResponse;
  nextPath: string;
  refreshed: { paths: string[]; rows: GroupRowsResponse } | null;
  refreshError: string | null;
};

/** A committed transfer must never be retried because a subsequent read failed. */
export async function moveAndReload(groups: GroupsService, request: TransferUnitsRequest): Promise<MoveOutcome> {
  const transaction = await groups.transferUnits(request);
  const nextPath = transaction.deleted_paths.includes(request.source_path) ? request.destination_path : request.source_path;
  try {
    const candidates = await groups.scan();
    const rows = await groups.rows(nextPath);
    return { transaction, nextPath, refreshed: { paths: candidates.filter(candidate => candidate.ready).map(candidate => candidate.path), rows }, refreshError: null };
  } catch (error) {
    return { transaction, nextPath, refreshed: null, refreshError: error instanceof Error ? error.message : String(error) };
  }
}
