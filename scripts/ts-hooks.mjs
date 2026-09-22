// Module-hooks loader that strips TypeScript types with amaro (the same
// stripper Node's built-in --experimental-strip-types uses). Needed because
// some Node distributions are compiled without TypeScript support
// (ERR_NO_TYPESCRIPT), so `node --experimental-strip-types` fails there.
// Registered by scripts/ts-register.mjs; erasable-only TS syntax supported.
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { transformSync } from 'amaro';

export async function load(url, context, next) {
  if (url.endsWith('.ts')) {
    const source = await readFile(fileURLToPath(url), 'utf8');
    const { code } = transformSync(source, { sourceMap: false });
    return { format: 'module', source: code, shortCircuit: true };
  }
  return next(url, context);
}
