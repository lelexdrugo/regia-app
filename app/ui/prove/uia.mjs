// The moviola in Windows' tree of UI Automation (#490, Acceptance 2): what Narrator and NVDA are given, on the
// product's components in a WebView2, by hand and on Windows only, never in `npm run prove`. It serves the harness
// under the app's CSP, as the proofs do, in a WebView2 host (an app that hosts WebView2 and lets its page navigate:
// the renderer spike's moviola-bench.exe is one), with Chromium's accessibility on, and chooses moments by keys sent
// as a keyboard sends them; prove/uia.ps1 then reads the window's tree, in Windows PowerShell 5.1:
//   1. x100 at the zoom of detail, Home and 250 arrows: the 251st moment, past the renderer spike's 200;
//   2. x100 at the zoom of groups, Home and one arrow: a moment in the first group of several, inside its group;
//   3. alpha with no moment chosen: the items of the directions' list, each name in words.
//
//   node prove/uia.mjs --host <a WebView2 app> --out <folder> [--port 9351]
//
// The folder receives uia-<case>.json, one per case, with what was asked and what the tree said. A host's window that
// something covers still answers: Chromium's occlusion is turned off, as for prove/bench.mjs.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { attach } from "./cdp.js";
import { messages } from "./messages.js";
import { policy, serve } from "./serve.js";

const UI = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET = resolve(UI, "../../target");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
const args = process.argv.slice(2);
const option = (name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const host = option("host");
const out = option("out");
const port = Number(option("port") ?? 9351);
if (!host || !out) {
  console.error("--host: a WebView2 app; --out: the folder for the results");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
const en = messages(UI, "en");
const CANVAS = `document.querySelector(".stage canvas")`;

const browserArgs = [
  `--remote-debugging-port=${port}`,
  "--force-renderer-accessibility",
  "--disable-features=CalculateNativeWinOcclusion",
  "--disable-backgrounding-occluded-windows",
  "--disable-renderer-backgrounding",
];
const child = spawn(host, [], { env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: browserArgs.join(" ") }, stdio: "ignore" });
let page = null;
for (let waited = 0; waited < 30_000 && !page; waited += 200) {
  const list = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json()).catch(() => []);
  if (list.some((entry) => entry.type === "page")) page = await attach(port, { browser: "WebView2", close: async () => void child.kill() });
  else await sleep(200);
}
if (!page) {
  child.kill();
  throw new Error(`no DevTools on port ${port}: the host may set its own browser arguments`);
}
const server = await serve(join(TARGET, "ui-harness"), policy(join(UI, "../src-tauri/tauri.conf.json")));

async function key(name, keyCode) {
  await page.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: name, code: name, windowsVirtualKeyCode: keyCode });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: name, code: name, windowsVirtualKeyCode: keyCode });
}

let loads = 0;
async function open(route) {
  loads += 1;
  await page.goto(`${server.origin}/harness/index.html?uia=${loads}${route}`);
  await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(route)}`, 60_000);
}

/** What the page says it chose, then what the tree of UI Automation says. */
async function read(name, asked, lists = "") {
  const page_ = await page.evaluate(`(() => {
    const canvas = ${CANVAS};
    const id = canvas?.getAttribute("aria-activedescendant");
    const option = id ? document.getElementById(id) : null;
    return { moment: option?.dataset.id ?? null, text: option?.textContent ?? null, place: option ? [option.getAttribute("aria-posinset"), option.getAttribute("aria-setsize")] : null };
  })()`);
  await sleep(500);
  const run = spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(UI, "prove/uia.ps1"), "-ProcessId", String(child.pid), "-Stage", en["moviola-stage"], ...(lists ? ["-Lists", lists] : [])], { encoding: "utf8" });
  const tree = run.status === 0 ? JSON.parse(run.stdout.trim()) : { error: run.stderr.trim() };
  const record = { case: name, asked, page: page_, tree, browser: await page.version(), date: new Date().toISOString() };
  writeFileSync(join(out, `uia-${name}.json`), JSON.stringify(record, null, 1) + "\n");
  return record;
}

try {
  const x100 = "#/moviola/x100?data=demo&lang=en";
  await open(x100);
  await page.evaluate(`${CANVAS}.focus()`);
  await key("Home", 36);
  for (let i = 0; i < 250; i++) await key("ArrowRight", 39);
  const detail = await read("x100-detail", "x100, zoom of detail: Home and 250 arrows, the 251st moment");

  const [left, top, width, height] = await page.evaluate(`(() => { const r = ${CANVAS}.getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  await page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: left + width / 2, y: top + height / 3, deltaX: 0, deltaY: 6000, buttons: 0 });
  await page.waitFor(`${CANVAS}.dataset.groups !== ""`, 10_000);
  await key("Home", 36);
  await key("ArrowRight", 39);
  const groups = await read("x100-groups", "x100, zoom of groups (the whole stage): Home and one arrow, a moment in the first group of several");

  await open("#/moviola/alpha?data=demo&lang=en");
  const directions = await read("alpha-directions", "alpha, no moment chosen: the items of every list but the stage's", "*");

  for (const record of [detail, groups, directions]) {
    const chosen = record.tree.stage?.chosen;
    console.log(`${record.case}: the page chose ${record.page.moment} (place ${record.page.place?.join(" of ") ?? "-"});`
      + ` the tree: ${chosen ? `${chosen.type} «${chosen.name}» focus ${chosen.focus} selected ${chosen.selected}` : "no item chosen"}`
      + `${record.tree.stage?.within ? `, in ${record.tree.stage.within.type} «${record.tree.stage.within.name}»` : ""}${record.tree.error ? ` (${record.tree.error})` : ""}`);
  }
  for (const list of directions.tree.lists ?? []) console.log(`  list «${list.name}»: ${list.items.slice(0, 6).map((item) => `«${item.slice(0, 90)}»`).join("; ")}`);
} finally {
  await page.close();
  await server.close();
}
