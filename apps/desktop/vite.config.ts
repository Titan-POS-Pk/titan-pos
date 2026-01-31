import { defineConfig } from 'vite';
import solidPlugin from 'vite-plugin-solid';
import { resolve } from 'path';

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [solidPlugin()],

  // Vite options tailored for Tauri development
  // https://v2.tauri.app/start/frontend/vite/
  clearScreen: false,

  server: {
    // Tauri expects a fixed port, fail if that port is not available
    port: 5173,
    strictPort: true,
    // Mobile development
    host: '0.0.0.0',
    hmr: {
      protocol: 'ws',
      host: 'localhost',
      port: 5173,
    },
    watch: {
      // Tell vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },

  // Path aliases for cleaner imports
  resolve: {
    alias: {
      '@': resolve(__dirname, './src'),
    },
  },

  // Build options
  build: {
    // Tauri uses Chromium on Windows and WebKit on macOS and Linux
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari14',
    // Don't minify for debug builds
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    // Produce sourcemaps for debug builds
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },

  // Environment variable prefix for client-side access
  envPrefix: ['VITE_', 'TAURI_ENV_'],
});
