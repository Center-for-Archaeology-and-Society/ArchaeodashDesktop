/**
 * Transport selection (Section 9.2): the Tauri IPC adapter inside the
 * desktop webview, the HTTP adapter against the Axum API elsewhere. The
 * detection key matches Tauri 2's injected `window.__TAURI_INTERNALS__`.
 */
import { HttpTransport, TauriTransport, type Transport } from '@archaeodash/client';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export async function createTransport(): Promise<Transport> {
  if (isTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    return new TauriTransport(invoke as never);
  }
  return new HttpTransport();
}
