/**
 * Design tokens for the three ArchaeoDash themes, Section 9.3 ("Preserve
 * Simple, Light, and Dark themes"). Hex values mirror the legacy Shiny
 * stylesheet (inst/app/www/styles.css): `--bg`, `--text`, `--accent`
 * (gradient start), `--accent-2` (gradient end), `--btn-text`, plus the
 * sidebar gradient pair. `simple` is the legacy flat/low-contrast fallback.
 */
export interface ThemeTokens {
  readonly name: 'simple' | 'light' | 'dark';
  readonly background: string;
  readonly foreground: string;
  readonly accent: string;
  readonly accent2: string;
  readonly buttonText: string;
  readonly sidebarGradient: readonly [string, string];
  readonly backgroundGradient: readonly [string, string];
  readonly muted: string;
  readonly panel: string;
}

export const themes: readonly ThemeTokens[] = [
  {
    name: 'light',
    background: '#eef6f3',
    foreground: '#163126',
    accent: '#1e8f66',
    accent2: '#48c895',
    buttonText: '#f4fffa',
    sidebarGradient: ['#b9ebd4', '#9ad6bc'],
    backgroundGradient: ['#f4fbf8', '#deefe8'],
    muted: '#3d5f53',
    panel: 'rgba(255,255,255,0.74)',
  },
  {
    name: 'simple',
    background: '#eef3ef',
    foreground: '#233028',
    accent: '#5f866f',
    accent2: '#7ca18a',
    buttonText: '#ffffff',
    sidebarGradient: ['#edf4ef', '#e2ece5'],
    backgroundGradient: ['#f5f8f5', '#e6eee7'],
    muted: '#4a5f54',
    panel: '#ffffff',
  },
  {
    name: 'dark',
    background: '#0f1b1e',
    foreground: '#dbf6ea',
    accent: '#47c997',
    accent2: '#66e3b3',
    buttonText: '#032217',
    sidebarGradient: ['#163730', '#0f2a24'],
    backgroundGradient: ['#13242a', '#0a1216'],
    muted: '#9fc8b9',
    panel: 'rgba(18,31,37,0.82)',
  },
];

export type ThemeName = ThemeTokens['name'];

/** Legacy JS fallback (`getStoredTheme`): unknown values fall back to simple. */
export function normalizeTheme(value: unknown): ThemeName {
  return value === 'light' || value === 'dark' || value === 'simple' ? value : 'simple';
}

/** CSS custom properties for one theme, ready for a `style` attribute. */
export function themeCssVariables(theme: ThemeTokens): Record<string, string> {
  return {
    '--bg': theme.background,
    '--text': theme.foreground,
    '--accent': theme.accent,
    '--accent-2': theme.accent2,
    '--btn-text': theme.buttonText,
    '--side-bg-1': theme.sidebarGradient[0],
    '--side-bg-2': theme.sidebarGradient[1],
    '--bg-grad-1': theme.backgroundGradient[0],
    '--bg-grad-2': theme.backgroundGradient[1],
    '--muted': theme.muted,
    '--panel': theme.panel,
  };
}

export const themeStorageKey = 'archaeodash_theme';
