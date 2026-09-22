// Registers the amaro TypeScript-stripping loader for `node --test`.
// Usage: node --test --import <abs-path-to-this-file> src/*.test.ts
import { register } from 'node:module';

register(new URL('./ts-hooks.mjs', import.meta.url).href);
