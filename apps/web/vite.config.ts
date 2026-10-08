import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// The hosted API port defaults to 8787 (Section 10 deployment default);
// AUTH_API_PORT overrides it for local e2e runs on a busy port.
const apiTarget = `http://127.0.0.1:${process.env.AUTH_API_PORT ?? '8787'}`;

export default defineConfig({
  plugins: [react()],
  build: { outDir: 'dist', sourcemap: true },
  server: {
    proxy: {
      // Axum serves /healthz at the root and the API under /api/v1.
      '/api': { target: apiTarget, changeOrigin: true },
      '/healthz': { target: apiTarget, changeOrigin: true },
    },
  },
});
