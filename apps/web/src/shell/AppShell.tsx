/**
 * App shell (Section 9.3): top navbar with the legacy tab order, collapsible
 * Data Manager sidebar on wide screens, drawer/below-content on narrow
 * screens, and the theme selector. Semantic nav with accessible names.
 */
import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactElement } from 'react';
import { NavLink } from 'react-router';
import { KeepAliveOutlet } from './KeepAliveOutlet.tsx';
import { navRoutes, type NavRoute } from './nav.ts';
import { DataManager, type DataManagerTransport } from '../data-manager/DataManager.tsx';
import type { ProjectInfo, RecentProject } from '@archaeodash/contracts';
import type { ProjectsService } from '@archaeodash/client';
import { ProjectMenu, RecentProjectList, revealLabel, useRecentProjects } from './RecentProjects.tsx';
import { ActivityIndicator } from './ActivityIndicator.tsx';
import { BusyLabel, BusyNote } from '../ui/Busy.tsx';
import { Toasts } from '../ui/Toasts.tsx';

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
  /** Startup message while the last project reopens, e.g. "Reopening Site A…". */
  readonly projectStatus?: string;
  /** Transport slice for the sidebar file manager; omitted in static tests. */
  readonly dataManager?: DataManagerTransport;
  /** Desktop: opens the app's log folder (an Info menu item). */
  readonly onShowLogFolder?: () => void;
}

/**
 * Dropdown menu in the top bar (Info). Click or Enter/Space toggles it;
 * Escape, choosing an item, or clicking elsewhere closes it; arrow keys move
 * between items.
 */
export function NavMenu({ route, onShowLogFolder, initiallyOpen = false }: { route: NavRoute; onShowLogFolder?: () => void; initiallyOpen?: boolean }): ReactElement {
  const [open, setOpen] = useState(initiallyOpen);
  const holder = useRef<HTMLLIElement | null>(null);
  const button = useRef<HTMLButtonElement | null>(null);
  const menuId = `nav-menu-${route.path.replace(/\W+/g, '')}`;
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (holder.current && !holder.current.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);
  const onKeyDown = (event: KeyboardEvent<HTMLLIElement>) => {
    if (event.key === 'Escape' && open) {
      event.stopPropagation();
      setOpen(false);
      button.current?.focus();
      return;
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    if (!open) setOpen(true);
    const items = [...(holder.current?.querySelectorAll<HTMLElement>('.nav-dropdown a, .nav-dropdown button') ?? [])];
    if (!items.length) {
      // The menu renders on this keypress; focus its first item next frame.
      requestAnimationFrame(() => holder.current?.querySelector<HTMLElement>('.nav-dropdown a, .nav-dropdown button')?.focus());
      return;
    }
    const at = items.indexOf(document.activeElement as HTMLElement);
    items[(at + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
  };
  return (
    <li className="nav-menu" ref={holder} onKeyDown={onKeyDown}>
      <button
        ref={button}
        type="button"
        className="nav-link nav-menu-button"
        aria-expanded={open}
        aria-haspopup="true"
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((value) => !value)}
      >
        {route.label}
      </button>
      {open && (
        <ul className="nav-dropdown" id={menuId}>
          {(route.children ?? []).map((child) => (
            <li key={child.path}>
              <NavLink to={child.path} className="nav-link" onClick={() => setOpen(false)}>
                {child.label}
              </NavLink>
            </li>
          ))}
          {onShowLogFolder && (
            <li className="nav-dropdown-extra">
              <button
                type="button"
                className="nav-link nav-dropdown-action"
                onClick={() => {
                  setOpen(false);
                  onShowLogFolder();
                }}
              >
                Show log folder
              </button>
            </li>
          )}
        </ul>
      )}
    </li>
  );
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
  projectStatus = '',
  dataManager,
  onShowLogFolder,
}: AppShellProps): ReactElement {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [projectError, setProjectError] = useState('');
  const [openingProject, setOpeningProject] = useState(false);
  const openingProjectRef = useRef(false);
  // Bumped when the sidebar changes project data so the routed page remounts
  // and rescans. Changes announced via notifyGroupsChanged() only refresh the
  // sidebar tree (the page that fired it already has fresh data).
  const [dataVersion, setDataVersion] = useState(0);
  const bumpDataVersion = useCallback(() => setDataVersion((version) => version + 1), []);
  const projectOpen = !projects || Boolean(projectPath);
  const { recent, refresh: refreshRecent, setRecent } = useRecentProjects(projects);
  const hasRecent = Boolean(projects?.recent && projects.openRecent);
  // The desktop app has no hosted accounts or hosted projects.
  const routes = projects ? navRoutes.filter((route) => route.path !== '/account' && route.path !== '/projects') : navRoutes;
  const reveal = projects?.reveal;
  const revealProject = reveal && projectPath
    ? () => { void reveal().catch((error: unknown) => setProjectError(error instanceof Error ? error.message : String(error))); }
    : undefined;

  /** Runs one project-opening action; the recent list refreshes after it. */
  const runOpen = async (open: () => Promise<ProjectInfo | null>) => {
    if (!projects || openingProjectRef.current) return;
    openingProjectRef.current = true;
    setOpeningProject(true);
    setProjectError('');
    try {
      const project = await open();
      if (project) onProjectOpened?.(project);
    } catch (error) {
      setProjectError(error instanceof Error ? error.message : String(error));
    } finally {
      openingProjectRef.current = false;
      setOpeningProject(false);
      refreshRecent();
    }
  };
  const openProject = () => runOpen(() => projects!.open());
  const openRecent = (project: RecentProject) => {
    const reopen = projects?.openRecent;
    if (reopen) void runOpen(() => reopen(project.path));
  };
  const removeRecent = (project: RecentProject) => {
    void projects?.removeRecent?.(project.path).then(setRecent).catch(() => refreshRecent());
  };
  const clearRecent = () => {
    void projects?.clearRecent?.().then(() => setRecent([])).catch(() => refreshRecent());
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
        <span className="brand">
          <img
            className="brand-mark"
            src="/brand-mark.png"
            srcSet="/brand-mark.png 1x, /brand-mark@2x.png 2x, /brand-mark@3x.png 3x"
            width={44}
            height={28}
            alt=""
          />
          ArchaeoDash
        </span>
        {projects && (
          <span className="project-controls">
            <button type="button" className="project-open" onClick={() => void openProject()} disabled={openingProject}>
              <BusyLabel busy={openingProject} busyText="Opening…">{projectPath ? 'Switch Project' : 'Open Project'}</BusyLabel>
            </button>
            {hasRecent && (
              <ProjectMenu
                recent={recent}
                currentPath={projectPath}
                busy={openingProject}
                onOpenRecent={openRecent}
                onBrowse={() => void openProject()}
                onClear={projects.clearRecent ? clearRecent : undefined}
                onReveal={revealProject}
              />
            )}
          </span>
        )}
        <ul className="nav-list">
          {routes.map((route) =>
            route.children === undefined ? (
              <li key={route.path}>
                <NavLink to={route.path} end={route.path === '/'} className="nav-link">
                  {route.label}
                </NavLink>
              </li>
            ) : (
              <NavMenu key={route.path} route={route} onShowLogFolder={onShowLogFolder} />
            ),
          )}
        </ul>
        <ActivityIndicator />
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
          className={dataManager ? 'sidebar sidebar-file-manager' : 'sidebar'}
          aria-label="Data Manager"
          hidden={!sidebarOpen}
        >
          <h2>Data Manager</h2>
          {dataManager && projectOpen ? (
            <DataManager
              key={`${projectGeneration}:${projectPath}`}
              transport={dataManager}
              projectName={projectName}
              onDataChanged={bumpDataVersion}
              onRevealProject={revealProject}
              revealLabel={revealLabel()}
            />
          ) : (
            <>
              {projectName && <p aria-label="Current project">Project: {projectName}</p>}
              <p className="sidebar-note">
                {projects && !projectPath
                  ? 'Open a project folder to see its group files here.'
                  : 'Import source files, inspect project groups, then choose a group in an analysis view.'}
              </p>
            </>
          )}
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
            <section className="start-screen" aria-labelledby="start-heading">
              <h1 id="start-heading">Open a project</h1>
              {projectStatus ? (
                <BusyNote>{projectStatus}</BusyNote>
              ) : (
                <p>Open a project folder to browse groups and run analyses.</p>
              )}
              <button type="button" className="start-open" onClick={() => void openProject()} disabled={openingProject}>
                <BusyLabel busy={openingProject} busyText="Opening…">Open a folder…</BusyLabel>
              </button>
              {recent.length > 0 && (
                <>
                  <h2>Recent projects</h2>
                  <RecentProjectList recent={recent} busy={openingProject || Boolean(projectStatus)} onOpen={openRecent} onRemove={projects.removeRecent ? removeRecent : undefined} />
                </>
              )}
            </section>
          ) : (
            <div key={`${projectGeneration}:${projectPath}:${dataVersion}`}>
              <KeepAliveOutlet />
            </div>
          )}
        </main>
      </div>
      <Toasts />
    </div>
  );
}
