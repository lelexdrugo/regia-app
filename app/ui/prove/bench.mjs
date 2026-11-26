// The moviola's quick check of the time per frame, and its screenshots (#486, Acceptance 1 and the screenshots of
// its conformance): by hand, outside `npm run prove`, on an engine with its GPU and its own window, since a
// headless browser without GPU says nothing about frames. It serves the harness under the app's CSP, as the proofs
// do, runs harness/bench.ts at x1 and x10 (the spike's harness on the product's stage), then takes the lanes of
// alpha in the three directions, the portfolio and the x10 stage.
//
//   node prove/bench.mjs --engine webview2|edge|chrome|headless --out <folder> [--host <a WebView2 app>] [--args "<more>"] [--list]
//
// webview2: an app that hosts WebView2 and does not keep its page from navigating, started with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS set to open the DevTools on a port; the renderer spike's moviola-bench.exe
// (84871ea) is one. edge and chrome: the browser in a window with its GPU. headless: the proofs' browser, for the
// screenshots only. The folder receives bench-<engine>.json and the PNGs.
//
// --list (#490): the cost of the list of moments instead (harness/bench.ts, `benchList`), at x1, x10 and x100, at the
// zoom of detail and of groups, against the conformance's bound of 1 ms; the folder receives bench-list-<engine>.json.
// It measures work in the page, so headless counts too.
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
      const worst = Math.max(level.still.max, level.stillNoChoice.max, level.step.max, level.order.max, level.playhead.max);
      console.log(`x${scale} ${level.level} (zoom ${level.zoom.toFixed(3)}): ${level.entries} entries, ${level.groups} groups of several, ${level.options} options in the page;`
        + ` ms median/p95/max: still ${ms(level.still)}, still with none chosen ${ms(level.stillNoChoice)}, arrow ${ms(level.step)},`
        + ` order ${ms(level.order)}, playhead ${ms(level.playhead)}; ${level.builds} windows built; worst ${worst.toFixed(3)} ms, under ${r.bound_ms} ms: ${worst < r.bound_ms ? "yes" : "NO"}`);
    }
  }
  return results;
}

/** The time per frame at x1 and x10 (not headless), then the screenshots. */
async function frameBench() {
  let n = 0;
  const results = [];
  if (engine !== "headless") {
    for (const scale of [1, 10]) {
      const route = `#/bench?scale=${scale}&lang=en&dir=montaggio`;
      await open(page, server.origin, route, ++n);
      results.push(await page.evaluate("window.benchResult"));
      if (scale === 10) await shot(page, "bench-x10");
      const r = results.at(-1);
      console.log(`x${scale}: ${r.milestones} milestones, ${r.aggregates} aggregates, ${r.lanes} lanes; display ${r.vsync_ms.toFixed(1)} ms;`
        + ` frame median ${r.all.dt.median.toFixed(1)} ms, p95 ${r.all.dt.p95.toFixed(1)} ms, script p95 ${r.all.js.p95.toFixed(2)} ms, late ${r.all.dropped_pct.toFixed(1)} %`);
    }
  }
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
  const record = { engine, version: await page.version(), host, date: new Date().toISOString(), results };
  writeFileSync(join(out, `bench-${engine}.json`), JSON.stringify(record, null, 1) + "\n");
  console.log(`${engine}: ${record.version}; ${host.renderer}; dpr ${host.dpr}; window ${host.inner.join("x")}`);
}

try {
  if (args.includes("--list")) {
    const list = await listBench();
    const record = { engine, version: await page.version(), userAgent: await page.evaluate("navigator.userAgent"), date: new Date().toISOString(), results: list };
    writeFileSync(join(out, `bench-list-${engine}.json`), JSON.stringify(record, null, 1) + "\n");
    console.log(`${engine}: ${record.version}`);
  } else {
    await frameBench();
  }
  const errors = [...new Set(page.consoleErrors)];
  console.log(`errors in the console: ${errors.length}`);
  for (const error of errors.slice(0, 5)) console.log(`  ${error}`);
} finally {
  await page.close();
  await server.close();
}
