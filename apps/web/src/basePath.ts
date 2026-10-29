/**
 * Where the web client is served: `''` at the site root (the default and the
 * desktop app), or a sub-path such as `/archaeodash` when built with
 * `ARCHAEODASH_BASE_PATH` (Vite `base`) to sit behind a reverse proxy. API
 * calls, the router and files from `public/` all start with it.
 */
// `import.meta.env` exists only in Vite builds; tests run without it.
export const BASE_PATH = ((import.meta.env as ImportMetaEnv | undefined)?.BASE_URL ?? '/').replace(/\/+$/, '');

/** URL of a file in `public/` (icons, logos) under the base path. */
export function publicUrl(name: string): string {
  return `${BASE_PATH}/${name}`;
}
