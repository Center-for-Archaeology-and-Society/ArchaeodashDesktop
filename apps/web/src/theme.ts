/**
 * Theme state (Section 9.3): three legacy themes, stored locally, with the
 * server preference applied after hydration so the stored choice always
 * paints first (no legacy startup race that sent the default before the
 * saved preference was read).
 */
import { normalizeTheme, themeCssVariables, themes, themeStorageKey, type ThemeName, type Transport } from '@archaeodash/client';

export type { ThemeName };
export { themes, normalizeTheme, themeStorageKey };

/** Legacy JS fallback (`getStoredTheme`): unknown values fall back to simple. */
export function readStoredTheme(): ThemeName {
  try {
    return normalizeTheme(window.localStorage.getItem(themeStorageKey));
  } catch {
    return 'simple';
  }
}

function tokensFor(theme: ThemeName) {
  return themes.find((t) => t.name === theme) ?? themes[0]!;
}

/** Applies the theme to the document root and persists it. */
export function applyTheme(theme: ThemeName): void {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(themeCssVariables(tokensFor(theme)))) {
    root.style.setProperty(key, value);
  }
  root.dataset.theme = theme;
  try {
    window.localStorage.setItem(themeStorageKey, theme);
  } catch {
    // Private-mode/localStorage denial: the in-memory choice still applies.
  }
}

/**
 * Hydration: ask the backend for the persisted theme preference; when present
 * it wins over the local choice (server preference after hydration), matching
 * the legacy `theme_preference` restore message.
 */
export async function hydrateTheme(transport: Transport, current: ThemeName): Promise<ThemeName> {
  try {
    const { preferences } = await transport.preferences.get();
    const entry = preferences.find((p) => p.key === 'theme');
    if (entry === undefined) return current;
    const serverTheme = normalizeTheme(entry.value);
    if (serverTheme !== current) applyTheme(serverTheme);
    return serverTheme;
  } catch {
    return current;
  }
}
