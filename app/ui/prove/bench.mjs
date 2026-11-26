// The moviola's screenshots in an engine of choice (the screenshots of #486's conformance) and the bench of its list
// of moments (#490), by hand, outside `npm run prove`. It serves the harness under the app's CSP, as the proofs do,
// and takes the lanes of alpha in the three directions and the portfolio. The time per frame is not measured here
// any more: that is the product's bench, crates/regia-moviola-bench (`npm run bench` there, in the product's
// webview), which took the place of #486's quick check (#683).
//
//   node prove/bench.mjs --engine webview2|edge|chrome|headless --out <folder> [--host <a WebView2 app>] [--args "<more>"] [--list]
//
// webview2: an app that hosts WebView2 and does not keep its page from navigating, started with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS set to open the DevTools on a port; the renderer spike's moviola-bench.exe
// (84871ea) is one. edge and chrome: the browser in a window with its GPU. headless: the proofs' browser. The folder
// receives shots-<engine>.json, which says what drew them, and the PNGs.
//
// --list (#490): the cost of the list of moments instead (harness/bench.ts, `benchList`), at x1, x10 and x100, at the
// zoom of detail and of groups, against the conformance's bound of 1 ms; the folder receives bench-list-<engine>.json.
// It measures work in the page, so headless counts too. A WebView2 window that something covers, or a locked screen,
// draws no frame and the stage never settles: then `--args "--disable-features=CalculateNativeWinOcclusion
// --disable-backgrounding-occluded-windows --disable-renderer-backgrounding"`.
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { attach, findChrome, launch } from "./cdp.js";
import { policy, serve } from "./serve.js";

const UI = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET = resolve(UI, "../../target");
const EDGE = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const args = process.argv.slice(2);
const option = (name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const engine = option("engine") ?? "headless";
const out = option("out");
if (!out) {
  console.error("--out: the folder for the results and the screenshots");
  process.exit(2);
}
mkdirSync(out, { recursive: true });

/** A WebView2 host with its DevTools on `port`, and its first page. */
async function webview2(host, port, extra) {
  const child = spawn(host, [], {
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: [`--remote-debugging-port=${port}`, ...extra].join(" ") },
    stdio: "ignore",
  });
  for (let waited = 0; waited < 30_000; waited += 200) {
    const list = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json()).catch(() => []);
    if (list.some((entry) => entry.type === "page")) {
      return attach(port, { browser: `WebView2 in ${host}`, close: async () => void child.kill() });
    }
    await sleep(200);
  }
  child.kill();
  throw new Error(`no DevTools on port ${port} from ${host}: the host may set its own browser arguments`);
}

async function open(page, origin, route, n, ms = 90_000) {
  await page.goto(`${origin}/harness/index.html?run=${n}${route}`);
  await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(route)}`, ms);
}

async function shot(page, name) {
  const { data } = await page.send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(out, `${name}-${engine}.png`), Buffer.from(data, "base64"));
}

const server = await serve(join(TARGET, "ui-harness"), policy(join(UI, "../src-tauri/tauri.conf.json")));
const profiles = join(TARGET, "ui-prove");
const page =
  engine === "webview2"
    ? await webview2(option("host") ?? "", Number(option("port") ?? 9333), (option("args") ?? "").split(" ").filter(Boolean))
    : await launch({ width: 1300, height: 860, profiles, binary: engine === "edge" ? EDGE : findChrome(), headed: engine !== "headless" });
/** The list's bench at x1, x10 and x100: each level's costs, and whether each stays under the bound. */
async function listBench() {
  const ms = (cost) => `${cost.median.toFixed(3)}/${cost.p95.toFixed(3)}/${cost.max.toFixed(3)} (mean ${cost.mean.toFixed(4)})`;
  const results = [];
  let n = 0;
  for (const scale of [1, 10, 100]) {
    const route = `#/bench?scale=${scale}&list=1&lang=en&dir=montaggio`;
    await open(page, server.origin, route, ++n);
    const r = await page.evaluate("window.benchList");
    results.push(r);
    for (const level of r.levels) {
      // The bound is the conformance's: a frame at a still view, and a rebuild, at p95; a single sample over it is
      // reported, not judged (a collection of garbage lands where it will).
      const still = Math.max(level.still.max, level.stillNoChoice.max);
      const rebuild = Math.max(level.step.p95, level.order.p95, level.playhead.p95);
      const worst = Math.max(level.step.max, level.order.max, level.playhead.max);
      console.log(`x${scale} ${level.level} (zoom ${level.zoom.toFixed(3)}): ${level.entries} entries, ${level.groups} groups of several, ${level.options} options in the page;`
        + ` ms median/p95/max: still ${ms(level.still)}, still with none chosen ${ms(level.stillNoChoice)}, arrow ${ms(level.step)},`
        + ` order ${ms(level.order)}, playhead ${ms(level.playhead)}; ${level.builds} windows built;`
        + ` a still frame at most ${still.toFixed(4)} ms, a rebuild ${rebuild.toFixed(3)} ms at p95, under ${r.bound_ms} ms: ${still < r.bound_ms && rebuild < r.bound_ms ? "yes" : "NO"};`
        + ` the slowest single rebuild ${worst.toFixed(3)} ms`);
    }
  }
  return results;
}

/** The screenshots: alpha's lanes in the three directions, then the portfolio. */
async function screenshots() {
  let n = 0;
  for (const direction of ["montaggio", "foglio", "nero"]) {
    await open(page, server.origin, `#/moviola/alpha?data=demo&lang=en&dir=${direction}`, ++n);
    await sleep(200);
    await shot(page, `alpha-${direction}`);
  }
  await open(page, server.origin, "#/moviola?data=demo&lang=it&dir=montaggio", ++n);
  await shot(page, "portfolio");
  const host = await page.evaluate(`(() => {
    const gl = document.createElement("canvas").getContext("webgl2");
    const debug = gl && gl.getExtension("WEBGL_debug_renderer_info");
    return { userAgent: navigator.userAgent, dpr: devicePixelRatio, inner: [innerWidth, innerHeight],
      renderer: gl ? gl.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : gl.RENDERER) : null };
  })()`);
  const record = { engine, version: await page.version(), host, date: new Date().toISOString() };
  writeFileSync(join(out, `shots-${engine}.json`), JSON.stringify(record, null, 1) + "\n");
  console.log(`${engine}: ${record.version}; ${host.renderer}; dpr ${host.dpr}; window ${host.inner.join("x")}`);
}

try {
  if (args.includes("--list")) {
    const list = await listBench();
    const record = { engine, version: await page.version(), userAgent: await page.evaluate("navigator.userAgent"), date: new Date().toISOString(), results: list };
    writeFileSync(join(out, `bench-list-${engine}.json`), JSON.stringify(record, null, 1) + "\n");
    console.log(`${engine}: ${record.version}`);
  } else {
    await screenshots();
  }
  const errors = [...new Set(page.consoleErrors)];
  console.log(`errors in the console: ${errors.length}`);
  for (const error of errors.slice(0, 5)) console.log(`  ${error}`);
} finally {
  await page.close();
  await server.close();
}
