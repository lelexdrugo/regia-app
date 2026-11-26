// The list of moments in Chrome (#490): what a screen reader reaches on the moviola's stage, by the keys a person
// presses, on x100's synthetic story (`#/moviola/x100`, 2,700 moments, the renderer spike's x100). The canvas is a
// listbox; the moment chosen is its active descendant, an option of a window of the list in the page.
//
// At the zoom of detail: Home, then → until the list ends. At every key the active descendant is an option inside the
// canvas, chosen, of the moment the story has next in time, with its title and its place in all 2,700, and every
// moment is reached once. Chrome's own accessibility tree is read at five of them: the first, one past the spike's 200,
// the middle, the last, and one in a group. PageDown and PageUp go ten moments. At the zoom of groups, the whole stage
// (lod.ts): Home, then → until the list ends again; every moment once, each group of the stage one stretch of the
// list, the groups in their order on the stage, each group of several named once, «Group k of n, m moments, lane»;
// PageDown and PageUp go a group; End is the list's last entry. The live lines keep quiet while the stage holds the
// focus and speak from the transport; a hidden line says when the level of detail changes.
//
// The negative controls (harness/main.ts, `list=`): no list, as before #490, must fail at the first moment, and the
// renderer spike's list, which stopped at 200 items, exactly at the 201st. The arrows chose every moment before #490
// too (#439/D38): what fails without the list is what a screen reader is told.
import { messages } from "./messages.js";

const ROUTE = "#/moviola/x100?data=demo&lang=en";
const CANVAS = `document.querySelector(".stage canvas")`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const KEYS = {
  Home: ["Home", 36],
  End: ["End", 35],
  PageUp: ["PageUp", 33],
  PageDown: ["PageDown", 34],
  ArrowLeft: ["ArrowLeft", 37],
  ArrowRight: ["ArrowRight", 39],
  Tab: ["Tab", 9],
};

/** A key as a keyboard sends it, to whatever holds the focus. */
async function key(page, name) {
  const [code, keyCode] = KEYS[name];
  await page.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: name, code, windowsVirtualKeyCode: keyCode });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: name, code, windowsVirtualKeyCode: keyCode });
}

/** What the list says now, in one read of the page: the active descendant and what it is. */
const READ = `(() => {
  const canvas = ${CANVAS};
  const id = canvas.getAttribute("aria-activedescendant");
  const option = id ? document.getElementById(id) : null;
  const group = option && option.parentElement && option.parentElement.getAttribute("role") === "group" ? option.parentElement : null;
  const label = group ? document.getElementById(group.getAttribute("aria-labelledby"))?.textContent ?? "" : null;
  return {
    found: option !== null,
    inside: option !== null && canvas.contains(option),
    role: option?.getAttribute("role") ?? null,
    selected: option?.getAttribute("aria-selected") ?? null,
    moment: option?.dataset.id ?? null,
    text: option?.textContent ?? "",
    pos: Number(option?.getAttribute("aria-posinset") ?? 0),
    size: Number(option?.getAttribute("aria-setsize") ?? 0),
    group: group ? Number(group.dataset.group) : null,
    label,
    options: canvas.querySelectorAll("[role=option]").length,
    chosen: document.getElementById("moviola-status")?.dataset.selected ?? null,
  };
})()`;

/** What a step should not be: an empty string when it is right. */
function wrongAtDetail(state, e, expected) {
  const want = expected[e];
  if (!state.found) return "no active descendant in the page";
  if (!state.inside || state.role !== "option" || state.selected !== "true") return `not a chosen option of the canvas (${state.role}, ${state.selected})`;
  if (state.moment !== want.id || state.chosen !== want.id) return `the option of ${state.moment}, the stage's ${state.chosen}, for ${want.id}`;
  if (!state.text.includes(want.title)) return `its words lack the title: ${state.text.slice(0, 80)}`;
  if (state.pos !== e + 1 || state.size !== expected.length) return `place ${state.pos} of ${state.size}`;
  if (state.options > 31) return `${state.options} options in the page`;
  return "";
}

/** Home, then → until the list ends or a step is wrong: how many entries were right, and the first wrong one. */
async function walk(page, wrong, total, sample = () => {}) {
  await page.evaluate(`${CANVAS}.focus()`);
  await key(page, "Home");
  const states = [];
  for (let e = 0; e < total; e++) {
    if (e > 0) await key(page, "ArrowRight");
    const state = await page.evaluate(READ);
    const why = wrong(state, e, states);
    if (why) return { reached: e, failure: `entry ${e + 1}: ${why}`, states };
    states.push(state);
    await sample(e, state);
  }
  // The end holds.
  await key(page, "ArrowRight");
  const after = await page.evaluate(READ);
  const held = after.moment === states.at(-1)?.moment;
  return { reached: total, failure: held ? null : `→ after the last went on to ${after.moment}`, states };
}

/** The moment chosen as Chrome's accessibility tree has it: the focused listbox, its active descendant, its parent. */
async function inTree(page) {
  const { nodes } = await page.send("Accessibility.getFullAXTree");
  const value = (node, name) => node?.properties?.find((entry) => entry.name === name)?.value;
  const box = nodes.find((node) => node.role?.value === "listbox" && value(node, "focused")?.value === true);
  const ref = value(box, "activedescendant")?.relatedNodes?.[0]?.backendDOMNodeId;
  const option = nodes.find((node) => ref !== undefined && node.backendDOMNodeId === ref);
  const parent = option ? nodes.find((node) => node.nodeId === option.parentId) : undefined;
  return {
    listbox: box?.name?.value ?? null,
    role: option?.role?.value ?? null,
    name: option?.name?.value ?? "",
    selected: value(option, "selected")?.value === true,
    parent: parent ? `${parent.role?.value} ${JSON.stringify(parent.name?.value ?? "")}` : null,
  };
}

/** A fresh load of `route`, its stage drawn. */
let loads = 0;
async function fresh(page, harness, route) {
  loads += 1;
  await page.goto(`${harness}?load=access${loads}${route}`);
  await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(route)}`, 30_000);
  await page.waitFor(`Number(${CANVAS}?.dataset.frames) > 0`, 10_000);
}

export const access = {
  browser: true,
  async run({ ui, page, harness }) {
    const errors = page.consoleErrors.length;
    const started = Date.now();
    await page.send("Accessibility.enable");
    const m = await import("../src/lib/moviola/model.ts");
    const s = await import("../src/lib/moviola/scene.ts");
    const l = await import("../src/lib/moviola/lod.ts");
    const { syntheticTimeline } = await import("../harness/synthetic.ts");
    const en = messages(ui, "en");
    const model = m.prepare(syntheticTimeline(100), 0);
    const expected = model.nodes.map((node) => ({ id: node.milestone.id, title: node.milestone.title }));
    const total = expected.length;
    const results = [];
    const check = (name, ok, detail = "") => results.push([name, ok, detail]);
    await page.resize(1440, 900);

    // The zoom of detail: every moment by the arrows, in time order, each one told.
    await fresh(page, harness, ROUTE);
    const samples = {};
    const at = new Set([0, 250, total >> 1, total - 1]);
    const detail = await walk(page, (state, e) => wrongAtDetail(state, e, expected), total, async (e) => {
      if (at.has(e)) samples[e] = await inTree(page);
    });
    const reached = new Set(detail.states.map((state) => state.moment));
    check(`the zoom of detail: Home, then → reaches each of the ${total} moments in time order, each the canvas's chosen option with its title and place`,
      detail.failure === null && detail.reached === total && reached.size === total, detail.failure ?? `${detail.reached} entries, ${reached.size} moments`);
    const sampled = Object.entries(samples).map(([e, node]) => [Number(e), node]);
    const toldRight = sampled.filter(([e, node]) => node.role === "option" && node.selected && node.name.includes(expected[e].title) && node.listbox === en["moviola-stage"]);
    check("Chrome's tree: the focused listbox names the moment chosen, at the first, past the 200th, the middle and the last",
      sampled.length === 4 && toldRight.length === 4, sampled.map(([e, node]) => `${e + 1}: ${node.role} «${node.name.slice(0, 60)}»`).join("; "));

    // PageUp and PageDown at the zoom of detail: ten moments.
    await key(page, "Home");
    await key(page, "PageDown");
    const ten = await page.evaluate(READ);
    await key(page, "PageDown");
    await key(page, "PageUp");
    const back = await page.evaluate(READ);
    check("PageDown and PageUp go ten moments", ten.moment === expected[10].id && back.moment === expected[10].id, `${ten.pos}, ${back.pos}`);

    // The live lines: quiet while the stage holds the focus, speaking from the transport.
    const live = () => page.evaluate(`[document.getElementById("moviola-status").getAttribute("aria-live"), document.querySelector(".at-date")?.getAttribute("aria-live") ?? null]`);
    const onStage = await live();
    await key(page, "Tab");
    const away = await page.evaluate(`document.activeElement?.getAttribute("aria-label") ?? ""`);
    const fromTransport = await live();
    await page.evaluate(`document.activeElement.click()`);
    await sleep(50);
    const told = await page.evaluate(`document.getElementById("moviola-status").dataset.selected`);
    check("the status line and the panel at the playhead keep quiet on the stage, and speak from the transport",
      JSON.stringify(onStage) === '["off","off"]' && JSON.stringify(fromTransport) === '["polite","polite"]' && told === expected[9].id,
      `on the stage ${onStage}; from «${away}» ${fromTransport}, which chose ${told}`);

    // The zoom of groups: the wheel takes the stage to the whole of itself.
    await page.evaluate(`${CANVAS}.focus()`);
    const [left, top, width, height] = await page.evaluate(`(() => { const r = ${CANVAS}.getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })()`);
    await page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: left + width / 2, y: top + height / 3, deltaX: 0, deltaY: 6000, buttons: 0 });
    await page.waitFor(`${CANVAS}.dataset.groups !== ""`, 5_000).catch(() => {});
    await page.waitFor(`document.getElementById("moviola-level")?.dataset.said !== ""`, 5_000).catch(() => {});
    const level = await page.evaluate(`document.getElementById("moviola-level").dataset.said`);
    const [w, h] = (await page.evaluate(`${CANVAS}.dataset.view`)).split(" ").map(Number);
    const z = Number((await page.evaluate(`${CANVAS}.dataset.camera`)).split(" ")[2]);
    const lay = m.layout(model, w, h, null);
    const scene = s.buildScene(model, lay);
    const sceneOf = new Map();
    scene.node.forEach((index, k) => sceneOf.set(index, k));
    const groups = l.groupsAt(scene, model, l.groupZoom(z));
    const several = [];
    for (let g = 0; g < groups.n; g++) if (l.size(groups, g) > 1) several.push(g);
    const groupOf = (id) => groups.of[sceneOf.get(model.byId.get(id).index)];
    const ordinal = new Map(several.map((g, i) => [g, i + 1]));
    const laneOf = (g) => (model.lanes[groups.lane[g]].id === model.timeline.main_direction ? en["moviola-lane-main"] : model.lanes[groups.lane[g]].id);
    check("past the zoom of groups, the hidden line says so", level === en["moviola-list-level-groups"], `«${level}» at zoom ${z}`);

    let lastGroup = -1;
    const left2 = new Set();
    const wrongAtGroups = (state, e, before) => {
      if (!state.found) return "no active descendant in the page";
      if (!state.inside || state.role !== "option" || state.selected !== "true" || state.chosen !== state.moment) return `not the canvas's chosen option (${state.role}, ${state.selected})`;
      const g = groupOf(state.moment);
      if (g === undefined) return `${state.moment} in no group`;
      if (g !== lastGroup) {
        if (g < lastGroup || left2.has(g)) return `group ${g} after group ${lastGroup}: not the stage's order, or one group in two stretches`;
        if (lastGroup >= 0) left2.add(lastGroup);
        lastGroup = g;
      }
      const n = l.size(groups, g);
      if (n === 1) {
        if (state.group !== null) return "a group of one in a group of options";
        if (state.pos !== e + 1 || state.size !== total) return `a moment alone at place ${state.pos} of ${state.size}`;
      } else {
        if (state.group !== g) return `in the group of options ${state.group}, on the stage ${g}`;
        const said = `Group ${ordinal.get(g)} of ${several.length}, ${n} moments, ${laneOf(g)}, `;
        if (!state.label.startsWith(said)) return `the group named «${state.label.slice(0, 70)}» for «${said}»`;
        const first = before.findIndex((earlier) => groupOf(earlier.moment) === g);
        const place = first < 0 ? 1 : e - first + 1;
        if (state.pos !== place || state.size !== n) return `place ${state.pos} of ${state.size} in its group, for ${place} of ${n}`;
      }
      if (!state.text.includes(model.byId.get(state.moment).milestone.title)) return "its words lack the title";
      return "";
    };
    let groupSample = null;
    const grouped = await walk(page, wrongAtGroups, total, async (e, state) => {
      if (!groupSample && state.group !== null && state.pos === 2) groupSample = await inTree(page);
    });
    const covered = new Set(grouped.states.map((state) => state.moment));
    check(`the zoom of groups (${groups.n} groups, ${several.length} of several): Home, then → reaches each moment once, group by group in the stage's order, each group named with its moments`,
      grouped.failure === null && covered.size === total && several.length > 0, grouped.failure ?? `${covered.size} moments`);
    check("Chrome's tree: an option of a group of several sits in a group named by its place and its moments",
      groupSample !== null && groupSample.role === "option" && groupSample.selected && /^group "Group \d+ of \d+, \d+ moments, /.test(groupSample.parent ?? ""),
      groupSample ? `${groupSample.parent} › ${groupSample.role} «${groupSample.name.slice(0, 50)}»` : "no option in a group of several");

    // PageDown and PageUp go a group; End is the list's last entry.
    await key(page, "Home");
    await key(page, "PageDown");
    const next = await page.evaluate(READ);
    await key(page, "ArrowRight");
    await key(page, "PageUp");
    const start = await page.evaluate(READ);
    const firstOfNext = grouped.states.find((state) => groupOf(state.moment) !== groupOf(grouped.states[0].moment));
    await key(page, "End");
    const end = await page.evaluate(READ);
    check("at the zoom of groups PageDown goes to the next group, PageUp back to its start, End to the list's last entry",
      next.moment === firstOfNext?.moment && start.moment === firstOfNext?.moment && end.moment === grouped.states.at(-1)?.moment,
      `${next.moment} for ${firstOfNext?.moment}; ${start.moment}; End ${end.moment} for ${grouped.states.at(-1)?.moment}`);

    // Back to the zoom of detail: the hidden line says so.
    await page.evaluate(`${CANVAS}.focus()`);
    await page.press("0");
    await page.waitFor(`${CANVAS}.dataset.groups === ""`, 5_000).catch(() => {});
    await sleep(50);
    const backIn = await page.evaluate(`document.getElementById("moviola-level").dataset.said`);
    check("back at the zoom of detail, the hidden line says so", backIn === en["moviola-list-level-detail"], `«${backIn}»`);

    // The negative controls: no list at all, and the spike's list of 200.
    await fresh(page, harness, `${ROUTE}&list=none`);
    const none = await walk(page, (state, e) => wrongAtDetail(state, e, expected), total);
    await fresh(page, harness, `${ROUTE}&list=capped`);
    const capped = await walk(page, (state, e) => wrongAtDetail(state, e, expected), total);
    const caughtNone = none.reached === 0;
    const caughtCapped = capped.reached === 200;

    for (const [name, ok, detail] of results) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
    console.log(`negative control, no list (as before #490): ${caughtNone ? "caught" : "NOT CAUGHT"} (${none.failure ?? "no failure"})`);
    console.log(`negative control, the spike's list of 200: ${caughtCapped ? "caught" : "NOT CAUGHT"} (${capped.failure ?? "no failure"})`);
    console.log(`time: ${((Date.now() - started) / 1000).toFixed(1)} s for ${2 * total + 200} arrows`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return results.every(([, ok]) => ok) && caughtNone && caughtCapped && newErrors.length === 0;
  },
};
