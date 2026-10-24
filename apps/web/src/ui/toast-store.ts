/**
 * App-wide nonblocking notifications (Section 9.3; the legacy
 * `mynotification`): any page can call `notify()`; the shell renders the
 * list in an aria-live region (ui/Toasts.tsx). Pure store, no React.
 */

export type ToastKind = 'info' | 'success' | 'warning' | 'error';

export interface Toast {
  readonly id: number;
  readonly kind: ToastKind;
  readonly text: string;
}

export interface NotifyOptions {
  readonly kind?: ToastKind;
  readonly text: string;
  /** Auto-dismiss delay; 0 keeps the toast until dismissed. Default 5 s (errors 10 s). */
  readonly durationMs?: number;
}

/** Most toasts kept on screen at once; the oldest goes first. */
const MAX_TOASTS = 5;

export class ToastStore {
  private list: readonly Toast[] = [];
  private readonly listeners = new Set<() => void>();
  private readonly timers = new Map<number, ReturnType<typeof setTimeout>>();
  private nextId = 1;

  /** Shows a notification; returns its id (for `dismiss`). */
  notify = ({ kind = 'info', text, durationMs }: NotifyOptions): number => {
    const id = this.nextId++;
    const kept = [...this.list, { id, kind, text }];
    for (const dropped of kept.slice(0, Math.max(0, kept.length - MAX_TOASTS))) this.clearTimer(dropped.id);
    this.list = kept.slice(-MAX_TOASTS);
    const delay = durationMs ?? (kind === 'error' ? 10_000 : 5_000);
    if (delay > 0) {
      const timer = setTimeout(() => this.dismiss(id), delay);
      // Never keep a Node test process alive for a pending toast.
      (timer as { unref?: () => void }).unref?.();
      this.timers.set(id, timer);
    }
    this.publish();
    return id;
  };

  dismiss = (id: number): void => {
    this.clearTimer(id);
    if (!this.list.some((toast) => toast.id === id)) return;
    this.list = this.list.filter((toast) => toast.id !== id);
    this.publish();
  };

  snapshot = (): readonly Toast[] => this.list;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  private clearTimer(id: number): void {
    const timer = this.timers.get(id);
    if (timer !== undefined) clearTimeout(timer);
    this.timers.delete(id);
  }

  private publish(): void {
    for (const listener of this.listeners) listener();
  }
}

/** The app's single toast store. */
export const toasts = new ToastStore();

/**
 * Shows a nonblocking notification from anywhere in the client, e.g.
 * `notify({ kind: 'success', text: 'Groups merged.' })`.
 */
export function notify(options: NotifyOptions): number {
  return toasts.notify(options);
}

export function dismissToast(id: number): void {
  toasts.dismiss(id);
}

/** Error text for a caught value (`Error.message`, else `String(value)`). */
export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
