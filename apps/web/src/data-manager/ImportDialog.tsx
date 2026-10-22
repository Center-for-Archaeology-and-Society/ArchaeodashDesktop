/**
 * Modal overlay for the import wizard. A fixed overlay with role="dialog"
 * (not window.prompt/confirm or <dialog>, which behave inconsistently in the
 * Tauri webview): focus moves in on open, Tab is trapped, Escape closes,
 * and focus returns to the opener on close.
 */
import { useEffect, useId, useRef, type KeyboardEvent, type ReactElement, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export interface ImportDialogProps {
  readonly title: string;
  readonly onClose: () => void;
  readonly children: ReactNode;
}

export function ImportDialog({ title, onClose, children }: ImportDialogProps): ReactElement {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    ref.current?.focus();
    return () => opener?.focus();
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== 'Tab' || !ref.current) return;
    const items = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
    if (!items.length) return;
    const first = items[0]!;
    const last = items[items.length - 1]!;
    if (event.shiftKey && (document.activeElement === first || document.activeElement === ref.current)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const dialog = (
    <div className="fm-dialog-backdrop">
      <div
        ref={ref}
        className="fm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onKeyDown}
      >
        <header className="fm-dialog-header">
          <h2 id={titleId}>{title}</h2>
          <button type="button" className="fm-dialog-close" aria-label="Close" onClick={onClose}>
            ×
          </button>
        </header>
        <div className="fm-dialog-body">{children}</div>
      </div>
    </div>
  );
  return typeof document === 'undefined' ? dialog : createPortal(dialog, document.body);
}
