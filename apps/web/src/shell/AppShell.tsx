/**
 * App shell (Section 9.3): top navbar with the legacy tab order, collapsible
 * Data Manager sidebar on wide screens, drawer/below-content on narrow
 * screens, and the theme selector. Semantic nav with accessible names.
 */
import { useState, type ReactElement } from 'react';
import { NavLink, Outlet } from 'react-router';
import { navRoutes } from './nav.ts';

export interface AppShellProps {
  readonly theme: string;
  readonly onThemeChange: (theme: string) => void;
  /** App metadata from the transport, rendered in the sidebar footer. */
  readonly appInfo?: { app: string; version: string; ready: boolean };
}

export function AppShell({ theme, onThemeChange, appInfo }: AppShellProps): ReactElement {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [infoOpen, setInfoOpen] = useState(false);

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
          <p className="sidebar-note">Dataset import and group management arrive with the Data Manager slice.</p>
          {appInfo && (
            <p className="sidebar-footer">
              {appInfo.app} {appInfo.version}
              {appInfo.ready ? '' : ' — API unavailable'}
            </p>
          )}
        </aside>
        <main className="main-panel" id="main-panel">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
