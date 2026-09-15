/**
 * Transport abstraction (Section 9): the client never knows whether it talks
 * to the Axum HTTP API or Tauri IPC. One adapter per delivery mode.
 */
import type { AppInfo, ErrorEnvelope } from '@archaeodash/contracts';

export interface Transport {
  appInfo(): Promise<AppInfo>;
}

export class TransportError extends Error {
  readonly envelope: ErrorEnvelope;

  constructor(envelope: ErrorEnvelope) {
    super(envelope.message);
    this.name = 'TransportError';
    this.envelope = envelope;
  }
}

/** HTTP adapter for the hosted web client. */
export class HttpTransport implements Transport {
  private readonly baseUrl: string;

  constructor(baseUrl: string = '') {
    this.baseUrl = baseUrl;
  }

  async appInfo(): Promise<AppInfo> {
    const res = await fetch(`${this.baseUrl}/healthz`);
    if (!res.ok) {
      throw new TransportError({ code: `http_${res.status}`, message: res.statusText });
    }
    return (await res.json()) as AppInfo;
  }
}
