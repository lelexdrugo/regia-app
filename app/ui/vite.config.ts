import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

// The app serves `dist` from its own origin under a strict CSP (app/src-tauri/tauri.conf.json): every
// script, style and asset must be a file, and nothing may come from another origin.
export default defineConfig({
  plugins: [svelte()],
  // The catalogs live in the Rust crate the CLI uses too, outside this folder.
  server: { fs: { allow: [".", "../../crates/regia-i18n/locales"] } },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // WebView2, WKWebView on macOS 15, WebKitGTK 2.4x.
    target: ["es2022", "chrome120", "safari17"],
    // A small asset would become a data: URL, which img-src and font-src refuse.
    assetsInlineLimit: 0,
    // The polyfill is a script the CSP has no need to allow; the three engines support modulepreload.
    modulePreload: { polyfill: false },
    sourcemap: false,
  },
});
