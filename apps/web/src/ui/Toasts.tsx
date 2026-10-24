/**
 * Toast region mounted once in the app shell. Notifications come from
 * `notify()` (ui/toast-store.ts), which this module re-exports so pages can
 * `import { notify } from '../ui/Toasts.tsx'`.
 */
import { useSyncExternalStore, type ReactElement } from 'react';
import { toasts as defaultStore, type Toast, type ToastStore } from './toast-store.ts';

export { notify, dismissToast, errorText, type NotifyOptions, type ToastKind } from './toast-store.ts';


export function useToasts(store: ToastStore = defaultStore): readonly Toast[] {
  // The server snapshot is the live list too (static renders in tests).
  return useSyncExternalStore(store.subscribe, store.snapshot, store.snapshot);
}

/**
 * Polite live region (screen readers announce new toasts without moving
 * focus); errors are announced assertively via role="alert".
 */
export function Toasts({ store = defaultStore }: { store?: ToastStore }): ReactElement {
  const list = useToasts(store);
  return (
    <div className="toasts" aria-live="polite" aria-relevant="additions" aria-label="Notifications">
      {list.map((toast) => (
        <div key={toast.id} className={`toast toast-${toast.kind}`} role={toast.kind === 'error' ? 'alert' : 'status'}>
          <span className="toast-text">{toast.text}</span>
          <button type="button" className="toast-dismiss" aria-label="Dismiss notification" onClick={() => store.dismiss(toast.id)}>
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
