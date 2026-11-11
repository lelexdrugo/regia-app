// Test only. In a build with the `smoke` feature the app reports what the webview rendered, then what it
// could and could not do on this engine, and exits (app/src-tauri/src/smoke.rs judges the checks). In a
// release build `smoke_report` does not exist, and nothing here runs.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

interface Violation {
  directive: string;
  blocked: string;
}

/** A host that never resolves: a probe that is not blocked fails on DNS, and nothing leaves the machine. */
const ELSEWHERE = "https://example.invalid";

const uncaught: string[] = [];
const violations: Violation[] = [];

function record(message: string): void {
  if (uncaught.length < 20) {
    uncaught.push(message);
  }
}

addEventListener("error", (event) => record(`${event.message} (${event.filename}:${event.lineno})`));
addEventListener("unhandledrejection", (event) => record(`unhandled rejection: ${String(event.reason)}`));
document.addEventListener("securitypolicyviolation", (event) => {
  violations.push({ directive: event.effectiveDirective || event.violatedDirective, blocked: event.blockedURI });
});

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** What `app-smoke` compares with the CLI. Taken before any probe adds an element to the page. */
function snapshot(language: string) {
  const sections: Record<string, number[]> = { waiting_on_owner: [], ready: [], in_progress: [] };
  for (const item of document.querySelectorAll<HTMLElement>("li[data-section]")) {
    (sections[item.dataset["section"] ?? ""] ??= []).push(Number(item.dataset["number"]));
  }
  return {
    language,
    sections,
    headings: [...document.querySelectorAll("h1, h2")].map((node) => node.textContent ?? ""),
    images: document.querySelectorAll("img").length,
    scripts_inline: document.querySelectorAll("script:not([src])").length,
    text: document.body.innerText,
  };
}

/** The four features the UX prototype's CSS and dialogs rely on (#473, change 3), used, not only parsed. */
function features(): Check[] {
  const reference = document.createElement("div");
  const probe = document.createElement("div");
  document.body.append(reference, probe);
  // CSSOM, which the CSP allows; a style attribute it would refuse.
  probe.style.color = "color-mix(in srgb, rgb(255, 0, 0) 50%, rgb(0, 0, 255))";
  const mixed = getComputedStyle(probe).color;
  probe.style.accentColor = "rgb(1, 2, 3)";
  const accent = getComputedStyle(probe).accentColor;
  const inherited = getComputedStyle(reference).color;
  reference.remove();
  probe.remove();

  const dialog = document.createElement("dialog");
  document.body.append(dialog);
  let modal = "";
  try {
    dialog.showModal();
    modal = dialog.open ? "open" : "not open";
    dialog.close();
  } catch (error) {
    modal = String(error);
  }
  dialog.remove();

  let focusVisible = "";
  try {
    document.querySelector(":focus-visible");
    focusVisible = CSS.supports("selector(:focus-visible)") ? "supported" : "not supported";
  } catch (error) {
    focusVisible = String(error);
  }

  return [
    { name: "color-mix()", ok: mixed !== "" && mixed !== inherited && mixed !== "rgb(255, 0, 0)", detail: mixed },
    { name: "<dialog> showModal()", ok: modal === "open", detail: modal },
    { name: "accent-color", ok: accent === "rgb(1, 2, 3)", detail: accent },
    { name: ":focus-visible", ok: focusVisible === "supported", detail: focusVisible },
  ];
}

interface Expected {
  name: string;
  directive: string;
  blocked: string;
}

/** V06 on this engine: each probe must be refused by the CSP, seen in a `securitypolicyviolation`. */
async function csp(): Promise<{ checks: Check[]; expected: Expected[] }> {
  const checks: Check[] = [];
  const expected: Expected[] = [];

  let evalRan = true;
  try {
    (0, eval)("1");
  } catch {
    evalRan = false;
  }
  checks.push({ name: "csp: eval", ok: !evalRan, detail: evalRan ? "eval ran" : "refused" });
  expected.push({ name: "csp: eval", directive: "script-src", blocked: "eval" });

  const flag = "__smokeInlineScriptRan";
  const script = document.createElement("script");
  script.textContent = `window.${flag} = true;`;
  document.body.append(script);
  script.remove();
  const inlineRan = flag in window;
  checks.push({ name: "csp: inline script", ok: !inlineRan, detail: inlineRan ? "ran" : "refused" });
  expected.push({ name: "csp: inline script", directive: "script-src", blocked: "inline" });

  const styled = document.createElement("div");
  styled.setAttribute("style", "outline: 7px solid rgb(1, 2, 3)");
  document.body.append(styled);
  const styleApplied = getComputedStyle(styled).outlineWidth === "7px";
  styled.remove();
  checks.push({ name: "csp: style attribute", ok: !styleApplied, detail: styleApplied ? "applied" : "refused" });
  expected.push({ name: "csp: style attribute", directive: "style-src", blocked: "inline" });

  const fetched = await fetch(`${ELSEWHERE}/connect`).then(
    () => "answered",
    () => "failed",
  );
  checks.push({ name: "csp: fetch elsewhere", ok: fetched === "failed", detail: fetched });
  expected.push({ name: "csp: fetch elsewhere", directive: "connect-src", blocked: ELSEWHERE });

  const image = new Image();
  image.src = `${ELSEWHERE}/image.png`;
  expected.push({ name: "csp: image elsewhere", directive: "img-src", blocked: ELSEWHERE });

  const frame = document.createElement("iframe");
  frame.src = `${ELSEWHERE}/frame`;
  document.body.append(frame);
  expected.push({ name: "csp: frame elsewhere", directive: "frame-src", blocked: ELSEWHERE });

  const form = document.createElement("form");
  form.action = `${ELSEWHERE}/form`;
  form.method = "get";
  document.body.append(form);
  form.submit();
  expected.push({ name: "csp: form elsewhere", directive: "form-action", blocked: ELSEWHERE });

  // Violation events arrive after the task that caused them.
  await sleep(500);
  frame.remove();
  form.remove();
  return { checks, expected };
}

/**
 * Engines name the same refusal differently: `script-src-elem` or `script-src`, `inline` or nothing, and a
 * blocked frame by its origin only. The directive tells the probes apart.
 */
function matches(violation: Violation, expected: Expected): boolean {
  const inline = expected.blocked === "inline" || expected.blocked === "eval";
  return (
    violation.directive.startsWith(expected.directive) &&
    (violation.blocked.startsWith(expected.blocked) || (inline && violation.blocked === ""))
  );
}

/** V09 on this engine: `on_navigation` and `on_new_window` keep the page where it is. */
async function navigation(): Promise<Check[]> {
  const opened = window.open(`${ELSEWHERE}/window`, "_blank");
  const here = location.href;
  location.href = `${ELSEWHERE}/navigation`;
  // A navigation that was not refused unloads this page, and the app's watchdog fails the run.
  await sleep(500);
  return [
    { name: "navigation: window.open", ok: opened === null, detail: opened === null ? "refused" : "a window opened" },
    { name: "navigation: location", ok: location.href === here, detail: location.href },
  ];
}

/** V10 on this engine: plugins, window APIs and unknown commands are refused, with an error. */
async function acl(): Promise<Check[]> {
  const refused: [string, Record<string, unknown>][] = [
    ["plugin:shell|execute", { program: "probe", args: ["--version"] }],
    ["plugin:fs|read_file", { path: "/etc/hosts" }],
    ["plugin:opener|open_url", { url: `${ELSEWHERE}/opener` }],
    ["plugin:window|set_title", { label: "main", value: "x" }],
    ["plugin:event|emit", { event: "smoke", payload: 1 }],
    ["plugin:webview|internal_toggle_devtools", {}],
    ["read_file", { path: "/etc/hosts" }],
  ];
  const checks: Check[] = [];
  for (const [command, args] of refused) {
    const outcome = await Promise.race([
      invoke(command, args).then(
        () => "allowed",
        (error: unknown) => `refused: ${String(error)}`,
      ),
      sleep(5000).then(() => "no answer"),
    ]);
    // Refused by the ACL itself, whose message says so: a call it let through could fail in the command.
    checks.push({ name: `acl: ${command}`, ok: outcome.includes("not allowed"), detail: outcome });
  }
  return checks;
}

/**
 * The threat model's minimal set for events (§3.3, an open point of V29): the smoke capability grants
 * only `core:event:allow-listen` and `allow-unlisten`, and the core pings this window.
 */
async function events(): Promise<Check[]> {
  let received = 0;
  let detail = "";
  try {
    const stop = await listen("smoke-ping", () => {
      received += 1;
    });
    for (let waited = 0; received === 0 && waited < 3000; waited += 100) {
      await sleep(100);
    }
    await stop();
    detail = received > 0 ? "received, then stopped listening" : "no event in 3 s";
  } catch (error) {
    detail = `refused: ${String(error)}`;
  }
  return [{ name: "events: listen and unlisten with the two event permissions", ok: received > 0, detail }];
}

export async function runSmoke(language: string): Promise<void> {
  const report = snapshot(language);
  const checks = features();
  const { checks: cspChecks, expected } = await csp();
  checks.push(...cspChecks);
  checks.push(...(await navigation()));
  checks.push(...(await acl()));
  checks.push(...(await events()));

  for (const probe of expected) {
    const seen = violations.some((violation) => matches(violation, probe));
    checks.push({ name: `${probe.name}: violation reported`, ok: seen, detail: `${probe.directive} ${probe.blocked}` });
  }
  const unexpected = violations.filter((violation) => !expected.some((probe) => matches(violation, probe)));
  checks.push({
    name: "no other CSP violation",
    ok: unexpected.length === 0,
    detail: unexpected.map((violation) => `${violation.directive} ${violation.blocked}`).join(", "),
  });
  checks.push({ name: "no uncaught error", ok: uncaught.length === 0, detail: uncaught.join(" | ") });

  await invoke("smoke_report", { report: { ...report, checks, user_agent: navigator.userAgent } });
}
