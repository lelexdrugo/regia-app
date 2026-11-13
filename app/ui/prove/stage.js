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
    await page.send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false });
    await page.waitFor(`${CANVAS}.width === 2 * ${CANVAS}.clientWidth`, 5_000).catch(() => {});
    const doubled = await page.evaluate(`[${CANVAS}.width, ${CANVAS}.clientWidth, devicePixelRatio]`);
    check("at density 2 the backing store doubles, without a reload", doubled[0] === 2 * doubled[1], JSON.stringify(doubled));
    await page.resize(1440, 900);
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
