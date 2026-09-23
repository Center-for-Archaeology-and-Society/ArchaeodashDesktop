/**
 * Web entry (Section 9.2/9.3): pick the transport (Tauri IPC inside the
 * desktop webview, HTTP elsewhere), paint the stored theme before anything
 * async resolves, then hydrate the persisted theme preference without the
 * legacy startup race.
 */
import { StrictMode, useEffect, useMemo, useState, type ReactElement } from 'react';
import { createRoot } from 'react-dom/client';
import { createBrowserRouter, Outlet, RouterProvider, type RouteObject } from 'react-router';
import type { AppInfo, Transport } from '@archaeodash/client';
import { AppShell } from './shell/AppShell.tsx';
import {
  ClusterPage,
  EuclideanPage,
  ExplorePage,
  type ExploreDeps,
  HelpPage,
  HomePage,
  OrdinationPage as OrdinationView,
  PrivacyPage,
  ProbabilitiesPage,
  TermsPage,
  VisualizePage,
} from './shell/routes.tsx';
import { OrdinationPage, type OrdinationDeps } from './ordination/OrdinationPage.tsx';
import { applyTheme, hydrateTheme, readStoredTheme } from './theme.ts';

function AppRoot({ transport }: { transport: Transport }): ReactElement {
  const [theme, setTheme] = useState<string>(() => readStoredTheme());
  const [appInfo, setAppInfo] = useState<AppInfo | undefined>(undefined);

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
    void hydrateTheme(transport, readStoredTheme()).then((hydrated) => {
      if (!cancelled) setTheme(hydrated);
    });
    return () => {
      cancelled = true;
    };
  }, [transport]);

  const deps = useMemo(
    () => ({
      groups: transport.groups,
      explore: transport.explore,
    }),
    [transport],
  );

  return (
    <AppShell
      theme={theme}
      onThemeChange={(next) => {
        setTheme(next);
        applyTheme(next as Parameters<typeof applyTheme>[0]);
      }}
      appInfo={appInfo}
    />
  );
}

function routeChildren(deps: ExploreDeps, ordinationDeps: OrdinationDeps): RouteObject[] {
  return [
    { index: true, element: <HomePage /> },
    { path: 'explore', element: <ExplorePage deps={deps} /> },
    { path: 'visualize', element: <VisualizePage /> },
    { path: 'ordination', element: <OrdinationPage deps={ordinationDeps} /> },
    { path: 'cluster', element: <ClusterPage /> },
    { path: 'probabilities', element: <ProbabilitiesPage /> },
    { path: 'euclidean', element: <EuclideanPage /> },
    { path: 'info', element: <HelpPage /> },
    { path: 'info/help', element: <HelpPage /> },
    { path: 'info/terms', element: <TermsPage /> },
    { path: 'info/privacy', element: <PrivacyPage /> },
    { path: '*', element: <HomePage /> },
  ];
}

export function createAppRouter(transport: Transport) {
  const exploreDeps: ExploreDeps = { groups: transport.groups, explore: transport.explore };
  const ordinationDeps: OrdinationDeps = {
    groups: transport.groups,
    ordination: transport.ordination,
  };
  return createBrowserRouter([
    { element: <AppRoot transport={transport} />, children: routeChildren(exploreDeps, ordinationDeps) },
  ]);
}

export async function boot(rootEl: Element | null): Promise<void> {
  if (!rootEl) throw new Error('missing #root');
  // Paint the stored theme before anything async resolves (Section 9.3).
  applyTheme(readStoredTheme());
  const transport = await createTransport();
  const router = createAppRouter(transport);
  createRoot(rootEl).render(
    <StrictMode>
      <RouterProvider router={router} />
    </StrictMode>,
  );
}

void boot(document.getElementById('root'));
