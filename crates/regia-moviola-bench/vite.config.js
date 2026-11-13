import { defineConfig } from 'vite';

// The page is served under the app's own CSP (scripts/serve.mjs), so it is built as app/ui builds the app:
// every script, style and font a file, no data: URL, no modulepreload polyfill.
export default defineConfig({
  clearScreen: false,
  logLevel: 'warn',
  base: './',
  // app/ui's modules import @fluent/bundle: it resolves from this folder, so app/ui needs no npm ci of its own.
  resolve: { dedupe: ['@fluent/bundle'] },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: ['es2022', 'chrome120', 'safari17'],
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    sourcemap: false,
  },
});
