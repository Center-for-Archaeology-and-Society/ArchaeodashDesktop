/**
 * Loading indicators used across the app: a small spinner for buttons and
 * inline text, and a status line with a spinner for work in progress.
 */
import type { ReactElement, ReactNode } from 'react';

/** Decorative spinner; the surrounding text says what is happening. */
export function Spinner({ className }: { className?: string }): ReactElement {
  return <span className={className ? `spinner ${className}` : 'spinner'} aria-hidden="true" />;
}

/** "Working…" status line with a spinner (announced to screen readers). */
export function BusyNote({ children, className }: { children: ReactNode; className?: string }): ReactElement {
  return (
    <p role="status" className={className ? `busy-note ${className}` : 'busy-note'}>
      <Spinner />
      {children}
    </p>
  );
}

/** Button label that shows a spinner (and optional busy text) while working. */
export function BusyLabel({ busy, children, busyText }: { busy: boolean; children: ReactNode; busyText?: ReactNode }): ReactElement {
  return busy ? (
    <>
      <Spinner />
      {busyText ?? children}
    </>
  ) : (
    <>{children}</>
  );
}
