// Module-hooks loader that strips TypeScript types with amaro (the same
// stripper Node's built-in --experimental-strip-types uses). Needed because
// some Node distributions are compiled without TypeScript support
// (ERR_NO_TYPESCRIPT), so `node --experimental-strip-types` fails there.
// Registered by scripts/ts-register.mjs.
//
// `.tsx` files additionally go through esbuild (a vite dependency, already in
// the lockfile) with the automatic JSX runtime, because type stripping alone
// cannot transform JSX elements.
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { transformSync } from 'amaro';

let esbuild = null;
function jsxTransform(source, loader) {
  if (esbuild === null) {
    // Lazy: only .tsx imports pay for the resolution.
    esbuild = import('esbuild');
  }
  return esbuild.then((mod) => mod.transformSync(source, { loader, jsx: 'automatic' }).code);
}

export async function load(url, context, next) {
  // Vite-style `?raw` imports (markdown content): expose the file text as the
  // default export.
  if (url.endsWith('.md') || url.endsWith('.md?raw')) {
    const path = fileURLToPath(url.replace(/\?raw$/, ''));
    const source = await readFile(path, 'utf8');
    return { format: 'module', source: `export default ${JSON.stringify(source)};`, shortCircuit: true };
  }
  if (url.endsWith('.tsx')) {
    const source = await readFile(fileURLToPath(url), 'utf8');
    const code = await jsxTransform(source, 'tsx');
    return { format: 'module', source: code, shortCircuit: true };
  }
  if (url.endsWith('.ts')) {
    const source = await readFile(fileURLToPath(url), 'utf8');
    const { code } = transformSync(source, { sourceMap: false });
    return { format: 'module', source: code, shortCircuit: true };
  }
  return next(url, context);
}
