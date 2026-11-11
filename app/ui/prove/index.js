// The frontend's proofs (README.md): the contrast of the colour tokens, the catalog in a pseudo-locale, the
// limits of every view and state, and V07 on the DOM. Each one checks the app, then a negative control of its
// own, and fails if the app fails or if the control passes. `npm run prove` builds the harness first.
//   node prove/index.js [contrast] [catalog] [limits] [v07]
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { catalog } from "./catalog.js";
import { launch } from "./cdp.js";
import { contrast } from "./contrast.js";
import { limits } from "./limits.js";
import { policy, serve } from "./serve.js";
import { v07 } from "./v07.js";

const UI = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET = resolve(UI, "../../target");
const PROOFS = { contrast, catalog, limits, v07 };

const chosen = process.argv.length > 2 ? process.argv.slice(2) : Object.keys(PROOFS);
const results = [];
let server;
let page;
try {
  for (const name of chosen) {
    const proof = PROOFS[name];
    if (!proof) {
      throw new Error(`no proof named ${name}: ${Object.keys(PROOFS).join(", ")}`);
    }
    if (proof.browser && !page) {
      server = await serve(join(TARGET, "ui-harness"), policy(join(UI, "../src-tauri/tauri.conf.json")));
      page = await launch({ width: 1440, height: 900, profiles: join(TARGET, "ui-prove") });
      console.log(`browser: ${await page.version()} (${page.browser})`);
    }
    console.log(`\n== ${name}`);
    const started = Date.now();
    const ok = await proof.run({ ui: UI, page, server, harness: `${server?.origin}/harness/index.html` });
    results.push([name, ok, Date.now() - started]);
  }
} finally {
  await page?.close();
  await server?.close();
}
console.log("");
for (const [name, ok, ms] of results) console.log(`${ok ? "ok  " : "FAIL"} ${name} (${(ms / 1000).toFixed(1)} s)`);
if (results.length !== chosen.length || results.some(([, ok]) => !ok)) {
  process.exitCode = 1;
}
