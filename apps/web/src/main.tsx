/**
 * Web entry (Section 9.2/9.3): pick the transport (Tauri IPC inside the
 * desktop webview, HTTP elsewhere), paint the stored theme before anything
 * async resolves, then hydrate the persisted theme preference without the
 * legacy startup race.
 */
import { StrictMode, useCallback, useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import { createRoot } from 'react-dom/client';
import { createBrowserRouter, Navigate, Outlet, RouterProvider, useLocation, useNavigate, type RouteObject } from 'react-router';
import type { AppInfo, AuthService, Transport } from '@archaeodash/client';
import { AppShell } from './shell/AppShell.tsx';
import { AnalysisSetupProvider } from './analysis-setup/AnalysisSetupContext.tsx';
import { GroupInclusionProvider } from './datasets/GroupInclusion.tsx';
import { DataCacheProvider } from './datasets/DataCache.tsx';
import { ExportsProvider } from './ui/TableExport.tsx';
import {
  HelpPage,
  HelpWindow,
  HomePage,
  PrivacyPage,
  ShellInfoContext,
  TermsPage,
} from './shell/routes.tsx';
import {
  AccountPage,
  ResetPasswordPage,
  VerifyEmailPage,
} from './shell/AccountPage.tsx';
import { HostedProjectsPage } from './shell/HostedProjectsPage.tsx';
import { ExplorePage, type ExploreDeps } from './explore/ExplorePage.tsx';
import { OrdinationPage, type OrdinationDeps } from './ordination/OrdinationPage.tsx';
import { VisualizePage, type VisualizeDeps } from './visualize/VisualizePage.tsx';
import { MultiplotWindow } from './visualize/MultiplotWindow.tsx';
import { parsePopoutRoute } from './visualize/multiplot-window.ts';
import { AnalysisPage, type AnalysisDeps } from './clustering/AnalysisPage.tsx';
import { applyTheme, hydrateTheme, normalizeTheme, readStoredTheme, saveThemePreference, themeStorageKey } from './theme.ts';
import { AppErrorPage } from './ui/ErrorBoundary.tsx';
import { HELP_WINDOW_PATH, revealLogFolder } from './ui/external.tsx';
import { BASE_PATH } from './basePath.ts';
import { errorText, notify } from './ui/toast-store.ts';
import { createTransport, isDemoMode } from './transport.ts';
import {
  GuestContext,
  HostedGate,
  HostedWorkspaceContext,
  observeSession,
  readLastProject,
  saveLastProject,
  type GuestControl,
  type HostedGateState,
  type HostedWorkspace,
} from './shell/hosted.tsx';
import './shell/shell.css';

/**
 * Which server the web app talks to. `plain`: a local single-project API
 * (development, browser e2e, `?demo`). `hosted`: the hosted server with
 * accounts; `username` is null while signed out.
 */
type ServerMode = { mode: 'checking' } | { mode: 'plain' } | { mode: 'hosted'; username: string | null };

/** The web app can use hosted accounts: HTTP transport, not the demo. */
function hostedCapable(transport: Transport): boolean {
  return transport.kind === 'http' && Boolean(transport.auth && transport.hostedWorkspace) && !isDemoMode();
}

function AppRoot({ transport, sessionEvents }: { transport: Transport; sessionEvents: EventTarget }): ReactElement {
  const [theme, setTheme] = useState<string>(() => readStoredTheme());
  const [appInfo, setAppInfo] = useState<AppInfo | undefined>(undefined);
  const [projectPath, setProjectPath] = useState('');
  const [projectName, setProjectName] = useState('');
  const [projectGeneration, setProjectGeneration] = useState(0);
  const [projectStatus, setProjectStatus] = useState('');
  const projectGenerationRef = useRef(0);
  useEffect(() => {
    let cancelled = false;
    transport
      .appInfo()
      .then((info) => {
        if (!cancelled) setAppInfo(info);
      })
      .catch(() => {
        /* offline/unready: the shell still renders */
      });
    // Hosted only: the desktop theme lives in local storage (theme.ts).
    void hydrateTheme(transport, readStoredTheme()).then((hydrated) => {
      if (!cancelled) setTheme(hydrated);
    });
    return () => {
      cancelled = true;
    };
  }, [transport]);

  useEffect(() => {
    let cancelled = false;
    const projects = transport.projects;
    const adopt = (project: { path: string; name: string; generation: number } | null) => {
      if (!cancelled && (project?.generation ?? 0) >= projectGenerationRef.current) {
        projectGenerationRef.current = project?.generation ?? 0;
        setProjectPath(project?.path ?? '');
        setProjectName(project?.name ?? '');
        setProjectGeneration(project?.generation ?? 0);
      }
    };
    void (async () => {
      const current = await projects?.current();
      if (current || !projects?.recent || !projects.openRecent) return adopt(current ?? null);
      // Nothing open yet (a fresh launch): reopen the last project if its
      // folder still exists, like most desktop apps.
      const last = (await projects.recent()).find((p) => p.available);
      if (!last || cancelled || projectGenerationRef.current > 0) return;
      setProjectStatus(`Reopening ${last.name}…`);
      try {
        adopt(await projects.openRecent(last.path));
      } finally {
        if (!cancelled) setProjectStatus('');
      }
    })().catch(() => { if (!cancelled) setProjectStatus(''); });
    return () => { cancelled = true; };
  }, [transport]);

  const desktop = transport.kind === 'tauri';
  const shellInfo = useMemo(() => ({ appInfo, desktop }), [appInfo, desktop]);

  // ---- Hosted server: session, project scoping, session expiry ----
  const navigate = useNavigate();
  const location = useLocation();
  const locationRef = useRef(location);
  locationRef.current = location;
  const [server, setServer] = useState<ServerMode>(() => (hostedCapable(transport) ? { mode: 'checking' } : { mode: 'plain' }));
  const username = server.mode === 'hosted' ? server.username : null;
  const usernameRef = useRef<string | null>(null);
  usernameRef.current = username;
  /** Where to go back to after signing in again (set when a session ends). */
  const returnTo = useRef<string | null>(null);
  const sessionEnded = useRef(false);

  const refreshSession = useCallback(async () => {
    const auth = transport.auth;
    if (!hostedCapable(transport) || !auth) return;
    try {
      const info = await auth.session();
      setServer({ mode: 'hosted', username: info.authenticated ? info.username ?? '' : null });
    } catch {
      // No session route: a plain single-project API. Later failures keep
      // the current state (a network blip is not a mode change).
      setServer((current) => (current.mode === 'checking' ? { mode: 'plain' } : current));
    }
  }, [transport]);

  useEffect(() => {
    void refreshSession();
    const onChange = () => void refreshSession();
    sessionEvents.addEventListener('change', onChange);
    return () => sessionEvents.removeEventListener('change', onChange);
  }, [refreshSession, sessionEvents]);

  /** Shows `key`/`name` as the open project and resets per-project state. */
  const scopeTo = useCallback((key: string, name: string) => {
    const generation = projectGenerationRef.current + 1;
    projectGenerationRef.current = generation;
    setProjectPath(key);
    setProjectName(name);
    setProjectGeneration(generation);
  }, []);

  const setHostedProject = useCallback((project: { project_id: string; name: string } | null) => {
    transport.hostedWorkspace?.set(project?.project_id ?? null);
    scopeTo(project?.project_id ?? '', project?.name ?? '');
  }, [transport, scopeTo]);

  // ---- Guest session: the token lives only in this page's memory ----
  const [guestToken, setGuestToken] = useState<string | null>(null);
  const guestTokenRef = useRef<string | null>(null);
  guestTokenRef.current = guestToken;
  const [guestStarting, setGuestStarting] = useState(false);

  /** Ends the guest session (server data deleted) and closes it here. */
  const endGuest = useCallback(() => {
    const token = guestTokenRef.current;
    if (!token) return;
    transport.guest?.end(token);
    guestTokenRef.current = null;
    setGuestToken(null);
    transport.hostedWorkspace?.setGuest(null);
    scopeTo('', '');
  }, [transport, scopeTo]);

  const startGuest = useCallback(async () => {
    const guest = transport.guest;
    if (!guest || guestTokenRef.current) return;
    setGuestStarting(true);
    try {
      const session = await guest.start();
      sessionEnded.current = false;
      transport.hostedWorkspace?.setGuest(session.token);
      guestTokenRef.current = session.token;
      setGuestToken(session.token);
      // A fresh key per session, never derived from the token.
      scopeTo(`guest-${Date.now()}`, 'Guest session');
      notify({ kind: 'info', text: 'Guest session started. Your work is kept only while this page stays open.' });
      if (['/account', '/projects'].includes(locationRef.current.pathname)) navigate('/');
    } catch (error) {
      notify({ kind: 'error', text: `Could not start a guest session: ${errorText(error)}` });
    } finally {
      setGuestStarting(false);
    }
  }, [transport, scopeTo, navigate]);

  // While a guest session is open: keep it alive, warn before leaving, and
  // end it on the server when the page goes away.
  useEffect(() => {
    const guest = transport.guest;
    if (!guestToken || !guest) return;
    const ping = window.setInterval(() => {
      void guest.ping(guestToken).then((alive) => {
        if (!alive && guestTokenRef.current === guestToken) guestEndedRef.current();
      }).catch(() => {});
    }, 5 * 60 * 1000);
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = '';
    };
    const leave = () => guest.end(guestToken);
    window.addEventListener('beforeunload', warn);
    window.addEventListener('pagehide', leave);
    return () => {
      window.clearInterval(ping);
      window.removeEventListener('beforeunload', warn);
      window.removeEventListener('pagehide', leave);
    };
  }, [transport, guestToken]);

  /** The server ended the guest session (idle, too old, or storage cleanup). */
  const guestEndedRef = useRef<() => void>(() => {});
  guestEndedRef.current = () => {
    guestTokenRef.current = null;
    setGuestToken(null);
    transport.hostedWorkspace?.setGuest(null);
    scopeTo('', '');
    notify({ kind: 'error', text: 'Your guest session has ended and its data was deleted. Start a new one, or sign in to keep your work.' });
    navigate('/account');
  };

  const openHostedProject = useCallback((project: { project_id: string; name: string }) => {
    sessionEnded.current = false;
    setHostedProject(project);
    if (usernameRef.current) saveLastProject(usernameRef.current, project.project_id);
    if (locationRef.current.pathname === '/projects') navigate('/');
  }, [navigate, setHostedProject]);

  // Sign-in: reopen this user's last project, then leave the Account page.
  // Sign-out: close the project.
  const previousUser = useRef<string | null>(null);
  useEffect(() => {
    if (server.mode !== 'hosted') return;
    const before = previousUser.current;
    previousUser.current = username;
    if (username === before) return;
    if (username === null) {
      setHostedProject(null);
      return;
    }
    if (before !== null) setHostedProject(null); // a different user signed in
    if (guestTokenRef.current) endGuest(); // signing in ends a guest session
    void hydrateTheme(transport, readStoredTheme()).then(setTheme);
    let cancelled = false;
    void (async () => {
      const last = readLastProject(username);
      let reopened = false;
      if (last && transport.hostedProjects) {
        try {
          const project = await transport.hostedProjects.get(last);
          if (cancelled) return;
          sessionEnded.current = false;
          setHostedProject(project);
          reopened = true;
        } catch {
          saveLastProject(username, null); // deleted, or no longer reachable
        }
      }
      if (cancelled) return;
      if (locationRef.current.pathname === '/account') {
        const target = returnTo.current ?? (reopened ? '/' : '/projects');
        returnTo.current = null;
        navigate(target);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [server.mode, username, transport, navigate, setHostedProject, endGuest]);

  // A scoped request refused with 401: the session ended while working.
  useEffect(() => {
    const workspace = transport.hostedWorkspace;
    if (!workspace || server.mode !== 'hosted') return;
    workspace.onUnauthorized(() => {
      if (guestTokenRef.current) {
        guestEndedRef.current();
        return;
      }
      if (sessionEnded.current) return;
      sessionEnded.current = true;
      const here = locationRef.current;
      returnTo.current = `${here.pathname}${here.search}`;
      notify({ kind: 'error', text: 'Your session has ended. Sign in again to continue.' });
      setServer({ mode: 'hosted', username: null });
      navigate('/account');
    });
    return () => workspace.onUnauthorized(null);
  }, [transport, server.mode, navigate]);

  const gateState: HostedGateState | null =
    server.mode === 'checking' ? 'checking'
      : server.mode === 'hosted' ? (projectPath ? null : username === null ? 'signed-out' : 'no-project')
      : null;
  const hostedShell = server.mode === 'plain'
    ? undefined
    : { username, guest: guestToken !== null, gate: gateState ? <HostedGate state={gateState} /> : undefined };
  const guestControl = useMemo<GuestControl | null>(
    () => (server.mode === 'hosted' && transport.guest
      ? { active: guestToken !== null, starting: guestStarting, start: () => void startGuest() }
      : null),
    [server.mode, transport, guestToken, guestStarting, startGuest],
  );
  const hostedWorkspace = useMemo<HostedWorkspace | null>(
    () => (server.mode === 'hosted' && username !== null ? { projectId: projectPath, open: openHostedProject } : null),
    [server.mode, username, projectPath, openHostedProject],
  );

  return (
    <ShellInfoContext.Provider value={shellInfo}>
    <HostedWorkspaceContext.Provider value={hostedWorkspace}>
    <GuestContext.Provider value={guestControl}>
    <ExportsProvider service={transport.exports}>
    <AnalysisSetupProvider service={transport.transformations} generation={projectGeneration}>
    <GroupInclusionProvider key={projectPath} projectKey={projectPath || 'default'}>
    <DataCacheProvider key={`${projectPath}:${projectGeneration}`}>
    <AppShell
      theme={theme}
      onThemeChange={(next) => {
        const chosen = normalizeTheme(next);
        setTheme(chosen);
        applyTheme(chosen);
        // Section 10.1: theme is an allowlisted persisted preference; local
        // choice paints immediately, the server upsert follows (hosted only).
        void saveThemePreference(transport, chosen);
      }}
      onShowLogFolder={desktop ? showLogFolder : undefined}
      hosted={hostedShell}
      appInfo={appInfo}
      projects={transport.projects}
      dataManager={transport}
      projectPath={projectPath}
      projectName={projectName}
      projectGeneration={projectGeneration}
      projectStatus={projectStatus}
      onProjectOpened={(project) => {
        projectGenerationRef.current = project.generation;
        setProjectPath(project.path);
        setProjectName(project.name);
        setProjectGeneration(project.generation);
      }}
    />
    </DataCacheProvider>
    </GroupInclusionProvider>
    </AnalysisSetupProvider>
    </ExportsProvider>
    </GuestContext.Provider>
    </HostedWorkspaceContext.Provider>
    </ShellInfoContext.Provider>
  );
}

function showLogFolder(): void {
  void revealLogFolder().catch((error: unknown) => notify({ kind: 'error', text: `Could not open the log folder: ${errorText(error)}` }));
}

/** Hosted-only routes: accounts, hosted projects, and email links. */
function hostedRoutes(transport: Transport, auth: AuthService | undefined): RouteObject[] {
  return [
    { path: 'account', element: <AccountPage auth={auth} /> },
    {
      path: 'projects',
      element: (
        <HostedProjectsPage projects={transport.hostedProjects} />
      ),
    },
    { path: 'auth/verify', element: <VerifyEmailPage auth={auth} /> },
    { path: 'auth/reset', element: <ResetPasswordPage auth={auth} /> },
  ];
}

function routeChildren(deps: ExploreDeps, ordinationDeps: OrdinationDeps, visualizeDeps: VisualizeDeps, analysisDeps: AnalysisDeps, transport: Transport, auth: AuthService | undefined): RouteObject[] {
  return [
    { index: true, element: <HomePage /> },
    // The Data Manager now lives in the sidebar; keep old /data links working.
    { path: 'data', element: <Navigate to="/" replace /> },
    { path: 'explore', element: <ExplorePage deps={deps} /> },
    { path: 'visualize', element: <VisualizePage deps={visualizeDeps} /> },
    { path: 'ordination', element: <OrdinationPage deps={ordinationDeps} /> },
    { path: 'cluster', element: <AnalysisPage key="cluster" kind="cluster" deps={analysisDeps} /> },
    { path: 'probabilities', element: <AnalysisPage key="membership" kind="membership" deps={analysisDeps} /> },
    { path: 'euclidean', element: <AnalysisPage key="euclidean" kind="euclidean" deps={analysisDeps} /> },
    // One Help page (two kept-alive copies would duplicate heading ids).
    { path: 'info', element: <Navigate to="/info/help" replace /> },
    { path: 'info/help', element: <HelpPage /> },
    { path: 'info/terms', element: <TermsPage /> },
    { path: 'info/privacy', element: <PrivacyPage /> },
    // The desktop app has no hosted accounts or hosted projects.
    ...(transport.kind === 'tauri' ? [] : hostedRoutes(transport, auth)),
    { path: '*', element: <HomePage /> },
  ];
}

export function createAppRouter(transport: Transport) {
  const visualizeDeps: VisualizeDeps = {
    groups: transport.groups,
    ordination: transport.ordination,
    exports: transport.exports,
    transformations: transport.transformations,
  };
  const exploreDeps: ExploreDeps = {
    groups: transport.groups,
    explore: transport.explore,
    exports: transport.exports,
  };
  const ordinationDeps: OrdinationDeps = {
    groups: transport.groups,
    ordination: transport.ordination,
    exports: transport.exports,
  };
  // Sign-in and sign-out on the Account page notify the app root.
  const sessionEvents = new EventTarget();
  const auth = transport.auth && hostedCapable(transport)
    ? observeSession(transport.auth, () => sessionEvents.dispatchEvent(new Event('change')))
    : transport.auth;
  return createBrowserRouter([
    {
      element: <AppRoot transport={transport} sessionEvents={sessionEvents} />,
      // Last resort for errors outside a page (pages have their own boundary).
      errorElement: <AppErrorPage />,
      children: routeChildren(exploreDeps, ordinationDeps, visualizeDeps, transport, transport, auth),
    },
  ], BASE_PATH ? { basename: BASE_PATH } : undefined);
}

export async function boot(rootEl: Element | null): Promise<void> {
  if (!rootEl) throw new Error('missing #root');
  // Paint the stored theme before anything async resolves (Section 9.3).
  applyTheme(readStoredTheme());
  if (window.location.pathname === `${BASE_PATH}${HELP_WINDOW_PATH}`) {
    // The separate Help window: static help only, no backend. Follow theme
    // changes made in the main window (same origin, shared local storage).
    window.addEventListener('storage', (event) => {
      if (event.key === themeStorageKey) applyTheme(normalizeTheme(event.newValue));
    });
    document.title = 'ArchaeoDash Help';
    createRoot(rootEl).render(
      <StrictMode>
        <HelpWindow />
      </StrictMode>,
    );
    return;
  }
  const popout = parsePopoutRoute(window.location.pathname);
  if (popout) {
    // A multiplot pop-out window: plots only, read from the data the main
    // window saved; no backend. Follows theme changes like Help.
    window.addEventListener('storage', (event) => {
      if (event.key === themeStorageKey) applyTheme(normalizeTheme(event.newValue));
    });
    document.title = 'Multiplot — ArchaeoDash';
    createRoot(rootEl).render(
      <StrictMode>
        <MultiplotWindow snapshotKey={popout.key} pair={popout.pair} />
      </StrictMode>,
    );
    return;
  }
  const transport = await createTransport();
  const router = createAppRouter(transport);
  createRoot(rootEl).render(
    <StrictMode>
      <RouterProvider router={router} />
    </StrictMode>,
  );
}

void boot(document.getElementById('root'));
