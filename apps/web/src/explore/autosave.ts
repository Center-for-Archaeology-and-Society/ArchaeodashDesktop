/**
 * Cell autosave for the Explore dataset table, with undo/redo.
 *
 * Each committed cell edit is queued per group file. A file has at most one
 * save in flight; edits made meanwhile are bundled into the next save, which
 * uses the revision the previous save returned (no reload, so the table keeps
 * focus and scroll). Successful edits go on the undo stack; undo and redo are
 * themselves saves of the previous / next value.
 *
 * Framework-free so it can be unit tested; the table subscribes through
 * `onChange`.
 */
import type { PatchDescriptiveValuesRequest, TransactionResponse } from '@archaeodash/client';

export type CellValue = string | null;

export interface CellEdit {
  readonly path: string;
  readonly uuid: string;
  readonly column: string;
  readonly before: CellValue;
  readonly after: CellValue;
  /** Human label for undo/redo buttons, e.g. "site_name for AID089". */
  readonly label: string;
}

type OpKind = 'edit' | 'undo' | 'redo';
interface Op {
  readonly kind: OpKind;
  /** The edit as the user made it (undo applies `before`). */
  readonly edit: CellEdit;
}

export type CellStatus = 'saving' | 'error';

export const cellKey = (uuid: string, column: string): string => `${uuid}\u0000${column}`;

/** Empty text is stored as a missing value. */
export function normalizeCell(value: string | null | undefined): CellValue {
  return value === undefined || value === null || value === '' ? null : value;
}

function isConflict(err: unknown): boolean {
  const code = (err as { envelope?: { code?: string } } | null)?.envelope?.code ?? '';
  const message = err instanceof Error ? err.message : String(err);
  return /conflict/i.test(code) || /revision/i.test(message);
}

export class CellAutosave {
  /** Saved values that differ from the loaded rows. */
  readonly saved = new Map<string, CellValue>();
  readonly status = new Map<string, CellStatus>();
  readonly undoStack: CellEdit[] = [];
  readonly redoStack: CellEdit[] = [];
  /** Last error message, and files that changed elsewhere. */
  error = '';
  readonly conflicts = new Set<string>();

  private readonly revisions: Map<string, string>;
  private readonly pending = new Map<string, Op[]>();
  private readonly failed: Op[] = [];
  private readonly inFlight = new Set<string>();
  private readonly listeners = new Set<() => void>();
  private idleWaiters: (() => void)[] = [];

  private readonly patch: (request: PatchDescriptiveValuesRequest) => Promise<TransactionResponse>;

  constructor(
    patch: (request: PatchDescriptiveValuesRequest) => Promise<TransactionResponse>,
    revisions: ReadonlyMap<string, string>,
  ) {
    this.patch = patch;
    this.revisions = new Map(revisions);
  }

  onChange(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** Queue a user edit; a no-op when the value did not change. */
  commit(edit: CellEdit): void {
    const next = { ...edit, before: normalizeCell(edit.before), after: normalizeCell(edit.after) };
    if (next.before === next.after) return;
    this.enqueue({ kind: 'edit', edit: next });
  }

  undo(): void {
    const edit = this.undoStack.pop();
    if (edit) this.enqueue({ kind: 'undo', edit });
  }

  redo(): void {
    const edit = this.redoStack.pop();
    if (edit) this.enqueue({ kind: 'redo', edit });
  }

  /** Re-queue saves that failed (not ones blocked by a conflict). */
  retry(): void {
    const ops = this.failed.splice(0).filter((op) => !this.conflicts.has(op.edit.path));
    this.error = '';
    for (const op of ops) this.enqueue(op);
    this.emit();
  }

  get saving(): boolean {
    return this.inFlight.size > 0 || [...this.pending.values()].some((ops) => ops.length > 0);
  }

  get failedCount(): number {
    return this.failed.length;
  }

  /** Resolves once nothing is queued or in flight. */
  idle(): Promise<void> {
    if (!this.saving) return Promise.resolve();
    return new Promise((resolve) => this.idleWaiters.push(resolve));
  }

  private target(op: Op): CellValue {
    return op.kind === 'undo' ? op.edit.before : op.edit.after;
  }

  private enqueue(op: Op): void {
    const { path, uuid, column } = op.edit;
    // Show the new value at once; the status marks it as unsaved.
    this.saved.set(cellKey(uuid, column), this.target(op));
    this.status.set(cellKey(uuid, column), 'saving');
    const list = this.pending.get(path) ?? [];
    list.push(op);
    this.pending.set(path, list);
    this.emit();
    void this.flush(path);
  }

  private async flush(path: string): Promise<void> {
    if (this.inFlight.has(path)) return;
    const ops = this.pending.get(path) ?? [];
    if (ops.length === 0) return;
    this.pending.set(path, []);
    this.inFlight.add(path);
    // One patch per file: the last value wins for a cell edited twice.
    const values = new Map<string, { uuid: string; column: string; value: CellValue }>();
    for (const op of ops) {
      values.set(cellKey(op.edit.uuid, op.edit.column), {
        uuid: op.edit.uuid,
        column: op.edit.column,
        value: this.target(op),
      });
    }
    try {
      const revision = this.revisions.get(path);
      if (!revision) throw new Error(`No current revision for ${path}.`);
      if (this.conflicts.has(path)) throw new Error(`${path} changed elsewhere; reload it to keep editing.`);
      const tx = await this.patch({
        path,
        expected_revision: revision,
        edits: [...values.values()].map((v) => ({ analytical_uuid: v.uuid, column: v.column, value: v.value })),
      });
      const out = tx.outputs.find((o) => o.path === path);
      if (out) this.revisions.set(path, out.revision_id);
      for (const op of ops) {
        const key = cellKey(op.edit.uuid, op.edit.column);
        // A later queued op for this cell still owns its status.
        if (!this.pendingFor(path, key)) this.status.delete(key);
        if (op.kind === 'undo') this.redoStack.push(op.edit);
        else {
          this.undoStack.push(op.edit);
          if (op.kind === 'edit') this.redoStack.length = 0;
        }
      }
    } catch (err: unknown) {
      if (isConflict(err)) this.conflicts.add(path);
      this.error = err instanceof Error ? err.message : String(err);
      for (const op of ops) {
        this.status.set(cellKey(op.edit.uuid, op.edit.column), 'error');
        this.failed.push(op);
      }
    } finally {
      this.inFlight.delete(path);
      this.emit();
      if ((this.pending.get(path) ?? []).length > 0) void this.flush(path);
      else if (!this.saving) {
        const waiters = this.idleWaiters.splice(0);
        for (const resolve of waiters) resolve();
      }
    }
  }

  private pendingFor(path: string, key: string): boolean {
    return (this.pending.get(path) ?? []).some((op) => cellKey(op.edit.uuid, op.edit.column) === key);
  }

  private emit(): void {
    for (const listener of this.listeners) listener();
  }
}
