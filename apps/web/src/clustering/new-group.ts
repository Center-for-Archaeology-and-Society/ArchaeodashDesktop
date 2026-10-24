/**
 * Naming a group that an assignment creates: the default file path beside
 * the analyzed groups, and the checks that keep a new group from landing
 * on an existing one (the backend would otherwise merge the units into
 * the existing file).
 */
import type { GroupCandidate } from '@archaeodash/client';

/** A project-relative path that stays inside the project. */
export function safePath(path: string): boolean {
  return !!path.trim() && !path.includes('\\') &&!path.includes('\0') && !path.startsWith('/') &&
    !/^[a-zA-Z]:/.test(path) && path.split('/').every(part => part !== '' && part !== '.' && part !== '..');
}

/** Default file for a new group named `name`, in the folder of `sourcePath`. */
export function newGroupPath(sourcePath: string, name: string, fallbackIndex = 1): string {
  // Beside the analyzed groups (Windows paths use backslashes).
  const cut = Math.max(sourcePath.lastIndexOf('/'), sourcePath.lastIndexOf('\\'));
  const folder = sourcePath.slice(0, cut + 1);
  return `${folder}${name.trim().replace(/[^A-Za-z0-9._-]/g, '_') || `group_${fallbackIndex}`}.parquet`;
}

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** Why a new group cannot use this name and path, or null when it can. */
export function newGroupProblem(name: string, path: string, candidates: readonly GroupCandidate[]): string | null {
  if (!name.trim()) return 'Enter a name for the new group.';
  if (!safePath(path)) return 'Choose a safe, project-relative file for the new group.';
  const named = candidates.find(c => c.group?.group_name !== undefined && same(c.group.group_name, name));
  if (named) return `A group named “${named.group!.group_name}” already exists; choose it from the list or use another name.`;
  if (candidates.some(c => same(c.path, path))) return 'That file already exists; choose a path unused by every group candidate.';
  return null;
}
