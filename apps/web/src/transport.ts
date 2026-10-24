/**
 * Transport selection (Section 9.2): the Tauri IPC adapter inside the
 * desktop webview, the HTTP adapter against the Axum API elsewhere. The
 * detection key matches Tauri 2's injected `window.__TAURI_INTERNALS__`.
 *
 * Every backend call is reported to the app-wide activity tracker, which
 * drives the top bar's loading indicator.
 */
import { HttpTransport, TauriTransport, type Transport } from '@archaeodash/client';
import { activity, activityLabel, JobActivity, jobFinishedNotice, returnsJobSnapshot } from './activity.ts';
import { notify } from './ui/toast-store.ts';

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

/**
 * Analysis jobs run in the background after submit returns, so they are
 * counted as activity from the job snapshots the analysis pages poll, with
 * a notification when one finishes (useful after navigating away).
 */
const jobActivity = new JobActivity(activity, (job, elapsedMs) => {
  const notice = jobFinishedNotice(job, elapsedMs);
  if (notice) notify(notice);
});

/** Feeds job snapshots from an HTTP response to the job tracker. */
function observeJobResponse(key: string, response: Promise<Response>): Promise<Response> {
  if (returnsJobSnapshot(key)) {
    void response
      .then((res) => (res.ok ? res.clone().json() : undefined))
      .then((body: unknown) => jobActivity.observe(body))
      .catch(() => undefined);
  }
  return response;
}

export async function createTransport(): Promise<Transport> {
  if (isTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    const { listen } = await import('@tauri-apps/api/event');
    const tracked = <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      const call = activity.track(activityLabel(cmd), invoke<T>(cmd, args));
      if (returnsJobSnapshot(cmd)) void call.then((result) => jobActivity.observe(result), () => undefined);
      return call;
    };
    return new TauriTransport(tracked, listen as never);
  }
  type Send = (input: string, init?: RequestInit) => Promise<Response>;
  const trackedFetch = (send: Send): Send => (input, init) => {
    const key = requestKey(input, init);
    return observeJobResponse(key, activity.track(activityLabel(key), send(input, init)));
  };
  // `?demo`: synthetic in-memory project for UI review in a plain browser.
  if (typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('demo')) {
    const { demoFetch, demoEventSource } = await import('./demo/demo-fetch.ts');
    return new HttpTransport(undefined, trackedFetch(demoFetch), demoEventSource);
  }
  return new HttpTransport(undefined, trackedFetch((input, init) => fetch(input, init)));
}
