/**
 * Group folders: each folder of group files is one grouping of the samples
 * (the backend refuses to put a sample in a folder twice). Analyses use one
 * folder at a time, so copies of the same samples in other folders are never
 * counted twice.
 */

/** Folder of a project-relative path ('/'-separated; '' = project root). */
export function folderOf(path: string): string {
  const normalized = path.replace(/\\/g, '/');
  const slash = normalized.lastIndexOf('/');
  return slash < 0 ? '' : normalized.slice(0, slash);
}

/** Display name of a folder. */
export function folderLabel(folder: string): string {
  if (!folder) return 'Project root';
  return folder.slice(folder.lastIndexOf('/') + 1);
}

/** Distinct folders of `paths`, sorted. */
export function foldersOf(paths: readonly string[]): string[] {
  return [...new Set(paths.map(folderOf))].sort((a, b) => a.localeCompare(b));
}

/**
 * The folder analyses use: the chosen one while it still has groups,
 * otherwise the first folder that has any.
 */
export function activeFolderAmong(paths: readonly string[], chosen: string | null | undefined): string | null {
  const folders = foldersOf(paths);
  if (chosen !== null && chosen !== undefined && folders.includes(chosen)) return chosen;
  return folders[0] ?? null;
}

/** Paths in `folder`. */
export function inFolder(paths: readonly string[], folder: string | null): string[] {
  return folder === null ? [...paths] : paths.filter((p) => folderOf(p) === folder);
}

/**
 * Folder name for imported groups: the group column, made safe for every
 * platform (no path separators or reserved characters, no leading dot).
 */
export function folderNameForColumn(column: string, fallback = 'groups'): string {
  const cleaned = column
    .replace(/[<>:"/\\|?*\u0000-\u001f]+/g, '_')
    .trim()
    .replace(/^[.\s]+/, '')
    .replace(/[.\s]+$/, '')
    .slice(0, 100);
  if (!cleaned) return fallback;
  const stem = cleaned.split('.')[0]!.toUpperCase();
  const reserved = ['CON', 'PRN', 'AUX', 'NUL'].includes(stem) || /^(COM|LPT)\d$/.test(stem) || cleaned.toLowerCase() === 'sources';
  return reserved ? `${cleaned}_groups` : cleaned;
}

/** Replaces the exclusions inside `scope`, keeping those of other folders. */
export function exclusionWithin(current: ReadonlySet<string>, scope: readonly string[], included: readonly string[]): string[] {
  const inScope = new Set(scope);
  return [...[...current].filter((p) => !inScope.has(p)), ...scope.filter((p) => !included.includes(p))];
}
