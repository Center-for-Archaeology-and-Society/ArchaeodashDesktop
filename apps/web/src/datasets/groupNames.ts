/**
 * Rules every new or renamed group name follows (the backend's
 * `validate_group_name`): a group name also names its file, so it must be a
 * portable file name on Windows, macOS and Linux.
 */

/** A character no group or folder name may contain (Windows, macOS, Linux), control characters included. */
export const invalidNameChar = (c: string): boolean => {
  const code = c.codePointAt(0) ?? 0;
  return code < 0x20 || (code >= 0x7f && code <= 0x9f) || '<>:"/\\|?*'.includes(c);
};

/** File name stem the backend gives a group name (`sanitize_group_name`). */
export function groupFileStem(name: string): string {
  return name.replace(/[^\p{L}\p{N}._-]/gu, '_') || 'group';
}

/** A name Windows reserves for devices (`CON`, `COM1`, `LPT3.txt`, ...). */
export function windowsReserved(name: string): boolean {
  const stem = name.toUpperCase().split('.')[0] ?? '';
  return /^(CON|PRN|AUX|NUL|COM\d|LPT\d)$/.test(stem);
}

/**
 * Why `name` cannot be a group name ('' when it can): not empty, at most 100
 * characters, not hidden, no characters invalid in file names, and no group
 * file Windows reserves. Whether another group already has it is the
 * caller's check.
 */
export function portableGroupNameProblem(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return 'Enter a group name.';
  if ([...trimmed].length > 100) return 'A group name can have at most 100 characters.';
  if (trimmed.startsWith('.')) return 'A group name cannot start with a dot.';
  const bad = [...new Set([...trimmed].filter(invalidNameChar))];
  if (bad.length) {
    const shown = bad.map((c) => ((c.codePointAt(0) ?? 0) < 0x20 ? 'control characters' : c)).join(' ');
    return `A group name cannot contain ${shown} (not allowed in file names: < > : " / \\ | ? *).`;
  }
  if (windowsReserved(groupFileStem(trimmed))) return `“${trimmed}” would give a group file name that Windows reserves.`;
  return '';
}
