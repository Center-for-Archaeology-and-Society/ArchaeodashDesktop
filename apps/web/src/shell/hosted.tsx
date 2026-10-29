/**
 * Hosted mode (web app on the hosted server): sign in, open one of your
 * projects, then analyse it. The analysis services are scoped to the open
 * project through `transport.hostedWorkspace`; until a project is open the
 * analysis pages show a short gate instead.
 */
import { createContext, type ReactElement } from 'react';
import { Link } from 'react-router';
import type { AuthService, HostedProjectSummary } from '@archaeodash/client';
import { BusyNote } from '../ui/Busy.tsx';

/** Pages that need an open hosted project. Home, Account, Projects and Info do not. */
const ANALYSIS_PATHS = ['/data', '/explore', '/visualize', '/ordination', '/cluster', '/probabilities', '/euclidean'];

export function isAnalysisPath(pathname: string): boolean {
  const path = pathname.replace(/\/+$/, '') || '/';
  return ANALYSIS_PATHS.some((prefix) => path === prefix || path.startsWith(`${prefix}/`));
}

/** The open hosted project and the action that opens another. */
export interface HostedWorkspace {
  /** Open project ID, or '' when none is open. */
  readonly projectId: string;
  readonly open: (project: Pick<HostedProjectSummary, 'project_id' | 'name'>) => void;
}

/** Provided by the app root in hosted mode only. */
export const HostedWorkspaceContext = createContext<HostedWorkspace | null>(null);

/**
 * Wraps the auth service so the app hears about sign-in and sign-out made
 * on the Account page. `onChange` runs after each successful call.
 */
export function observeSession(auth: AuthService, onChange: () => void): AuthService {
  const after = <A extends unknown[], R>(call: (...args: A) => Promise<R>) =>
    async (...args: A): Promise<R> => {
      const result = await call(...args);
      onChange();
      return result;
    };
  return {
    ...auth,
    login: after(auth.login.bind(auth)),
    logout: after(auth.logout.bind(auth)),
    logoutAll: after(auth.logoutAll.bind(auth)),
  };
}

const lastProjectKey = (username: string) => `archaeodash.hosted.lastProject.${username}`;

/** The project this user last opened in this browser, if remembered. */
export function readLastProject(username: string): string | null {
  try {
    return window.localStorage.getItem(lastProjectKey(username));
  } catch {
    return null;
  }
}

export function saveLastProject(username: string, projectId: string | null): void {
  try {
    if (projectId) window.localStorage.setItem(lastProjectKey(username), projectId);
    else window.localStorage.removeItem(lastProjectKey(username));
  } catch {
    // Storage denied: the project just isn't reopened next time.
  }
}

export type HostedGateState = 'checking' | 'signed-out' | 'no-project';

/** Shown in place of an analysis page until signed in with a project open. */
export function HostedGate({ state }: { state: HostedGateState }): ReactElement {
  if (state === 'checking') {
    return (
      <section className="start-screen" aria-labelledby="start-heading">
        <h1 id="start-heading">ArchaeoDash</h1>
        <BusyNote>Checking your session…</BusyNote>
      </section>
    );
  }
  if (state === 'signed-out') {
    return (
      <section className="start-screen" aria-labelledby="start-heading">
        <h1 id="start-heading">Sign in to continue</h1>
        <p>Your projects and group files are stored with your account. Sign in, or create an account, to work with them.</p>
        <Link className="start-open" to="/account">Sign in or create an account</Link>
      </section>
    );
  }
  return (
    <section className="start-screen" aria-labelledby="start-heading">
      <h1 id="start-heading">Open a project</h1>
      <p>Choose one of your projects, or create a new one, to browse its groups and run analyses.</p>
      <Link className="start-open" to="/projects">Choose a project</Link>
    </section>
  );
}
