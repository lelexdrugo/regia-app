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

/**
 * The world from tiles against the world drawn directly (#489), on the stage as it is: still wheels at the same zoom
 * until a frame's world comes from tiles (`data-world`), then a new direction and back, whose frame draws it
 * directly. The share of the stage's pixels that differ by more than 8 in a channel, and, for the control, the share
 * that differs from the tiles' own frame moved by one device pixel.
 */
async function sameWorld(page, at) {
  const grab = (name) => page.evaluate(`(() => { const c = ${CANVAS}; const k = c.width / c.clientWidth;
    window[${JSON.stringify(name)}] = c.getContext("2d").getImageData(0, 0, c.width, Math.floor(${at.h} * k)).data; return c.dataset.world; })()`);
  let world = "";
  for (let i = 0; i < 12 && world !== "tiles"; i++) {
    const before = await frames(page);
    await mouse(page, "mouseWheel", at.left + 4, at.top + at.h - 4, { deltaX: 0, deltaY: 0, buttons: 0 });
    await waitFrames(page, before).catch(() => {});
    world = await data(page, "world");
  }
  const tiles = await grab("__tiles");
  const dir = (await page.evaluate("document.documentElement.dataset.dir")) || "montaggio";
  for (const next of [dir === "foglio" ? "nero" : "foglio", dir]) {
    const before = await frames(page);
    await page.evaluate(`document.documentElement.dataset.dir = ${JSON.stringify(next)}`);
    await waitFrames(page, before).catch(() => {});
  }
  const direct = await grab("__direct");
  const [differ, moved] = await page.evaluate(`(() => {
    const a = window.__tiles, b = window.__direct, w = ${CANVAS}.width;
    const far = (p, q) => Math.max(Math.abs(a[p] - b[q]), Math.abs(a[p + 1] - b[q + 1]), Math.abs(a[p + 2] - b[q + 2])) > 8;
    let n = 0, m = 0;
    for (let i = 0; i < a.length; i += 4) {
      if (far(i, i)) n++;
      if ((i / 4) % w > 0 && Math.max(Math.abs(a[i] - a[i - 4]), Math.abs(a[i + 1] - a[i - 3]), Math.abs(a[i + 2] - a[i - 2])) > 8) m++;
    }
    return [n / (a.length / 4), m / (a.length / 4)];
  })()`);
  return { tiles, direct, differ, moved };
}

/** The tape's ink of each kind, which a group of several takes, and the ink a group of one's glyph shows (canvas2d.ts's
 * TICK and STYLE): the decision's fill is the surface, so its stroke is what shows. */
const TICK_INK = { milestone: "node", decision: "node", gate: "accent", exception: "negative", exploration: "muted", rejoin: "positive", abandonment: "faint" };
const GLYPH_INK = { milestone: "node", decision: "node", gate: "accent", exception: "negative", exploration: "node", rejoin: "node", abandonment: "node" };

/**
 * The level of detail (#489, Acceptance 2), on x10's synthetic story (`#/moviola/dense`), in a fresh page: the wheel
 * takes the stage to the whole of itself, where the canvas draws a mark in its kind's ink where each group in view
 * is, measured on the canvas's pixels around the place lod.ts gives it, and the line under the stage says how many
 * groups and moments; a click on a group of several opens it at the zoom of detail, and the line goes. The negative
 * control: every kind hidden, the groups almost gone as a filtered moment is, and the same measure must fail.
 */
async function groups(page, harness, m, check) {
  const s = await import("../src/lib/moviola/scene.ts");
  const l = await import("../src/lib/moviola/lod.ts");
  const c = await import("../src/lib/moviola/camera.ts");
  const { syntheticTimeline } = await import("../harness/synthetic.ts");
  const route = "#/moviola/dense?data=demo&lang=en";
  await page.resize(1440, 900);
  await page.goto(`${harness}?load=dense${route}`);
  await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(route)}`);
  const at = await box(page);
  const model = m.prepare(syntheticTimeline(10), 0);
  const lay = m.layout(model, at.w, at.h, null);
  const scene = s.buildScene(model, lay);
  const view = { w: at.w, h: at.h };
  const extent = { w: lay.width, h: lay.height };
  const fit = c.fitZoom(extent, view);
  await mouse(page, "mouseWheel", at.left + at.w / 2, at.top + at.h / 2, { deltaX: 0, deltaY: 4000, buttons: 0 });
  await page.waitFor(`${CANVAS}.dataset.groups !== "" && document.getElementById("moviola-groups")?.dataset.groups !== ""`, 5_000).catch(() => {});
  await sleep(100);
  // The line was there before, empty: the stage kept its size.
  check("the line of the groups keeps the stage's size", (await data(page, "view")) === `${at.w} ${at.h}`, `${await data(page, "view")} for ${at.w} ${at.h}`);
  const said = await data(page, "groups");
  const [cx, cy, z] = (await data(page, "camera")).split(" ").map(Number);
  const cam = { cx, cy, z: fit };
  const g = l.groupsAt(scene, model, l.groupZoom(fit));
  const shown = l.inView(g, cam, view);
  const line = await page.evaluate(`(() => { const p = document.getElementById("moviola-groups"); return p ? [p.dataset.groups, p.textContent.trim()] : null; })()`);
  check("zoomed out to the whole stage, the stage draws groups and the line under it says what they hold",
    Math.abs(z - fit) < 1e-3 && said === `${shown.groups} ${shown.moments}` && line?.[0] === said && line[1].startsWith(`${shown.moments} moments in view, drawn in ${shown.groups} groups`),
    `${said} at zoom ${z}; «${line?.[1] ?? "no line"}»`);

  // Where each group in view is drawn, and in which ink, away from the lanes' names and the days' band.
  const ink = {};
  for (const name of new Set([...Object.values(TICK_INK), ...Object.values(GLYPH_INK)])) ink[name] = rgb(await token(page, `--${name}`));
  const boxes = [];
  for (let q = 0; q < g.n; q++) {
    const sx = s.toScreenX(cam, view, (g.x0[q] + g.x1[q]) / 2);
    const sy = s.toScreenY(cam, view, g.y[q]);
    if (sx < 120 || sx > view.w - 6 || sy < 30 || sy > view.h - 6) continue;
    const kind = s.KINDS[g.kind[q]];
    const one = l.size(g, q) === 1;
    boxes.push([sx, sy, one ? 3 : 1, ink[one ? GLYPH_INK[kind] : TICK_INK[kind]]]);
  }
  const measure = () => page.evaluate(`(() => {
    const canvas = ${CANVAS};
    const k = canvas.width / canvas.clientWidth;
    const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    let hit = 0;
    for (const [x, y, r, [R, G, B]] of ${JSON.stringify(boxes)}) {
      let found = false;
      for (let dy = Math.floor((y - r) * k); dy <= Math.ceil((y + r) * k) && !found; dy++) {
        for (let dx = Math.floor((x - r) * k); dx <= Math.ceil((x + r) * k); dx++) {
          const i = (dy * canvas.width + dx) * 4;
          if (Math.hypot(data[i] - R, data[i + 1] - G, data[i + 2] - B) <= 60) { found = true; break; }
        }
      }
      if (found) hit += 1;
    }
    return hit;
  })()`);
  const hit = await measure();
  check("each group in view has its mark in its kind's ink where lod.ts puts it, measured on the canvas", boxes.length >= 50 && hit >= 0.9 * boxes.length,
    `${hit} of ${boxes.length} groups`);
  const far = await sameWorld(page, at);
  check("zoomed out, the world from tiles is the world drawn directly", far.tiles === "tiles" && far.direct === "direct" && far.differ <= 0.001,
    `${(far.differ * 100).toFixed(3)} % of the stage differs (${far.tiles}, then ${far.direct})`);

  // The control: every kind hidden.
  let before = await frames(page);
  await page.evaluate(`[...document.querySelectorAll("[data-kind] input")].forEach((input) => input.checked && input.click()), true`);
  await waitFrames(page, before).catch(() => {});
  await sleep(100);
  const hidden = await measure();
  const caught = hidden < 0.5 * boxes.length;
  before = await frames(page);
  await page.evaluate(`document.getElementById("moviola-clear")?.click(), true`);
  await waitFrames(page, before).catch(() => {});

  // A click on a group of several, away from the edges: the zoom of detail, on it.
  const pick = boxes.findIndex(([x, y, r]) => r === 1 && x > view.w / 3 && x < (2 * view.w) / 3 && y > view.h / 3 && y < (2 * view.h) / 3);
  const [px, py] = pick >= 0 ? boxes[pick] : [view.w / 2, view.h / 2];
  await mouse(page, "mouseMoved", at.left + px, at.top + py, { buttons: 0 });
  await mouse(page, "mousePressed", at.left + px, at.top + py);
  await mouse(page, "mouseReleased", at.left + px, at.top + py);
  await page.waitFor(`${CANVAS}.dataset.groups === "" && document.getElementById("moviola-groups")?.textContent.trim() === ""`, 5_000).catch(() => {});
  const opened = (await data(page, "camera")).split(" ").map(Number);
  check("a click on a group of several opens it at the zoom of detail, and the line empties",
    pick >= 0 && opened[2] >= l.GROUP_ZOOM && (await data(page, "groups")) === "" && (await page.evaluate(`document.getElementById("moviola-groups").textContent.trim()`)) === "",
    `camera ${opened.join(" ")}`);
  const near = await sameWorld(page, at);
  check("at the zoom of detail, the world from tiles is the world drawn directly", near.tiles === "tiles" && near.direct === "direct" && near.differ <= 0.001,
    `${(near.differ * 100).toFixed(3)} % of the stage differs (${near.tiles}, then ${near.direct})`);
  // The control: the tiles' frame against itself moved by one device pixel.
  const moved = Math.min(far.moved, near.moved);
  return { caught, hidden, of: boxes.length, caughtMoved: moved > 0.001, moved };
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
    const grouped = await groups(page, harness, m, check);

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
    console.log(`negative control, the groups with every kind hidden: ${grouped.caught ? "caught" : "NOT CAUGHT"} (${grouped.hidden} of ${grouped.of} still in their ink)`);
    console.log(`negative control, the world one device pixel off: ${grouped.caughtMoved ? "caught" : "NOT CAUGHT"} (${(grouped.moved * 100).toFixed(2)} % of the stage differs)`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return results.every(([, ok]) => ok) && caughtFormat && caughtTheme && grouped.caught && grouped.caughtMoved && newErrors.length === 0;
  },
};
