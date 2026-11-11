import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

// The harness: the app's own components on fixtures, built as the app is and served under the app's CSP by
// prove/serve.js, for the proofs of prove/ (README.md). It never ships: it is written to the workspace's
// target/, beside Cargo's output, and the app's build reads only dist/.
export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: "../../target/ui-harness",
    emptyOutDir: true,
    target: ["es2022", "chrome120", "safari17"],
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    sourcemap: false,
    rollupOptions: { input: "harness/index.html" },
  },
});
