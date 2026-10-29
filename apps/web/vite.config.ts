import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// The hosted API port defaults to 8787 (Section 10 deployment default);
// AUTH_API_PORT overrides it for local e2e runs on a busy port.
const apiTarget = `http://127.0.0.1:${process.env.AUTH_API_PORT ?? '8787'}`;

// ARCHAEODASH_BASE_PATH builds the client for a sub-path behind a reverse
// proxy (for example `/archaeodash`); the default is the site root.
const basePath = (process.env.ARCHAEODASH_BASE_PATH ?? '').replace(/\/+$/, '');
if (basePath && !/^\/[A-Za-z0-9._~-]+(\/[A-Za-z0-9._~-]+)*$/.test(basePath)) {
  throw new Error(`ARCHAEODASH_BASE_PATH must look like /archaeodash, not ${JSON.stringify(basePath)}`);
}

export default defineConfig({
  base: `${basePath}/`,
  plugins: [react()],
  build: { outDir: 'dist', sourcemap: true },
  server: {
    proxy: {
      // Axum serves /healthz at the root and the API under /api/v1; the
      // hosted server serves each project's analysis API under /w/<id>/.
      '/api': { target: apiTarget, changeOrigin: true },
      '/healthz': { target: apiTarget, changeOrigin: true },
      '/w/': { target: apiTarget, changeOrigin: true },
    },
  },
});
