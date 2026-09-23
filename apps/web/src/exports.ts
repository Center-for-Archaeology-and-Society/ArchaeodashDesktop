/**
 * Export downloads (Section 7.3): export results arrive as JSON
 * `{ file_name, media_type, content }` (CSV as a string); the client turns
 * them into a browser download. The formula-injection guard stays server-side.
 */
import type { ExportResult } from '@archaeodash/client';

export function downloadExportResult(result: ExportResult): void {
  const blob = new Blob([result.content], { type: `${result.media_type};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = result.file_name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}
