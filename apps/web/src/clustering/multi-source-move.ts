/**
 * Moves for analysis results that span several group files. The result
 * components build one request with a placeholder source; here it is split
 * into one transfer per real source file (in project order). Revisions are
 * refreshed from each transaction's outputs, so a destination touched by an
 * earlier step is addressed at its new revision, and a newly created group
 * is only named on the step that creates it.
 *
 * Steps run one after another, so a failure part-way leaves earlier steps
 * committed; the error says how many units had already moved.
 */
import type {
  BatchTransferUnitsRequest,
  GroupsService,
  TransactionResponse,
  TransferUnitsRequest,
} from '@archaeodash/client';

export interface MultiSourceContext {
  /** Source file of each analytical unit. */
  readonly sourceOf: ReadonlyMap<string, string>;
  /** Source files in project order. */
  readonly sources: readonly string[];
  /** Current revision of every known group file. */
  readonly revisions: ReadonlyMap<string, string>;
}

export interface MultiSourceOutcome {
  readonly moved: number;
  readonly transactions: TransactionResponse[];
}

type GroupsMover = Pick<GroupsService, 'transferUnits' | 'batchTransferUnits'>;

function track(revisions: Map<string, string>, tx: TransactionResponse): void {
  for (const path of tx.deleted_paths) revisions.delete(path);
  for (const out of tx.outputs) revisions.set(out.path, out.revision_id);
}

export async function moveAcrossSources(
  groups: GroupsMover,
  request: TransferUnitsRequest | BatchTransferUnitsRequest,
  context: MultiSourceContext,
): Promise<MultiSourceOutcome> {
  const revisions = new Map(context.revisions);
  const created = new Set<string>();
  const transactions: TransactionResponse[] = [];
  let moved = 0;
  const fromSource = (uuids: readonly string[], source: string) =>
    uuids.filter((uuid) => context.sourceOf.get(uuid) === source);

  try {
    for (const source of context.sources) {
      const sourceRevision = revisions.get(source);
      if ('targets' in request) {
        const targets = request.targets
          .map((t) => ({ ...t, selected_uuids: fromSource(t.selected_uuids, source) }))
          .filter((t) => t.selected_uuids.length > 0 && t.destination_path !== source)
          .map((t) => {
            const isNew = t.destination_group_name != null && !created.has(t.destination_path);
            return isNew
              ? t
              : {
                  destination_path: t.destination_path,
                  expected_destination_revision: revisions.get(t.destination_path) ?? t.expected_destination_revision ?? null,
                  selected_uuids: t.selected_uuids,
                };
          });
        if (targets.length === 0) continue;
        if (!sourceRevision) throw new Error(`No current revision for ${source}.`);
        const tx = await groups.batchTransferUnits({
          source_path: source,
          expected_source_revision: sourceRevision,
          targets,
        });
        transactions.push(tx);
        track(revisions, tx);
        for (const t of targets) {
          created.add(t.destination_path);
          moved += t.selected_uuids.length;
        }
      } else {
        const uuids = fromSource(request.selected_uuids, source);
        if (uuids.length === 0 || request.destination_path === source) continue;
        if (!sourceRevision) throw new Error(`No current revision for ${source}.`);
        const name = created.has(request.destination_path) ? null : (request.destination_group_name ?? null);
        const tx = await groups.transferUnits({
          action: request.action,
          source_path: source,
          destination_path: request.destination_path,
          destination_group_name: name,
          selected_uuids: uuids,
          expected_source_revision: sourceRevision,
        });
        transactions.push(tx);
        track(revisions, tx);
        created.add(request.destination_path);
        moved += uuids.length;
      }
    }
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(moved > 0 ? `${message} (${moved} units had already moved.)` : message);
  }
  return { moved, transactions };
}
