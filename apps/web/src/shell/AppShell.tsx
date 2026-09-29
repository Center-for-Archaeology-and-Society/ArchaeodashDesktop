/**
 * App shell (Section 9.3): top navbar with the legacy tab order, collapsible
 * Data Manager sidebar on wide screens, drawer/below-content on narrow
 * screens, and the theme selector. Semantic nav with accessible names.
 */
import { useRef, useState, type ReactElement } from 'react';
import { NavLink, Outlet } from 'react-router';
import { navRoutes } from './nav.ts';
import type { ProjectInfo } from '@archaeodash/contracts';
import type { ProjectsService } from '@archaeodash/client';

export interface AppShellProps {
  readonly theme: string;
  readonly onThemeChange: (theme: string) => void;
  /** App metadata from the transport, rendered in the sidebar footer. */
  readonly appInfo?: { app: string; version: string; ready: boolean };
  readonly projects?: ProjectsService;
  readonly projectPath?: string;
  readonly projectName?: string;
  readonly projectGeneration?: number;
  readonly onProjectOpened?: (project: ProjectInfo) => void;
}

export function AppShell({
  theme,
  onThemeChange,
  appInfo,
  projects,
  projectPath = '',
  projectName = '',
  projectGeneration = 0,
  onProjectOpened,
}: AppShellProps): ReactElement {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [infoOpen, setInfoOpen] = useState(false);
  const [projectError, setProjectError] = useState('');
  const [openingProject, setOpeningProject] = useState(false);
  const openingProjectRef = useRef(false);

  const openProject = async () => {
    if (!projects || openingProjectRef.current) return;
    openingProjectRef.current = true;
    setOpeningProject(true);
    setProjectError('');
    try {
      const project = await projects.open();
      if (project) onProjectOpened?.(project);
    } catch (error) {
      setProjectError(error instanceof Error ? error.message : String(error));
    } finally {
      openingProjectRef.current = false;
      setOpeningProject(false);
    }
  };

  return (
    <div className="app-shell" data-sidebar-open={sidebarOpen ? 'true' : 'false'}>
      <nav className="topnav" aria-label="Primary">
        <button
          type="button"
          className="sidebar-toggle"
          aria-expanded={sidebarOpen}
          aria-controls="data-manager-sidebar"
          aria-label={sidebarOpen ? 'Hide Data Manager' : 'Show Data Manager'}
          onClick={() => setSidebarOpen((open) => !open)}
        >
          {sidebarOpen ? '◀' : '▶'}
        </button>
        <span className="brand">ArchaeoDash</span>
        {projects && (
          <button type="button" onClick={() => void openProject()} disabled={openingProject}>
            {openingProject ? 'Opening…' : projectPath ? 'Switch Project' : 'Open Project'}
          </button>
        )}
        <ul className="nav-list">
          {navRoutes.map((route) =>
            route.children === undefined ? (
              <li key={route.path}>
                <NavLink to={route.path} end={route.path === '/'} className="nav-link">
                  {route.label}
                </NavLink>
              </li>
            ) : (
              <li
                key={route.path}
                className="nav-menu"
                onMouseEnter={() => setInfoOpen(true)}
                onMouseLeave={() => setInfoOpen(false)}
              >
                <button
                  type="button"
                  className="nav-link nav-menu-button"
                  aria-expanded={infoOpen}
                  aria-haspopup="true"
                  onClick={() => setInfoOpen((open) => !open)}
                >
                  {route.label}
                </button>
                {infoOpen && (
                  <ul className="nav-dropdown">
                    {route.children.map((child) => (
                      <li key={child.path}>
                        <NavLink
                          to={child.path}
                          className="nav-link"
                          onClick={() => setInfoOpen(false)}
                        >
                          {child.label}
                        </NavLink>
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            ),
          )}
        </ul>
        <label className="theme-select">
          Theme
          <select
            value={theme}
            aria-label="Theme"
            onChange={(event) => onThemeChange(event.target.value)}
          >
            <option value="simple">Simple</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
      </nav>
      <div className="app-body">
        <aside
          id="data-manager-sidebar"
          className="sidebar"
          aria-label="Data Manager"
          hidden={!sidebarOpen}
        >
          <h2>Data Manager</h2>
          {projectName && <p aria-label="Current project">Project: {projectName}</p>}
          <p className="sidebar-note">Dataset import and group management arrive with the Data Manager slice.</p>
          {appInfo && (
            <p className="sidebar-footer">
              {appInfo.app} {appInfo.version}
              {appInfo.ready ? '' : ' — API unavailable'}
            </p>
          )}
        </aside>
        <main className="main-panel" id="main-panel">
          {projectError && <p role="alert">Could not open project: {projectError}</p>}
          {projects && !projectPath ? (
            <p>Open a project folder to browse groups and run analyses.</p>
          ) : (
            <div key={`${projectGeneration}:${projectPath}`}>
              <Outlet />
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
