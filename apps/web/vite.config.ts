import { defineConfig } from 'vite';

export default defineConfig({
  build: { outDir: 'dist', sourcemap: true },
  server: {
    proxy: {
      // Dev proxy to the Axum API (crates/api); desktop uses Tauri IPC instead.
      '/api': { target: 'http://127.0.0.1:8787', changeOrigin: true },
    },
  },
});
