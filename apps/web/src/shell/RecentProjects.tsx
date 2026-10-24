/**
 * Recent projects (desktop): a start screen list shown when no project is
 * open, and a menu beside the project button. The list itself lives in the
 * desktop backend, which only reopens folders it already recorded.
 */
import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactElement } from 'react';
import type { ProjectsService, RecentProject } from '@archaeodash/client';

/** Display form of a project path (drops Windows' `\\?\` long-path prefix). */
export function displayPath(path: string): string {
  return path.replace(/^\\\\\?\\(UNC\\)?/, (_, unc: string | undefined) => (unc ? '\\\\' : ''));
}

/** "just now", "5 minutes ago", "yesterday", "3 days ago", else a date. */
export function openedAgo(unixSecs: number, nowMs: number): string {
  const seconds = Math.max(0, Math.floor(nowMs / 1000) - unixSecs);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'} ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? '' : 's'} ago`;
  const days = Math.floor(hours / 24);
  if (days === 1) return 'yesterday';
  if (days < 30) return `${days} days ago`;
  return new Date(unixSecs * 1000).toLocaleDateString();
}

/** Loads the recent list; `refresh` after any open, remove, or clear. */
export function useRecentProjects(projects?: ProjectsService): {
  recent: RecentProject[];
  refresh: () => void;
  setRecent: (next: RecentProject[]) => void;
} {
  const [recent, setRecent] = useState<RecentProject[]>([]);
  const [token, setToken] = useState(0);
  useEffect(() => {
    if (!projects?.recent) return;
    let active = true;
    projects.recent().then((list) => { if (active) setRecent(list); }).catch(() => { if (active) setRecent([]); });
    return () => { active = false; };
  }, [projects, token]);
  const refresh = useCallback(() => setToken((t) => t + 1), []);
  return { recent, refresh, setRecent };
}

export interface RecentProjectListProps {
  readonly recent: readonly RecentProject[];
  readonly busy?: boolean;
  readonly onOpen: (project: RecentProject) => void;
  readonly onRemove?: (project: RecentProject) => void;
  readonly nowMs?: number;
}

/** Start-screen list: one row per project; missing folders can be removed. */
export function RecentProjectList({ recent, busy = false, onOpen, onRemove, nowMs = Date.now() }: RecentProjectListProps): ReactElement | null {
  if (!recent.length) return null;
  return (
    <ul className="recent-list" aria-label="Recent projects">
      {recent.map((project) => (
        <li key={project.path} className={project.available ? 'recent-item' : 'recent-item is-missing'}>
          <button
            type="button"
            className="recent-open"
            disabled={busy || !project.available}
            onClick={() => onOpen(project)}
            title={project.available ? `Open ${displayPath(project.path)}` : 'This folder was moved or deleted'}
          >
            <span className="recent-name">{project.name}</span>
            <span className="recent-path">{displayPath(project.path)}</span>
            <span className="recent-when">{project.available ? `Opened ${openedAgo(project.last_opened_unix_secs, nowMs)}` : 'Folder not found'}</span>
          </button>
          {onRemove && (
            <button type="button" className="recent-remove" disabled={busy} aria-label={`Remove ${project.name} from recent projects`} title="Remove from list" onClick={() => onRemove(project)}>
              ×
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

export interface ProjectMenuProps {
  readonly recent: readonly RecentProject[];
  readonly currentPath?: string;
  readonly busy?: boolean;
  readonly onOpenRecent: (project: RecentProject) => void;
  readonly onBrowse: () => void;
  readonly onClear?: () => void;
  /** Opens the current project folder in the system file manager. */
  readonly onReveal?: () => void;
}

/** "Show in File Explorer" / "Show in Finder" / "Open project folder". */
export function revealLabel(userAgent: string = typeof navigator === 'undefined' ? '' : navigator.userAgent): string {
  if (/Windows/i.test(userAgent)) return 'Show in File Explorer';
  if (/Mac OS X|Macintosh/i.test(userAgent)) return 'Show in Finder';
  return 'Open project folder';
}

/** "▾" menu beside the project button: recent projects, browse, clear. */
export function ProjectMenu({ recent, currentPath = '', busy = false, onOpenRecent, onBrowse, onClear, onReveal }: ProjectMenuProps): ReactElement {
  const [open, setOpen] = useState(false);
  const holder = useRef<HTMLDivElement | null>(null);
  const button = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (holder.current && !holder.current.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    // Focus the first item so the menu works from the keyboard.
    holder.current?.querySelector<HTMLButtonElement>('[role="menuitem"]:not([disabled])')?.focus();
    return () => document.removeEventListener('mousedown', close);
  }, [open]);
  const choose = (action: () => void) => { setOpen(false); action(); };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') { setOpen(false); button.current?.focus(); return; }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    const items = [...(holder.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not([disabled])') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    items[(at + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
  };
  return (
    <div className="project-menu" ref={holder} onKeyDown={onKeyDown}>
      <button
        ref={button}
        type="button"
        className="project-menu-button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Recent projects"
        title="Recent projects"
        disabled={busy}
        onClick={() => setOpen((v) => !v)}
      >
        ▾
      </button>
      {open && (
        <div className="project-menu-list" role="menu" aria-label="Recent projects">
          <p className="project-menu-heading" role="presentation">Recent projects</p>
          {!recent.length && <p className="project-menu-empty" role="presentation">No recent projects yet.</p>}
          {recent.map((project) => {
            const current = project.path === currentPath;
            return (
              <button
                key={project.path}
                type="button"
                role="menuitem"
                className="project-menu-item"
                disabled={!project.available || current}
                title={project.available ? displayPath(project.path) : 'Folder not found'}
                onClick={() => choose(() => onOpenRecent(project))}
              >
                <span className="recent-name">{project.name}{current ? ' (open)' : ''}</span>
                <span className="recent-path">{project.available ? displayPath(project.path) : 'Folder not found'}</span>
              </button>
            );
          })}
          <hr />
          {onReveal && currentPath && (
            <button type="button" role="menuitem" className="project-menu-item" onClick={() => choose(onReveal)}>
              {revealLabel()}
            </button>
          )}
          <button type="button" role="menuitem" className="project-menu-item" onClick={() => choose(onBrowse)}>
            Browse for a folder…
          </button>
          {onClear && recent.length > 0 && (
            <button type="button" role="menuitem" className="project-menu-item" onClick={() => choose(onClear)}>
              Clear recent list
            </button>
          )}
        </div>
      )}
    </div>
  );
}
