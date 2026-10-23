/**
 * Export downloads (Section 7.3): export results arrive as JSON
 * `{ file_name, media_type, content, content_encoding? }` — CSV as a string,
 * binary formats (XLSX) base64-encoded. The client turns them into a browser
 * download or a native save dialog. The formula-injection guard stays
 * server-side.
 */
import type { ExportResult } from '@archaeodash/client';
import { isTauri } from './transport.ts';

/** File bytes (or text) for a browser download. */
export function exportBlobPart(result: ExportResult): BlobPart {
  if (result.content_encoding !== 'base64') return result.content;
  const binary = atob(result.content);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** Saves an export; resolves false when the user cancels the save dialog. */
export async function downloadExportResult(result: ExportResult): Promise<boolean> {
  if (isTauri()) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<boolean>('save_export_file', { result });
  }

  const type = result.content_encoding === 'base64' ? result.media_type : `${result.media_type};charset=utf-8`;
  const blob = new Blob([exportBlobPart(result)], { type });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = result.file_name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
  return true;
}
