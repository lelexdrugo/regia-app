// A minimal Chrome DevTools Protocol driver with no dependencies (Node 22 or later has WebSocket and fetch),
// ported from the UX prototype's tools/cdp.mjs in the dossier. Test only: it drives the harness for the proofs.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// The named keys page.press knows: their code, key code and text, as Chrome's own keyboard sends them.
const KEYS = {
  Tab: ["Tab", 9, ""],
  Enter: ["Enter", 13, "\r"],
  Escape: ["Escape", 27, ""],
  " ": ["Space", 32, " "],
  ArrowLeft: ["ArrowLeft", 37, ""],
  ArrowUp: ["ArrowUp", 38, ""],
  ArrowRight: ["ArrowRight", 39, ""],
  ArrowDown: ["ArrowDown", 40, ""],
  "?": ["Slash", 191, "?"],
};

/** CHROME_PATH, or the first Chrome, Chromium or Edge where a runner or a desktop keeps one. */
export function findChrome() {
  const candidates = [
    process.env.CHROME_PATH,
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
    join(homedir(), "AppData/Local/ms-playwright/chromium-1223/chrome-win64/chrome.exe"),
    // Last: a desktop Edge that is already open can take the launch over and exit at once.
    "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  ].filter(Boolean);
  const found = candidates.find((path) => existsSync(path));
  if (!found) {
    throw new Error("no Chrome, Chromium or Edge found: set CHROME_PATH");
  }
  return found;
}

/** The port Chrome writes once it listens. If it never does, its own stderr says why. */
async function devtoolsPort(file, chrome, stderr) {
  for (let waited = 0; waited < 30_000 && chrome.exitCode === null; waited += 50) {
    const port = existsSync(file) ? readFileSync(file, "utf8").split("\n")[0].trim() : "";
    if (port) {
      return port;
    }
    await sleep(50);
  }
  chrome.kill();
  throw new Error(`the browser did not start (exit code ${chrome.exitCode}): ${stderr().trim() || "nothing on stderr"}`);
}

/**
 * A headless browser with one page. `profiles` holds its throwaway profile. `CHROME_FLAGS` adds flags, for a
 * machine that needs one. `headed` opens a window with the GPU instead, at its own size, and `binary` picks the
 * browser: prove/bench.mjs measures time per frame there, never headless.
 */
export async function launch({ width, height, profiles, binary = findChrome(), headed = false }) {
  const profile = join(profiles, `chrome-${process.pid}`);
  rmSync(profile, { recursive: true, force: true });
  mkdirSync(profile, { recursive: true });
  const flags = (process.env.CHROME_FLAGS ?? "").split(" ").filter(Boolean);
  const chrome = spawn(
    binary,
    [
      ...(headed ? [] : ["--headless=new", "--disable-gpu"]), "--no-first-run", "--no-default-browser-check", "--hide-scrollbars",
      "--disable-extensions", "--disable-background-networking", "--disable-component-update", "--disable-sync",
      // The proofs move the harness more than 200 times in 10 s, and Chrome then ignores further changes of the
      // address without a word (crbug.com/1038223): a view would never arrive.
      "--disable-ipc-flooding-protection",
      ...flags, "--remote-debugging-port=0", `--user-data-dir=${profile}`, `--window-size=${width},${height}`, "about:blank",
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );
  let stderr = "";
  chrome.stderr.on("data", (chunk) => {
    stderr = (stderr + chunk).slice(-4000);
  });
  const port = await devtoolsPort(join(profile, "DevToolsActivePort"), chrome, () => stderr);
  const close = async () => {
    // On Windows the profile stays locked until the process has really gone.
    const exited = chrome.exitCode === null ? new Promise((resolve) => chrome.once("exit", resolve)) : Promise.resolve();
    chrome.kill();
    await Promise.race([exited, sleep(10_000)]);
    await sleep(300);
    rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  };
  const page = await attach(port, { browser: binary, close });
  if (!headed) {
    await page.resize(width, height);
  }
  return page;
}

/** The first page of a browser whose DevTools listen on `port`; `close` ends that browser. */
export async function attach(port, { browser, close }) {
  let target;
  for (let tries = 0; tries < 100 && !target; tries++) {
    const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
    target = list.find((entry) => entry.type === "page");
    if (!target) {
      await sleep(50);
    }
  }
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = reject;
  });

  let sequence = 0;
  const waiting = new Map();
  const listeners = new Map();
  const consoleErrors = [];
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.id && waiting.has(message.id)) {
      const { resolve, reject } = waiting.get(message.id);
      waiting.delete(message.id);
      if (message.error) {
        reject(new Error(message.error.message));
      } else {
        resolve(message.result);
      }
      return;
    }
    if (message.method === "Runtime.exceptionThrown") {
      const details = message.params.exceptionDetails;
      consoleErrors.push(details.exception?.description ?? details.text);
    } else if (message.method === "Runtime.consoleAPICalled" && ["error", "assert"].includes(message.params.type)) {
      consoleErrors.push(message.params.args.map((arg) => arg.value ?? arg.description).join(" "));
    }
    for (const listener of listeners.get(message.method) ?? []) {
      listener(message.params);
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++sequence;
      waiting.set(id, { resolve, reject });
      socket.send(JSON.stringify({ id, method, params }));
    });

  const page = {
    browser,
    consoleErrors,
    send,
    on(method, listener) {
      listeners.set(method, [...(listeners.get(method) ?? []), listener]);
    },
    async evaluate(expression) {
      const out = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
      if (out.exceptionDetails) {
        throw new Error(out.exceptionDetails.exception?.description ?? out.exceptionDetails.text);
      }
      return out.result.value;
    },
    async resize(w, h) {
      await send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 1, mobile: false });
    },
    async goto(url) {
      await send("Page.navigate", { url });
    },
    /**
     * A key pressed and released as a keyboard would send it, with the focus where it is: one character
     * (`g`, `?`) or a key's name (`Tab`, `Enter`, `Escape`, `ArrowDown`, ` `). `modifiers` is CDP's mask:
     * 1 Alt, 2 Ctrl, 4 Meta, 8 Shift.
     */
    async press(key, modifiers = 0) {
      const [code, keyCode, text] = KEYS[key] ?? [`Key${key.toUpperCase()}`, key.toUpperCase().charCodeAt(0), key];
      const shift = key === "?" ? 8 : 0;
      const common = { key, code, windowsVirtualKeyCode: keyCode, modifiers: modifiers | shift };
      await send("Input.dispatchKeyEvent", { ...common, type: text ? "keyDown" : "rawKeyDown", ...(text ? { text } : {}) });
      await send("Input.dispatchKeyEvent", { ...common, type: "keyUp" });
    },
    /** Polls until `expression` is true. A page that is still loading only delays it. */
    async waitFor(expression, ms = 15_000) {
      for (let waited = 0; waited < ms; waited += 10) {
        if (await page.evaluate(expression).catch(() => false)) {
          return;
        }
        await sleep(10);
      }
      throw new Error(`timed out after ${ms} ms waiting for ${expression}`);
    },
    async version() {
      return (await send("Browser.getVersion")).product;
    },
    async close() {
      try {
        socket.close();
      } catch {
        // Already closed.
      }
      await close();
    },
  };
  await send("Page.enable");
  await send("Runtime.enable");
  return page;
}
