/**
 * Transport selection (Section 9.2): the Tauri IPC adapter inside the
 * desktop webview, the HTTP adapter against the Axum API elsewhere. The
 * detection key matches Tauri 2's injected `window.__TAURI_INTERNALS__`.
 *
 * Every backend call is reported to the app-wide activity tracker, which
 * drives the top bar's loading indicator.
 */
import { HttpTransport, TauriTransport, type Transport } from '@archaeodash/client';
import { activity, activityLabel } from './activity.ts';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** `METHOD /path` of a fetch call, for activity labels. */
export function requestKey(url: string, init?: RequestInit): string {
  const method = (init?.method ?? 'GET').toUpperCase();
  let path = url;
  try {
    path = new URL(url, 'http://localhost').pathname;
  } catch {
    /* keep the raw text */
  }
  return `${method} ${path}`;
}

export async function createTransport(): Promise<Transport> {
  if (isTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    const { listen } = await import('@tauri-apps/api/event');
    const tracked = <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
      activity.track(activityLabel(cmd), invoke<T>(cmd, args));
    return new TauriTransport(tracked, listen as never);
  }
  // `?demo`: synthetic in-memory project for UI review in a plain browser.
  if (typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('demo')) {
    const { demoFetch, demoEventSource } = await import('./demo/demo-fetch.ts');
    return new HttpTransport(undefined, (input, init) => activity.track(activityLabel(requestKey(input, init)), demoFetch(input, init)), demoEventSource);
  }
  return new HttpTransport(undefined, (input, init) => activity.track(activityLabel(requestKey(input, init)), fetch(input, init)));
}
