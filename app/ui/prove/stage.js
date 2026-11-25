// The moviola's stage in Chrome, on the harness (#486): the Canvas as it draws and as a hand and a keyboard move
// it. On alpha's lanes: the backing store follows the screen's density; the background and a node are the
// direction's tokens, and a new direction on <html> redraws them without a reload, the page's own marker and
// origin unchanged (Acceptance 3); the timecode in the DOM is the catalog's date-day-time of its instant
// (#474/V3), not the prototype's format; the arrows, Home and End go from moment to moment and move the playhead;
// a click selects the node under it; the wheel zooms, a drag pans, 0 brings the camera home; the playhead follows
// a drag on the stage, and a click on the tape. Then the portfolio: every project of the fixture with its health and
// its last milestone, and a link only for a project with a story (Acceptance 2). The negative controls: a stage that
// does not hear the direction (`theme=fixed`) must be caught keeping the old colours, and the prototype's timecode
// must not pass for the catalog's.
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { open } from "./views.js";

const ROUTE = "#/moviola/alpha?data=demo&lang=en";
const CANVAS = `document.querySelector(".stage canvas")`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** The colour of the canvas at (x, y) CSS pixels, as [r, g, b]. */
const pixel = (page, x, y) =>
  page.evaluate(`(() => {
    const canvas = ${CANVAS};
    const k = canvas.width / canvas.clientWidth;
    const d = canvas.getContext("2d").getImageData(Math.floor(${x} * k), Math.floor(${y} * k), 1, 1).data;
    return [d[0], d[1], d[2]];
  })()`);

const token = (page, name) => page.evaluate(`getComputedStyle(document.documentElement).getPropertyValue(${JSON.stringify(name)}).trim()`);

/** A token's colour: the minified stylesheet spells #ffffff as #fff. */
function rgb(hex) {
  const digits = hex.replace("#", "");
  const full = digits.length === 3 ? [...digits].map((d) => d + d).join("") : digits;
  const n = Number.parseInt(full, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

const near = (a, b) => a.every((v, i) => Math.abs(v - b[i]) <= 2);

const data = (page, name) => page.evaluate(`${CANVAS}.dataset.${name}`);

/** A key as a keyboard sends it, for the keys prove/cdp.js does not name. */
async function key(page, name, code, keyCode) {
  await page.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: name, code, windowsVirtualKeyCode: keyCode });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: name, code, windowsVirtualKeyCode: keyCode });
}

async function mouse(page, type, x, y, extra = {}) {
  await page.send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mouseReleased" ? 0 : 1, clickCount: 1, ...extra });
}

/** Where the canvas is on the page, and its stage above the tape. */
async function box(page) {
  const [left, top, width, height] = await page.evaluate(`(() => { const r = ${CANVAS}.getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
  const [w, h] = (await data(page, "view")).split(" ").map(Number);
  return { left, top, width, height, w, h };
}

async function frames(page) {
  return Number(await data(page, "frames"));
}

async function waitFrames(page, after) {
  await page.waitFor(`Number(${CANVAS}.dataset.frames) > ${after}`, 5_000);
}

const OUTPUT = `document.querySelector("output.strip-range")`;
const PANEL = `document.querySelector(".at-date")`;
const distance = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
const light = ([r, g, b]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;

/**
 * The overview (#598) on alpha, in a fresh page. The tape, measured on the canvas at density 2, where a tick of one
 * pixel covers a whole device pixel: from alpha's first milestone, in the node's ink, to its last, an exception in
 * the negative ink, the story takes at least half the tape (the prototype's scale gave it 130 px of 1,420). A kind
 * hidden: the first moment, a milestone, loses its ink and the pointer's reach, «Clear the filters» brings it back.
 * Its ink is read just under its centre, where no edge and no label passes. The project
 * at the playhead, at three places of it, is what the prototype's panel «alla data» says, with the open decisions of the last moment's
 * notes. A window drawn on the strip becomes the stage's view; a double click shows the whole story. A direction
 * chosen lists its moments and dims the other lanes.
 */
async function overview(page, harness, m, fixture, model, check) {
  const snapshots = fixture.cases.find((c) => c.name === "alpha").snapshots;
  // A real load each time: an address that differs only after # would keep the page, and its playhead.
  let loads = 0;
  const fresh = async () => {
    loads += 1;
    await page.goto(`${harness}?load=${loads}${ROUTE}`);
    await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(ROUTE)}`);
    // The screen settles a line or so after its first frame: the backing store at its box, and a frame drawn in it.
    const settled = `(() => { const c = ${CANVAS}; const k = devicePixelRatio || 1;
      return c.width === Math.round(c.clientWidth * k) && c.height === Math.round(c.clientHeight * k); })()`;
    await page.waitFor(settled, 5_000).catch(() => {});
    await sleep(150);
    await page.waitFor(settled, 5_000).catch(() => {});
  };
  // The tape first, on the page as the checks above left it, the playhead at the tape's left end: at density 2, which
  // the next frame finds (Chromium 156 announces an emulated density to no listener).
  let at = await box(page);
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false });
  await mouse(page, "mouseWheel", at.left + at.w / 2, at.top + at.h / 2, { deltaX: 0, deltaY: 0, buttons: 0 });
  await page.waitFor(`${CANVAS}.width === 2 * ${CANVAS}.clientWidth`, 5_000).catch(() => {});
  at = await box(page);
  const [k, row] = await page.evaluate(`(() => {
    const canvas = ${CANVAS};
    const k = canvas.width / canvas.clientWidth;
    return [k, Array.from(canvas.getContext("2d").getImageData(0, Math.floor((${at.h} + 30) * k), canvas.width, 1).data)];
  })()`);
  const xs = (colour) => {
    const out = [];
    for (let i = 0; i < row.length; i += 4) if (near([row[i], row[i + 1], row[i + 2]], colour)) out.push(i / 4 / k);
    return out;
  };
  const first = Math.min(...xs(rgb(await token(page, "--node"))));
  const last = Math.max(...xs(rgb(await token(page, "--negative"))));
  const tape = m.tapeScale(model.nodes.map((n) => n.t), 10, at.width - 10);
  const measured = (last - first) / (at.width - 20);
  check("alpha's story takes at least half the tape, measured on the canvas", measured >= 0.5
    && Math.abs(first - tape.x(model.t0)) < 2 && Math.abs(last - tape.x(model.t1)) < 2, `${(measured * 100).toFixed(1)} %, from ${first} to ${last} of ${at.width}`);
  await page.resize(1440, 900);
  await fresh();
  at = await box(page);
  const lay = m.layout(model, at.w, at.h, null);

  // The filters on the canvas: the first moment, with the playhead at the end of the story.
  const ids = model.nodes.map((n) => n.milestone.id);
  const [ink, surface] = [rgb(await token(page, "--node")), rgb(await token(page, "--surface"))];
  const selected = () => page.evaluate(`document.getElementById("moviola-status").dataset.selected`);
  const under = () => pixel(page, lay.x[0], lay.y[0] + 4);
  const shown = await under();
  let before = await frames(page);
  await page.evaluate(`document.querySelector('[data-kind="milestone"] input').click(), true`);
  await waitFrames(page, before).catch(() => {});
  const hidden = await under();
  await mouse(page, "mousePressed", at.left + lay.x[0], at.top + lay.y[0]);
  await mouse(page, "mouseReleased", at.left + lay.x[0], at.top + lay.y[0]);
  await sleep(60);
  const reached = await selected();
  const count = await page.evaluate(`document.getElementById("moviola-found")?.dataset.count ?? null`);
  check("a kind hidden: the milestone loses its ink and the pointer's reach, 6 of 7 counted",
    near(shown, ink) && distance(hidden, ink) > 60 && reached !== ids[0] && count === "6", `${shown} → ${hidden}; a click selects «${reached}»`);
  before = await frames(page);
  await page.evaluate(`document.getElementById("moviola-clear").click(), true`);
  await waitFrames(page, before).catch(() => {});
  check("«Clear the filters» brings it back", near(await under(), ink), `${await under()}`);

  // The project at the playhead: at the first moment, at the fourth, and between the fourth and the fifth.
  const said = async () => {
    await page.waitFor(`${PANEL}.dataset.decisions !== "loading"`, 5_000).catch(() => {});
    return page.evaluate(`(({ before, after, last, open, decisions }) => ({ before: +before, after: +after, last, open, decisions }))(${PANEL}.dataset)`);
  };
  const wanted = (t) => {
    const s = m.stateAt(model, t);
    const last = s.last ? s.last.milestone.id : "";
    const decisions = s.last ? String(snapshots[last]?.open_decisions?.length ?? 0) : "none";
    return { before: s.before, after: s.after, last, open: s.open.map((l) => l.id).join(","), decisions };
  };
  const places = [];
  await page.evaluate(`${CANVAS}.focus()`);
  await key(page, "Home", "Home", 36);
  places.push([model.nodes[0].t, await said()]);
  for (let i = 0; i < 3; i++) await key(page, "ArrowRight", "ArrowRight", 39);
  places.push([model.nodes[3].t, await said()]);
  await key(page, "Escape", "Escape", 27);
  const between = (model.nodes[3].t + model.nodes[4].t) / 2;
  await page.evaluate(`(() => { const r = document.querySelector(".scrub"); r.value = String(Math.round(${(between - model.t0) / (model.t1 - model.t0)} * 1000)); r.dispatchEvent(new Event("input", { bubbles: true })); })()`);
  const tNow = await page.evaluate(`Number(document.querySelector("output.timecode").dataset.t)`);
  places.push([tNow, await said()]);
  const wrong = places.filter(([t, got]) => JSON.stringify(got) !== JSON.stringify(wanted(t)));
  check("the project at the playhead is the prototype's, at three places, with its notes' open decisions", wrong.length === 0,
    places.map(([, got]) => `${got.before}/${got.after} ${got.last.slice(5, 7)} [${got.open}] ${got.decisions}`).join("; "));

  // The strip: a window from the second moment to the fifth, drawn by hand, becomes the view; a double click, the
  // whole story.
  await page.evaluate(`${CANVAS}.focus()`);
  await page.press("0");
  await page.waitFor(`${OUTPUT}.dataset.whole === "true"`, 5_000).catch(() => {});
  const [sl, st, sw] = await page.evaluate(`(() => { const r = document.querySelector("canvas.strip").getBoundingClientRect(); return [r.left, r.top, r.width]; })()`);
  const strip = m.tapeScale(model.nodes.map((n) => n.t), 8, sw - 8, 12 * m.HOUR, 18);
  const [xa, xb] = [strip.x(model.nodes[1].t) - 4, strip.x(model.nodes[4].t) + 4];
  await mouse(page, "mousePressed", sl + xa, st + 20);
  await mouse(page, "mouseMoved", sl + (xa + xb) / 2, st + 20);
  await mouse(page, "mouseMoved", sl + xb, st + 20);
  await mouse(page, "mouseReleased", sl + xb, st + 20);
  await page.waitFor(`${OUTPUT}.dataset.whole === "false"`, 5_000).catch(() => {});
  // The pointer lands on whole pixels: the window is the one drawn within a pixel of the strip.
  const [from, to] = await page.evaluate(`[Number(${OUTPUT}.dataset.from), Number(${OUTPUT}.dataset.to)]`);
  const zoomed = Number((await data(page, "camera")).split(" ")[2]);
  check("a window drawn on the strip: the stage shows it", Math.abs(strip.x(from) - xa) <= 1 && Math.abs(strip.x(to) - xb) <= 1 && zoomed > 1,
    `${new Date(from).toISOString()} → ${new Date(to).toISOString()} at zoom ${zoomed}; on the strip ${strip.x(from).toFixed(1)}, ${strip.x(to).toFixed(1)} for ${xa.toFixed(1)}, ${xb.toFixed(1)}`);
  await mouse(page, "mousePressed", sl + sw / 2, st + 20);
  await mouse(page, "mouseReleased", sl + sw / 2, st + 20);
  await mouse(page, "mousePressed", sl + sw / 2, st + 20, { clickCount: 2 });
  await mouse(page, "mouseReleased", sl + sw / 2, st + 20, { clickCount: 2 });
  await page.waitFor(`${OUTPUT}.dataset.whole === "true"`, 5_000).catch(() => {});
  check("a double click on the strip shows the whole story", (await page.evaluate(`${OUTPUT}.dataset.whole`)) === "true" && Number((await data(page, "camera")).split(" ")[2]) === 1);

  // The directions: probe-b chosen lists its one moment and dims the main lane's first node.
  const main = await under();
  before = await frames(page);
  await page.evaluate(`document.querySelector('[data-lane="probe-b"] .lane-toggle').click(), true`);
  await waitFrames(page, before).catch(() => {});
  const dimmed = await under();
  const listed = await page.evaluate(`[document.querySelector('[data-lane="probe-b"] .lane-toggle').getAttribute("aria-expanded"),
    document.querySelectorAll('[data-lane="probe-b"] .lane-moments li').length]`);
  check("a direction chosen lists its moments and dims the other lanes", listed[0] === "true" && listed[1] === 1 && light(dimmed) < 0.5 * light(main) && distance(dimmed, surface) < distance(main, surface),
    `${JSON.stringify(listed)}; the main lane's first node ${main} → ${dimmed}`);
}

export const stage = {
  browser: true,
  async run({ ui, page, harness }) {
    const errors = page.consoleErrors.length;
    const m = await import("../src/lib/moviola/model.ts");
    const fixture = JSON.parse(readFileSync(join(ui, "harness/moviola.json"), "utf8"));
    const alpha = fixture.cases.find((c) => c.name === "alpha").timeline;
    const model = m.prepare(alpha, 0);
    const results = [];
    const check = (name, ok, detail = "") => results.push([name, ok, detail]);

    await page.resize(1440, 900);
    await open(page, harness, ROUTE);
    let at = await box(page);
    // At home the camera's zoom is 1 and the stage starts at the view's corner: world and screen agree.
    const lay = m.layout(model, at.w, at.h, null);
    const first = [lay.x[0], lay.y[0]];

    // Density.
    const backing = await page.evaluate(`[${CANVAS}.width, ${CANVAS}.clientWidth]`);
    check("the backing store is the box at density 1", backing[0] === backing[1], JSON.stringify(backing));
    // Chromium 156 announces an emulated density to no listener: the next frame, which a still wheel asks for, finds it.
    const nudge = () => mouse(page, "mouseWheel", at.left + at.w / 2, at.top + at.h / 2, { deltaX: 0, deltaY: 0, buttons: 0 });
    await page.send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false });
    await nudge();
    await page.waitFor(`${CANVAS}.width === 2 * ${CANVAS}.clientWidth`, 5_000).catch(() => {});
    const doubled = await page.evaluate(`[${CANVAS}.width, ${CANVAS}.clientWidth, devicePixelRatio]`);
    check("at density 2 the backing store doubles by the next frame, without a reload", doubled[0] === 2 * doubled[1], JSON.stringify(doubled));
    await page.resize(1440, 900);
    await nudge();
    await page.waitFor(`${CANVAS}.width === ${CANVAS}.clientWidth`, 5_000).catch(() => {});

    // The directions: each one redraws the background and the marks in its tokens, with no reload.
    const marker = await page.evaluate(`(window.stageMarker = Math.random(), [window.stageMarker, performance.timeOrigin])`);
    for (const direction of ["foglio", "nero", "montaggio"]) {
      const before = await frames(page);
      await page.evaluate(`document.documentElement.dataset.dir = ${JSON.stringify(direction)}`);
      await page.waitFor(`${CANVAS}.dataset.dir === ${JSON.stringify(direction)}`, 5_000).catch(() => {});
      const [ground, node] = [await pixel(page, 2, 30), await pixel(page, first[0], first[1])];
      const [surface, ink] = [rgb(await token(page, "--surface")), rgb(await token(page, "--node"))];
      const same = await page.evaluate(`[window.stageMarker, performance.timeOrigin]`);
      check(
        `${direction}: redrawn in its tokens, no reload`,
        near(ground, surface) && near(node, ink) && (await frames(page)) > before && same[0] === marker[0] && same[1] === marker[1],
        `background ${ground} for ${surface}, node ${node} for ${ink}`,
      );
    }

    // The timecode: the catalog's date-day-time of the instant it carries, the last milestone's at first.
    const [t, shown] = await page.evaluate(`[Number(document.querySelector("output.timecode").dataset.t), document.querySelector("output.timecode").textContent]`);
    // The page's own Intl, which the catalog's DATETIME formats with: Node's may spell a space differently.
    const catalog = await page.evaluate(
      `new Intl.DateTimeFormat("en", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }).format(new Date(${t}))`,
    );
    check("the timecode is date-day-time of its instant, the last milestone's", shown === catalog && t === model.t1, `${shown} at ${new Date(t).toISOString()}`);
    const parts = Object.fromEntries(
      new Intl.DateTimeFormat("it-IT", { timeZone: "Europe/Rome", hourCycle: "h23", day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" })
        .formatToParts(new Date(t))
        .map((p) => [p.type, p.value]),
    );
    const prototype = `${parts.day}·${parts.month} ${parts.hour}:${parts.minute}`;
    const caughtFormat = prototype !== shown;

    // The keys: Left from the playhead at the end, Left again, End, Home.
    const selected = () => page.evaluate(`document.getElementById("moviola-status").dataset.selected`);
    const ids = model.nodes.map((n) => n.milestone.id);
    await page.evaluate(`${CANVAS}.focus()`);
    const visits = [];
    for (const [name, code, keyCode] of [["ArrowLeft", "ArrowLeft", 37], ["ArrowLeft", "ArrowLeft", 37], ["End", "End", 35], ["Home", "Home", 36]]) {
      await key(page, name, code, keyCode);
      await sleep(60);
      visits.push(ids.indexOf(await selected()));
    }
    const [tHome, title] = await page.evaluate(`[Number(document.querySelector("output.timecode").dataset.t), document.querySelector("#moviola-status [data-raw]")?.textContent]`);
    check("Left, Left, End and Home select those moments", JSON.stringify(visits) === JSON.stringify([5, 4, 6, 0]), JSON.stringify(visits));
    check("a selection moves the playhead and names the moment", tHome === model.nodes[0].t && title === model.nodes[0].milestone.title, `${title}`);
    await key(page, "Escape", "Escape", 27);
    await sleep(60);
    check("Esc lets the selection go", (await selected()) === "");

    // A click on a node selects it.
    at = await box(page);
    await mouse(page, "mousePressed", at.left + lay.x[2], at.top + lay.y[2]);
    await mouse(page, "mouseReleased", at.left + lay.x[2], at.top + lay.y[2]);
    await sleep(60);
    check("a click on a node selects it", (await selected()) === ids[2], `${await selected()}`);

    // The wheel zooms around the pointer, a drag pans, 0 comes home.
    const camera = async () => (await data(page, "camera")).split(" ").map(Number);
    let before = await frames(page);
    await mouse(page, "mouseWheel", at.left + at.w / 2, at.top + at.h / 2, { deltaX: 0, deltaY: -400, buttons: 0 });
    await waitFrames(page, before).catch(() => {});
    const zoomed = await camera();
    before = await frames(page);
    const y = at.top + at.h - 20;
    await mouse(page, "mousePressed", at.left + 700, y);
    await mouse(page, "mouseMoved", at.left + 600, y);
    await mouse(page, "mouseMoved", at.left + 500, y);
    await mouse(page, "mouseReleased", at.left + 500, y);
    await waitFrames(page, before).catch(() => {});
    const panned = await camera();
    await page.evaluate(`${CANVAS}.focus()`);
    before = await frames(page);
    await page.press("0");
    await waitFrames(page, before).catch(() => {});
    const home = await camera();
    check("the wheel zooms in", zoomed[2] > 1, JSON.stringify(zoomed));
    check("a drag moves the camera", Math.abs(panned[0] - zoomed[0] - 200 / zoomed[2]) < 1, JSON.stringify(panned));
    check("0 brings it home", home[2] === 1, JSON.stringify(home));

    // The playhead, dragged on the stage from where it stands, at the last milestone once at home.
    await page.evaluate(`${CANVAS}.focus()`);
    await key(page, "End", "End", 35);
    await page.press("0");
    await sleep(60);
    const headX = at.left + lay.xAt(model.t1);
    await mouse(page, "mousePressed", headX, at.top + at.h / 2);
    await mouse(page, "mouseMoved", headX - 120, at.top + at.h / 2);
    await mouse(page, "mouseMoved", headX - 240, at.top + at.h / 2);
    await mouse(page, "mouseReleased", headX - 240, at.top + at.h / 2);
    await sleep(60);
    const tDragged = await page.evaluate(`Number(document.querySelector("output.timecode").dataset.t)`);
    check("the playhead follows a drag on the stage", Math.abs(tDragged - lay.tAt(lay.xAt(model.t1) - 240)) < 1000, new Date(tDragged).toISOString());

    // The tape: a click at its left end takes the playhead to the start of the story.
    await mouse(page, "mousePressed", at.left + 12, at.top + at.h + 30);
    await mouse(page, "mouseReleased", at.left + 12, at.top + at.h + 30);
    await sleep(60);
    const tTape = await page.evaluate(`Number(document.querySelector("output.timecode").dataset.t)`);
    check("a click on the tape moves the playhead", tTape < model.t0, new Date(tTape).toISOString());

    await overview(page, harness, m, fixture, model, check);

    // The portfolio (Acceptance 2): each project of the fixture with its health and its last milestone.
    await open(page, harness, "#/moviola?data=demo&lang=en");
    const rows = await page.evaluate(`[...document.querySelectorAll("li[data-project]")].map((li) => ({
      id: li.dataset.project,
      health: li.querySelector("[data-health]")?.dataset.health,
      last: li.querySelector("[data-last]")?.dataset.last ?? null,
      title: li.querySelector("[data-last] .reel-title")?.textContent ?? null,
      link: !!li.querySelector("a.reel-name"),
    }))`);
    const portfolio = fixture.portfolio.map((p) => ({ id: p.id, health: p.health ?? "none", last: p.last?.id ?? null, title: p.last?.title ?? null, link: p.milestones > 0 }));
    const byId = (list) => JSON.stringify([...list].sort((a, b) => a.id.localeCompare(b.id)));
    check("the portfolio: every project, its health and its last milestone; only a story opens", byId(rows) === byId(portfolio), JSON.stringify(rows.map((r) => `${r.id} ${r.health} ${r.last ?? "-"}`)));

    // The control: a stage that does not hear the direction keeps the old colours.
    await page.evaluate(`document.documentElement.dataset.dir = "montaggio"`);
    await open(page, harness, `${ROUTE}&theme=fixed`);
    const old = await pixel(page, 2, 30);
    await page.evaluate(`document.documentElement.dataset.dir = "foglio"`);
    await sleep(400);
    const kept = await pixel(page, 2, 30);
    const caughtTheme = !near(kept, rgb(await token(page, "--surface"))) && near(kept, old);
    await page.evaluate(`document.documentElement.dataset.dir = "montaggio"`);

    for (const [name, ok, detail] of results) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
    console.log(`negative control, the prototype's timecode ${JSON.stringify(prototype)}: ${caughtFormat ? "caught" : "NOT CAUGHT"}`);
    console.log(`negative control, a stage deaf to the direction: ${caughtTheme ? "caught" : "NOT CAUGHT"} (kept ${kept})`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return results.every(([, ok]) => ok) && caughtFormat && caughtTheme && newErrors.length === 0;
  },
};
