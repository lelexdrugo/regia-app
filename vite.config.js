import { defineConfig } from 'vite';

export default defineConfig({
  clearScreen: false,
  base: './',
  build: { outDir: 'dist', target: 'es2022', sourcemap: false },
  server: { port: 5173, strictPort: true },
});
