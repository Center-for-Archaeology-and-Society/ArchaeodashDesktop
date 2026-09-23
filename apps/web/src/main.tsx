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
  const [lastOpenedDataset, setLastOpenedDataset] = useState('');
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
    // Restore the legacy `lastOpenedDataset` selector default (Section 10.1).
    transport.preferences
      .get()
      .then((prefs) => {
        if (cancelled) return;
        const pref = prefs.preferences.find((p) => p.key === 'lastOpenedDataset');
        if (typeof pref?.value === 'string' && pref.value) setLastOpenedDataset(pref.value);
      })
      .catch(() => {
        /* offline: fall back to the first ready candidate */
      });
    return () => {
      cancelled = true;
    };
  }, [transport]);

  const deps = useMemo(
    () => ({
      groups: transport.groups,
      explore: transport.explore,
      exports: transport.exports,
      onDatasetOpened: (path: string) => {
        // Legacy `lastOpenedDataset` selector-default semantics (Section 10.1).
        void transport.preferences.put('lastOpenedDataset', path).catch(() => {});
      },
      initialDataset: lastOpenedDataset,
    }),
    [transport, lastOpenedDataset],
  );
  return (
    <AppShell
      theme={theme}
      onThemeChange={(next) => {
        setTheme(next);
        applyTheme(next as Parameters<typeof applyTheme>[0]);
        // Section 10.1: theme is an allowlisted persisted preference; local
        // choice paints immediately, the server upsert follows.
        void transport.preferences.put('theme', next).catch(() => {
          /* offline: the local choice still stands */
        });
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
