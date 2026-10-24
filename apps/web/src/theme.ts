/**
 * Theme state (Section 9.3): three legacy themes, stored locally. Hosted:
 * the server preference is applied after hydration so the stored choice
 * always paints first (no legacy startup race that sent the default before
 * the saved preference was read). Desktop: local storage only.
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

/**
 * Applies the theme to the document root and persists it. Colors come from
 * the stylesheet's tokens for `html[data-theme]`; inline variables set by
 * older builds are cleared so they cannot override them.
 */
export function applyTheme(theme: ThemeName): void {
  const root = document.documentElement;
  for (const key of Object.keys(themeCssVariables(tokensFor(theme)))) {
    root.style.removeProperty(key);
  }
  root.dataset.theme = theme;
  try {
    window.localStorage.setItem(themeStorageKey, theme);
  } catch {
    // Private-mode/localStorage denial: the in-memory choice still applies.
  }
}

/**
 * Whether the theme is synced with the backend. The desktop app keeps the
 * choice in local storage only (Section 9.3): its preferences live inside
 * each project folder, so a per-project copy would go stale and repaint the
 * app on every reload or project switch.
 */
export function themeSyncsWithBackend(transport: Pick<Transport, 'kind'>): boolean {
  return transport.kind !== 'tauri';
}

/** Saves a theme choice to the backend preference (hosted only). */
export async function saveThemePreference(transport: Pick<Transport, 'kind' | 'preferences'>, theme: ThemeName): Promise<void> {
  if (!themeSyncsWithBackend(transport)) return;
  try {
    await transport.preferences.put('theme', theme);
  } catch {
    /* offline: the local choice still stands */
  }
}

/**
 * Hydration (hosted): ask the backend for the persisted theme preference;
 * when present it wins over the local choice (server preference after
 * hydration), matching the legacy `theme_preference` restore message. The
 * desktop app keeps the local choice.
 */
export async function hydrateTheme(transport: Pick<Transport, 'kind' | 'preferences'>, current: ThemeName): Promise<ThemeName> {
  if (!themeSyncsWithBackend(transport)) return current;
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
