/**
 * Web entry (Section 9.2/9.3): pick the transport (Tauri IPC inside the
 * desktop webview, HTTP elsewhere), paint the stored theme before anything
 * async resolves, then hydrate the persisted theme preference without the
 * legacy startup race.
 */
import { StrictMode, useEffect, useRef, useState, type ReactElement } from 'react';
import { createRoot } from 'react-dom/client';
import { createBrowserRouter, Outlet, RouterProvider, type RouteObject } from 'react-router';
import type { AppInfo, Transport } from '@archaeodash/client';
import { AppShell } from './shell/AppShell.tsx';
import {
  HelpPage,
  HomePage,
  PrivacyPage,
  TermsPage,
} from './shell/routes.tsx';
import { ExplorePage, type ExploreDeps } from './explore/ExplorePage.tsx';
import { OrdinationPage, type OrdinationDeps } from './ordination/OrdinationPage.tsx';
import { VisualizePage, type VisualizeDeps } from './visualize/VisualizePage.tsx';
import { AnalysisPage, type AnalysisDeps } from './clustering/AnalysisPage.tsx';
import { applyTheme, hydrateTheme, readStoredTheme } from './theme.ts';
import { createTransport } from './transport.ts';

function AppRoot({ transport }: { transport: Transport }): ReactElement {
  const [theme, setTheme] = useState<string>(() => readStoredTheme());
  const [appInfo, setAppInfo] = useState<AppInfo | undefined>(undefined);
  const [projectPath, setProjectPath] = useState('');
  const [projectName, setProjectName] = useState('');
  const [projectGeneration, setProjectGeneration] = useState(0);
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
    void hydrateTheme(transport, readStoredTheme()).then((hydrated) => {
      if (!cancelled) setTheme(hydrated);
    });
    return () => {
      cancelled = true;
    };
  }, [transport]);

  useEffect(() => {
    let cancelled = false;
    void transport.projects?.current().then((project) => {
      if (!cancelled && (project?.generation ?? 0) >= projectGenerationRef.current) {
        projectGenerationRef.current = project?.generation ?? 0;
        setProjectPath(project?.path ?? '');
        setProjectName(project?.name ?? '');
        setProjectGeneration(project?.generation ?? 0);
      }
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [transport]);

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
      projects={transport.projects}
      projectPath={projectPath}
      projectName={projectName}
      projectGeneration={projectGeneration}
      onProjectOpened={(project) => {
        projectGenerationRef.current = project.generation;
        setProjectPath(project.path);
        setProjectName(project.name);
        setProjectGeneration(project.generation);
      }}
    />
  );
}

function routeChildren(deps: ExploreDeps, ordinationDeps: OrdinationDeps, visualizeDeps: VisualizeDeps, analysisDeps: AnalysisDeps): RouteObject[] {
  return [
    { index: true, element: <HomePage /> },
    { path: 'explore', element: <ExplorePage deps={deps} /> },
    { path: 'visualize', element: <VisualizePage deps={visualizeDeps} /> },
    { path: 'ordination', element: <OrdinationPage deps={ordinationDeps} /> },
    { path: 'cluster', element: <AnalysisPage key="cluster" kind="cluster" deps={analysisDeps} /> },
    { path: 'probabilities', element: <AnalysisPage key="membership" kind="membership" deps={analysisDeps} /> },
    { path: 'euclidean', element: <AnalysisPage key="euclidean" kind="euclidean" deps={analysisDeps} /> },
    { path: 'info', element: <HelpPage /> },
    { path: 'info/help', element: <HelpPage /> },
    { path: 'info/terms', element: <TermsPage /> },
    { path: 'info/privacy', element: <PrivacyPage /> },
    { path: '*', element: <HomePage /> },
  ];
}

export function createAppRouter(transport: Transport) {
  const visualizeDeps: VisualizeDeps = {
    groups: transport.groups,
    ordination: transport.ordination,
    exports: transport.exports,
  };
  const exploreDeps: ExploreDeps = {
    groups: transport.groups,
    explore: transport.explore,
    exports: transport.exports,
    // Legacy `lastOpenedDataset` selector default (Section 10.1): hydrate on
    // mount, persist on every dataset open. Stable callbacks so the router
    // never remounts Explore when the preference changes.
    getInitialDataset: async () => {
      try {
        const prefs = await transport.preferences.get();
        const pref = prefs.preferences.find((p) => p.key === 'lastOpenedDataset');
        return typeof pref?.value === 'string' ? pref.value : '';
      } catch {
        return '';
      }
    },
    onDatasetOpened: (path: string) => {
      void transport.preferences.put('lastOpenedDataset', path).catch(() => {});
    },
  };
  const ordinationDeps: OrdinationDeps = {
    groups: transport.groups,
    ordination: transport.ordination,
    exports: transport.exports,
  };
  return createBrowserRouter([
    { element: <AppRoot transport={transport} />, children: routeChildren(exploreDeps, ordinationDeps, visualizeDeps, transport) },
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
