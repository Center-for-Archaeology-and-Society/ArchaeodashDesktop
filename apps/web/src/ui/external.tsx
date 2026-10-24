/**
 * Links and windows that leave the main view. In the desktop app external
 * links must open in the system browser (the opener plugin, limited to
 * https URLs by the capability), never navigate the app's webview; in a
 * browser they open a new tab.
 */
import type { MouseEvent, ReactElement, ReactNode } from 'react';
import { isTauri } from '../transport.ts';
import { errorText, notify } from './toast-store.ts';

async function invokeDesktop(command: string, args?: Record<string, unknown>): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke(command, args);
}

/** True for the absolute https URLs external links may open. */
export function isExternalUrl(href: string): boolean {
  return /^https:\/\/[^\s/]+/i.test(href);
}

/** Opens an https URL in the system browser (desktop) or a new tab (web). */
export async function openExternal(url: string): Promise<void> {
  if (!isExternalUrl(url)) throw new Error(`not an https link: ${url}`);
  if (isTauri()) {
    await invokeDesktop('plugin:opener|open_url', { url });
    return;
  }
  window.open(url, '_blank', 'noopener,noreferrer');
}

/** Click handler body for external links: open outside, toast on failure. */
export function followExternal(url: string): void {
  void openExternal(url).catch((error: unknown) => notify({ kind: 'error', text: `Could not open the link: ${errorText(error)}` }));
}

/** Route of the standalone help view (no shell, no transport). */
export const HELP_WINDOW_PATH = '/help-window';

/** Opens Help on its own: a desktop window, or a browser window on the web. */
export async function openHelpWindow(): Promise<void> {
  if (isTauri()) {
    await invokeDesktop('open_help_window');
    return;
  }
  window.open(HELP_WINDOW_PATH, 'archaeodash-help', 'width=900,height=800,resizable=yes,scrollbars=yes');
}

/** Desktop only: opens the app's log folder in the system file manager. */
export async function revealLogFolder(): Promise<void> {
  await invokeDesktop('reveal_log_folder');
}

/** An https link that opens outside the app. */
export function ExternalLink({ href, children }: { href: string; children: ReactNode }): ReactElement {
  const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    followExternal(href);
  };
  return (
    <a href={href} target="_blank" rel="noopener noreferrer" onClick={onClick}>
      {children}
    </a>
  );
}
