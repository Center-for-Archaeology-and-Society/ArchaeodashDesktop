/** Design tokens for the three themes (light/dark/high-contrast), Section 9. */
export interface ThemeTokens {
  readonly name: string;
  readonly background: string;
  readonly foreground: string;
  readonly accent: string;
}

export const themes: readonly ThemeTokens[] = [
  { name: 'light', background: '#ffffff', foreground: '#1a1a1a', accent: '#2563eb' },
  { name: 'dark', background: '#111318', foreground: '#e6e6e6', accent: '#60a5fa' },
  { name: 'high-contrast', background: '#000000', foreground: '#ffffff', accent: '#ffff00' },
];
