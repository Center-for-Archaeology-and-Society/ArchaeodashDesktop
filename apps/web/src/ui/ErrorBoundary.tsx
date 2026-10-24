/**
 * Per-page error boundary: a render error in one page shows a message with
 * "Try again" in that page only; the shell, the sidebar, and the other
 * (kept-alive) pages stay usable.
 */
import { Component, type ErrorInfo, type ReactElement, type ReactNode } from 'react';
import { useRouteError } from 'react-router';

interface Props {
  readonly children: ReactNode;
  /** Names the page in the message, e.g. "Explore". */
  readonly label?: string;
}

interface State {
  readonly error: unknown;
  /** Bumped by "Try again" so the page remounts with fresh state. */
  readonly attempt: number;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export class PageErrorBoundary extends Component<Props, State> {
  override state: State = { error: undefined, attempt: 0 };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error ?? new Error('Unknown error') };
  }

  override componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error('Page failed to render', error, info.componentStack);
  }

  private readonly reset = () => {
    this.setState((state) => ({ error: undefined, attempt: state.attempt + 1 }));
  };

  override render(): ReactNode {
    const { error, attempt } = this.state;
    if (error !== undefined) {
      return (
        <section className="page page-error" role="alert">
          <h1>{`Something went wrong on ${this.props.label ?? 'this page'}`}</h1>
          <p>The rest of ArchaeoDash still works. Your project files were not changed by this error.</p>
          <p className="page-error-detail">{message(error)}</p>
          <button type="button" onClick={this.reset}>Try again</button>
        </section>
      );
    }
    return <ErrorBoundaryAttempt key={attempt}>{this.props.children}</ErrorBoundaryAttempt>;
  }
}

/** Keyed wrapper so a reset remounts the page instead of reusing its state. */
function ErrorBoundaryAttempt({ children }: { children: ReactNode }): ReactElement {
  return <>{children}</>;
}

/** Router-level fallback (`errorElement`) for errors outside any page. */
export function AppErrorPage(): ReactElement {
  const error = useRouteError();
  return (
    <section className="page page-error" role="alert">
      <h1>ArchaeoDash hit an unexpected error</h1>
      <p className="page-error-detail">{message(error)}</p>
      <button type="button" onClick={() => window.location.reload()}>Reload</button>
    </section>
  );
}
