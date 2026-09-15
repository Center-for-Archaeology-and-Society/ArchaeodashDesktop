/**
 * Generated-from-Rust API/IPC contract types (Section 4).
 *
 * These mirror crates/contracts DTOs. Keep both sides in sync until codegen
 * (schemars -> TS) is introduced; this file is the single client source of truth.
 */

export interface AppInfo {
  app: string;
  version: string;
  transport: 'http' | 'tauri';
  ready: boolean;
}

export interface ErrorEnvelope {
  code: string;
  message: string;
}

export type JobState =
  | 'idle'
  | 'validating'
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'timed_out';
