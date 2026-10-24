/**
 * Web entry (Section 9.2/9.3): pick the transport (Tauri IPC inside the
 * desktop webview, HTTP elsewhere), paint the stored theme before anything
 * async resolves, then hydrate the persisted theme preference without the
 * legacy startup race.
 */
import { StrictMode, useEffect, useMemo, useRef, useState, type ReactElement } from 'react';
import { createRoot } from 'react-dom/client';
import { createBrowserRouter, Navigate, Outlet, RouterProvider, type RouteObject } from 'react-router';
import type { AppInfo, Transport } from '@archaeodash/client';
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
import { AnalysisPage, type AnalysisDeps } from './clustering/AnalysisPage.tsx';
import { applyTheme, hydrateTheme, normalizeTheme, readStoredTheme, saveThemePreference, themeStorageKey } from './theme.ts';
import { AppErrorPage } from './ui/ErrorBoundary.tsx';
import { HELP_WINDOW_PATH, revealLogFolder } from './ui/external.tsx';
import { errorText, notify } from './ui/toast-store.ts';
import { createTransport } from './transport.ts';
import './shell/shell.css';

function AppRoot({ transport }: { transport: Transport }): ReactElement {
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

  return (
    <ShellInfoContext.Provider value={shellInfo}>
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
    </ShellInfoContext.Provider>
  );
}

function showLogFolder(): void {
  void revealLogFolder().catch((error: unknown) => notify({ kind: 'error', text: `Could not open the log folder: ${errorText(error)}` }));
}

/** Hosted-only routes: accounts, hosted projects, and email links. */
function hostedRoutes(transport: Transport): RouteObject[] {
  return [
    { path: 'account', element: <AccountPage auth={transport.auth} /> },
    {
      path: 'projects',
      element: (
        <HostedProjectsPage
          projects={transport.hostedProjects}
          files={transport.hostedFiles}
        />
      ),
    },
    { path: 'auth/verify', element: <VerifyEmailPage auth={transport.auth} /> },
    { path: 'auth/reset', element: <ResetPasswordPage auth={transport.auth} /> },
  ];
}

function routeChildren(deps: ExploreDeps, ordinationDeps: OrdinationDeps, visualizeDeps: VisualizeDeps, analysisDeps: AnalysisDeps, transport: Transport): RouteObject[] {
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
    ...(transport.kind === 'tauri' ? [] : hostedRoutes(transport)),
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
  return createBrowserRouter([
    {
      element: <AppRoot transport={transport} />,
      // Last resort for errors outside a page (pages have their own boundary).
      errorElement: <AppErrorPage />,
      children: routeChildren(exploreDeps, ordinationDeps, visualizeDeps, transport, transport),
    },
  ]);
}

export async function boot(rootEl: Element | null): Promise<void> {
  if (!rootEl) throw new Error('missing #root');
  // Paint the stored theme before anything async resolves (Section 9.3).
  applyTheme(readStoredTheme());
  if (window.location.pathname === HELP_WINDOW_PATH) {
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
  const transport = await createTransport();
  const router = createAppRouter(transport);
  createRoot(rootEl).render(
    <StrictMode>
      <RouterProvider router={router} />
    </StrictMode>,
  );
}

void boot(document.getElementById('root'));
