// The moviola's model, layout and scene against the prototype's viewer: on the fixtures of harness/moviola.json
// (the synthetic dossiers of fixtures/moviola/ as the core gives them, and the case of the dossier's
// test_moviola_viewer.py), the same lanes in the same order, the same nodes in each lane with their rings and
// badges, the same x and y at three stage sizes, with and without a window, and the same time scale with its
// joints, as prove/moviola-record.mjs recorded them from the prototype. Then the scene: nodes ascending in x,
// each found by the grid where it is drawn, one edge per parent. The negative controls: the lanes in the order
// of the directions' file, which the prototype's static graph uses, and the columns moved by one pixel, must
// both be caught. No browser: Node runs the TypeScript as it is, its types stripped.
//
// Then what the renderer stands on, against the prototype where it has an answer and by invariants where it has
// none: the rings' places (`yAnello`) and a lane's half thickness (`semiSpessore`) as the prototype computes them;
// the spike's labels (`pickLabels`), which the prototype does not have, by what they must hold on a crowded lane:
// only turning points, no two on one cell, none below the zoom of labels, at most the cap; with a control that
// labels every turning point and must be caught overlapping. The camera keeps the stage in view and the point
// under the pointer where it is; the lanes' envelopes and the days' first nodes; the portfolio's order and names,
// and a catalog id for every kind and health of the core.
import { readFileSync } from "node:fs";
import { join } from "node:path";

const EPSILON = 1e-6;

/** The differences between what the prototype answered and what the product computes, each with its path. */
function differences(expected, actual, path, out) {
  if (typeof expected === "number" && typeof actual === "number") {
    if (!(Math.abs(expected - actual) <= EPSILON)) out.push(`${path}: ${expected} ≠ ${actual}`);
  } else if (Array.isArray(expected)) {
    if (!Array.isArray(actual) || actual.length !== expected.length) {
      out.push(`${path}: ${JSON.stringify(expected)} ≠ ${JSON.stringify(actual)}`);
      return out;
    }
    expected.forEach((item, i) => differences(item, actual[i], `${path}[${i}]`, out));
  } else if (expected && typeof expected === "object") {
    if (!actual || typeof actual !== "object") out.push(`${path}: ${JSON.stringify(expected)} ≠ ${JSON.stringify(actual)}`);
    else for (const key of Object.keys(expected)) differences(expected[key], actual[key], `${path}.${key}`, out);
  } else if (expected !== actual) {
    out.push(`${path}: ${JSON.stringify(expected)} ≠ ${JSON.stringify(actual)}`);
  }
  return out;
}

/** The product's answers in the recording's shape. */
function answers(m, c, timeline) {
  const model = m.prepare(timeline, 0);
  const ids = (list) => list.map((i) => model.nodes[i].milestone.id);
  const lanes = model.lanes.map((l) => ({ ...l, nodes: ids(l.nodes) }));
  const nodes = model.nodes.map((n) => ({
    id: n.milestone.id, lane: n.lane, t: n.t, children: n.children,
    rings: n.rings.map((r) => ({ declared: r.declared, model: r.model ? r.model.model : null })),
    badges: n.badges, declared: n.declared,
  }));
  const layouts = c.expected.layouts.map((e) => {
    const l = m.layout(model, e.width, e.height, e.window);
    return {
      width: e.width, height: e.height, window: e.window, x: l.x, y: l.y, laneY: l.laneY, step: l.step, left: l.left,
      i0: l.i0, i1: l.i1, clip: l.clip, stageWidth: l.width, stageHeight: l.height,
      xAt: e.xAt.map(([t]) => [t, l.xAt(t)]), tAt: e.tAt.map(([x]) => [x, l.tAt(x)]),
    };
  });
  const scale = m.timeScale(model.nodes.map((n) => n.t), 0, 800);
  return {
    model,
    recorded: {
      lanes, nodes, layouts,
      scale: { x0: 0, x1: 800, x: c.expected.scale.x.map(([t]) => [t, scale.x(t)]), joints: scale.joints },
    },
  };
}

/** What a scene must hold whatever the data: x ascending, each node found where it is, one edge per parent. */
function sceneProblems(m, s, model) {
  const problems = [];
  const l = m.layout(model, 1280, 720, null);
  const scene = s.buildScene(model, l);
  for (let i = 1; i < scene.n; i++) if (scene.x[i] < scene.x[i - 1]) problems.push(`x not ascending at ${i}`);
  for (let i = 0; i < scene.n; i++) {
    const found = s.hitTest(scene, scene.x[i], scene.y[i] + 2, 6);
    if (found !== i) problems.push(`hitTest at node ${i} found ${found}`);
  }
  const parents = model.nodes.reduce((sum, n) => sum + n.milestone.parents.filter((p) => model.byId.has(p)).length, 0);
  if (scene.edges.n !== parents) problems.push(`${scene.edges.n} edges for ${parents} parents`);
  if (scene.bands.n !== model.lanes.length) problems.push(`${scene.bands.n} bands for ${model.lanes.length} lanes`);
  const [lo, hi] = s.visibleRange(scene, { cx: l.width / 2, cy: l.height / 2, z: 1 }, { w: l.width + 200, h: l.height });
  if (lo !== 0 || hi !== scene.n) problems.push(`the whole stage in view shows ${lo}..${hi} of ${scene.n}`);
  return problems;
}

export const moviola = {
  browser: false,
  async run({ ui }) {
    const m = await import("../src/lib/moviola/model.ts");
    const s = await import("../src/lib/moviola/scene.ts");
    const fixture = JSON.parse(readFileSync(join(ui, "harness/moviola.json"), "utf8"));
    let ok = true;
    for (const c of fixture.cases) {
      const { model, recorded } = answers(m, c, c.timeline);
      const found = differences(c.expected, recorded, c.name, []);
      const scene = sceneProblems(m, s, model);
      console.log(`${c.name}: ${model.nodes.length} nodes, ${model.lanes.length} lanes (${model.lanes.map((l) => l.id).join(", ")}),`
        + ` ${c.expected.layouts.length} layouts: ${found.length ? "DIFFERENT" : "the prototype's"}; scene: ${scene.length ? "WRONG" : "ok"}`);
      for (const line of [...found, ...scene].slice(0, 12)) console.log(`  ${line}`);
      ok &&= found.length === 0 && scene.length === 0;
    }
    // The prototype's own viewer case, as test_moviola_viewer.py asserts it.
    const viewer = fixture.cases.find((c) => c.name === "viewer");
    const columns = answers(m, viewer, viewer.timeline).recorded.layouts[0].x;
    const literal = JSON.stringify(columns) === JSON.stringify([150, 306, 462, 618, 774]);
    console.log(`viewer at 800×400: columns ${JSON.stringify(columns)} ${literal ? "as test_moviola_viewer.py" : "NOT as test_moviola_viewer.py"}`);
    ok &&= literal;

    // Negative controls: each must be caught.
    const alpha = fixture.cases.find((c) => c.name === "alpha");
    const { recorded } = answers(m, alpha, alpha.timeline);
    const fileOrder = [alpha.timeline.main_direction, ...alpha.timeline.directions.map((d) => d.id)];
    const reordered = { ...recorded, lanes: fileOrder.map((id) => recorded.lanes.find((l) => l.id === id)).filter(Boolean) };
    const caughtOrder = differences(alpha.expected.lanes, reordered.lanes, "alpha", []).length > 0;
    console.log(`negative control, alpha's lanes in the directions' file order: ${caughtOrder ? "caught" : "NOT CAUGHT"}`);
    const moved = structuredClone(viewer.expected);
    moved.layouts[0].x[2] += 1;
    const caughtMoved = differences(moved, answers(m, viewer, viewer.timeline).recorded, "viewer", []).length > 0;
    console.log(`negative control, a column moved by one pixel: ${caughtMoved ? "caught" : "NOT CAUGHT"}`);

    const parts = await renderer(ui, m, s, fixture);
    const detail = await lodChecks(m, s);
    const frames = await frameChecks(m, fixture);
    const found = await searchChecks(m, fixture);
    const packs = await packChecks(m, fixture);
    const tape = tapeChecks(m, fixture);
    const overview = await overviewChecks(m, fixture);
    return ok && caughtOrder && caughtMoved && parts && detail && frames && found && packs && tape && overview;
  },
};

/** How far apart two scales put the same instants, in pixels. */
const deviation = (expected, scale) => Math.max(0, ...expected.x.map(([t, x]) => Math.abs(scale.x(t) - x)), ...expected.joints.map((j, i) => Math.abs((scale.joints[i]?.x ?? Infinity) - j.x)));

/** The share of the width between the first and the last milestone. */
const share = (scale, times, x0, x1) => (scale.x(Math.max(...times)) - scale.x(Math.min(...times))) / (x1 - x0);

/**
 * The tape and the strip (#486/V18, #598): on the dense story the product's scale is the prototype's within a pixel,
 * on alpha the story takes at least half the tape (with the prototype's, 130 px of 1,420). The negative controls:
 * the prototype's own scale on alpha must fail the half, and joints always as wide as twelve hours of story (a rule
 * that widens them on a dense story too) must fail the tolerance.
 */
function tapeChecks(m, fixture) {
  const { width, strip, stories } = fixture.tape;
  const TOLERANCE = 1;
  const checks = [];
  const check = (name, ok, detail = "") => checks.push([name, ok, detail]);
  const dense = stories.dense;
  const onTape = m.tapeScale(dense.times, 10, width - 10);
  const onStrip = m.tapeScale(dense.times, 8, strip - 8, 12 * m.HOUR, 18);
  const [dTape, dStrip] = [deviation(dense.tape, onTape), deviation(dense.strip, onStrip)];
  check(`dense story, tape: the prototype's scale within ${TOLERANCE} px`, dTape <= TOLERANCE && onTape.joints.length === dense.tape.joints.length, `${dTape.toFixed(3)} px`);
  check(`dense story, strip: the prototype's scale within ${TOLERANCE} px`, dStrip <= TOLERANCE, `${dStrip.toFixed(3)} px`);
  const shares = {};
  for (const name of ["alpha", "viewer", "frame"]) {
    const { times } = stories[name];
    const product = share(m.tapeScale(times, 10, width - 10), times, 10, width - 10);
    const prototype = share({ x: (t) => stories[name].tape.x.find(([at]) => at === t)[1] }, times, 10, width - 10);
    const narrow = share(m.tapeScale(times, 8, strip - 8, 12 * m.HOUR, 18), times, 8, strip - 8);
    shares[name] = { product, prototype, narrow };
  }
  const a = shares.alpha;
  check("alpha: the story takes at least half the tape, and of the strip", a.product >= 0.5 && a.narrow >= 0.5,
    `tape ${(a.product * 100).toFixed(1)} %, strip ${(a.narrow * 100).toFixed(1)} %; the prototype ${(a.prototype * 100).toFixed(1)} %`);
  check("viewer and frame, as sparse: at least half too", shares.viewer.product >= 0.5 && shares.frame.product >= 0.5,
    `${(shares.viewer.product * 100).toFixed(1)} %, ${(shares.frame.product * 100).toFixed(1)} %`);
  const ends = m.tapeScale(stories.alpha.times, 10, width - 10);
  check("the tape still runs from end to end, and back", Math.abs(ends.x(-Infinity) - 10) < 1e-6 && Math.abs(ends.x(Infinity) - (width - 10)) < 1e-6
    && stories.alpha.times.every((t) => Math.abs(ends.t(ends.x(t)) - t) < 1));
  // The controls.
  const caughtSparse = a.prototype < 0.5;
  const twelveHours = (times, x0, x1, threshold = 12 * m.HOUR) => {
    const ts = [...new Set(times)].sort((p, q) => p - q);
    const knots = [ts[0] - m.HOUR, ...ts, ts.at(-1) + m.HOUR];
    const gaps = knots.slice(1).map((t, i) => t - knots[i]);
    const live = gaps.filter((g) => g <= threshold).reduce((sum, g) => sum + g, 0);
    const k = (x1 - x0) / (live + gaps.filter((g) => g > threshold).length * threshold);
    const xs = [x0];
    for (const g of gaps) xs.push(xs.at(-1) + (g > threshold ? threshold : g) * k);
    return { x: (t) => m.interpolate(knots, xs, t), joints: [] };
  };
  const caughtDense = deviation({ ...dense.tape, joints: [] }, twelveHours(dense.times, 10, width - 10)) > TOLERANCE;
  for (const [name, ok, detail] of checks) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
  console.log(`negative control, the prototype's tape on alpha: ${caughtSparse ? "caught" : "NOT CAUGHT"} (${(a.prototype * 100).toFixed(1)} %)`);
  console.log(`negative control, joints always as wide as twelve hours: ${caughtDense ? "caught" : "NOT CAUGHT"}`);
  return checks.every(([, ok]) => ok) && caughtSparse && caughtDense;
}

/**
 * The overview (#598) against the prototype's answers on every case: the filters (`visibileFiltri`) keep the same
 * moments for each setting, with the same «who» to choose from; the state at a date (the panel «alla data»), its open
 * decisions from the last milestone's sidecar; the directions' cards (`disegnaL1`) and each one's moments; the time
 * window (`impostaFinestra`). Where the prototype's card has a slip, a lane with no record titled «Linea principale»
 * and a lane with no reason given the main lane's sentence, the product says what it knows instead, and the check
 * says so. The negative controls: a filter that ignores the kinds hidden, and a state at a date that counts the
 * milestone at the instant as after it, must both be caught.
 */
async function overviewChecks(m, fixture) {
  const f = await import("../src/lib/moviola/filters.ts");
  const checks = [];
  const check = (name, ok, detail = "") => checks.push([name, ok, detail]);
  const models = Object.fromEntries(fixture.cases.map((c) => [c.name, { model: m.prepare(c.timeline, 0), c }]));
  const settingOf = (s) => ({ ...f.NO_FILTERS, hidden: s.hide, who: s.who, lane: s.lane, health: s.health, declared: s.declared });
  const keep = (model, filters, visible = f.visible) => model.nodes.filter((n) => visible(n, filters)).map((n) => n.milestone.id);
  let filtered = [];
  let settings = 0;
  let whoSame = true;
  for (const [name, { model }] of Object.entries(models)) {
    const recorded = fixture.filters[name];
    whoSame &&= JSON.stringify(f.whoOptions(model).slice().sort()) === JSON.stringify(recorded.who.slice().sort());
    for (const s of recorded.settings) {
      settings += 1;
      const ids = keep(model, settingOf(s));
      if (JSON.stringify(ids) !== JSON.stringify(s.visible)) filtered.push(`${name} ${JSON.stringify({ ...s, visible: undefined })}: ${ids.length} ≠ ${s.visible.length}`);
    }
  }
  check(`filters: the prototype's moments for ${settings} settings on every case`, filtered.length === 0, filtered.slice(0, 3).join("; "));
  check("filters: the same «who» to choose from, none declared last", whoSame);
  check("filters: back to basic clears the advanced ones, and an advanced one makes the mode advanced",
    f.modeOf({ ...f.NO_FILTERS, lane: "x" }) === "advanced" && f.modeOf(f.NO_FILTERS) === "basic" && !f.advancedOn(f.basic({ ...f.NO_FILTERS, who: f.NONE, declared: true })));
  const address = f.fromAddress("advanced", "gate,nothing,exception");
  check("filters: an address opens a mode and hides the kinds it names, nothing else", address.mode === "advanced" && JSON.stringify(address.hidden) === '["gate","exception"]'
    && f.fromAddress("other", null).mode === null);

  let states = [];
  let samples = 0;
  for (const [name, { model, c }] of Object.entries(models)) {
    for (const r of fixture.states[name]) {
      samples += 1;
      const s = m.stateAt(model, r.t);
      const decisions = s.last ? (c.snapshots[s.last.milestone.id]?.open_decisions ?? []) : [];
      const said = { t: r.t, before: s.before, last: s.last ? s.last.milestone.id : null, open: s.open.map((l) => l.id), decisions, after: s.after };
      if (JSON.stringify(said) !== JSON.stringify(r)) states.push(`${name} at ${new Date(r.t).toISOString()}: ${JSON.stringify(said)}`);
    }
  }
  check(`the state at a date: the prototype's at ${samples} instants, open decisions from the last sidecar`, states.length === 0, states.slice(0, 2).join("; "));

  const cards = [];
  let compared = 0;
  for (const [name, { model }] of Object.entries(models)) {
    const product = m.laneCards(model);
    fixture.lanes[name].forEach((r, i) => {
      compared += 1;
      const card = product[i];
      const main = card?.lane.id === model.timeline.main_direction;
      const said = card && {
        id: card.lane.id, status: card.lane.status, count: card.lane.nodes.length,
        openedBy: card.openedBy?.milestone.id ?? null, closedBy: card.closedBy?.milestone.id ?? null,
        list: card.lane.nodes.map((k) => model.nodes[k].milestone.id),
      };
      const same = said && Object.keys(said).every((key) => JSON.stringify(said[key]) === JSON.stringify(r[key]));
      // The title and the reason: the record's, where there is one; the main lane's are the catalog's words.
      const title = main ? card.lane.title === null : card.lane.recorded ? card.lane.title === r.title : card.lane.title === null && r.title === "Linea principale";
      const reason = card && (card.lane.reason ? card.lane.reason === r.reason : r.reason.startsWith("La linea del "));
      if (!same || !title || !reason) cards.push(`${name}/${r.id}: ${JSON.stringify(said)}`);
    });
  }
  check(`the directions: the prototype's ${compared} cards, each with its moments once chosen`, cards.length === 0, cards.slice(0, 2).join("; "));

  let windows = [];
  for (const name of ["viewer", "alpha"]) {
    const { model } = models[name];
    for (const r of fixture.windows[name]) {
      const w = m.chooseWindow(model, r.ask[0], r.ask[1]);
      if (JSON.stringify(w) !== JSON.stringify(r.window)) windows.push(`${name} ${JSON.stringify(r.ask)}: ${JSON.stringify(w)} ≠ ${JSON.stringify(r.window)}`);
    }
  }
  check("the time window: the prototype's for every ask, the whole story as none", windows.length === 0, windows.slice(0, 2).join("; "));

  // The controls.
  const alpha = models.alpha.model;
  const deaf = (node, filters) => f.visible(node, { ...filters, hidden: [] });
  const caughtKinds = fixture.filters.alpha.settings.some((s) => JSON.stringify(keep(alpha, settingOf(s), deaf)) !== JSON.stringify(s.visible));
  const strict = (model, t) => ({ ...m.stateAt(model, t - 1) });
  const caughtStrict = fixture.states.alpha.some((r) => strict(alpha, r.t).before !== r.before);
  for (const [name, ok, detail] of checks) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
  console.log(`negative control, a filter deaf to the kinds hidden: ${caughtKinds ? "caught" : "NOT CAUGHT"}`);
  console.log(`negative control, the milestone at the instant counted after it: ${caughtStrict ? "caught" : "NOT CAUGHT"}`);
  return checks.every(([, ok]) => ok) && caughtKinds && caughtStrict;
}

/**
 * The search (#487), which the prototype viewer does not have: by what it must hold on every case. Every moment
 * found has every word and every #number of the query, every moment left out lacks one; the moments found come
 * newest first; a #number is the task a moment names or a `#N` of its title, never a longer number; case and
 * accents do not count; `,` and `.` walk the moments found in time. The negative control: a #number looked for as
 * a substring of the title, the mistake that finds #12 for #1, must be caught.
 */
async function searchChecks(m, fixture) {
  const q = await import("../src/lib/moviola/search.ts");
  const checks = [];
  const check = (name, ok, detail = "") => checks.push([name, ok, detail]);
  const names = { kind: (node) => node.milestone.kind, lane: () => "" };
  const queries = ["probe", "PROBE b", "#12", "#14", "#1", "#99 over", "gate", "zzzz", "#12 merged", "probé"];
  let consistent = true;
  for (const c of fixture.cases) {
    const model = m.prepare(c.timeline, 0);
    for (const text of queries) {
      const query = q.parseQuery(text);
      const found = q.search(model, query, names);
      const ids = new Set(found.map((n) => n.index));
      const newestFirst = found.every((n, i) => i === 0 || found[i - 1].t >= n.t);
      const words = (n) => query.words.every((w) => q.fold(`${n.milestone.title} ${n.milestone.kind}`).includes(w));
      const tasks = (n) => query.tasks.every((t) => n.milestone.refs.issue.includes(t) || new RegExp(`#${t}(?!\\d)`).test(n.milestone.title));
      const exact = model.nodes.every((n) => ids.has(n.index) === (words(n) && tasks(n)));
      if (!newestFirst || !exact) {
        consistent = false;
        console.log(`  ${c.name} «${text}»: ${newestFirst ? "" : "not newest first; "}${exact ? "" : "not exactly the moments that match"}`);
      }
    }
  }
  check("every case and query: exactly the moments that match, newest first", consistent);
  const frameModel = m.prepare(fixture.cases.find((c) => c.name === "frame").timeline, 0);
  const ids = (text) => q.search(frameModel, q.parseQuery(text), names).map((n) => n.milestone.id);
  check("#12 finds the moment that names it, #1 none", ids("#12").length === 1 && ids("#1").length === 0, `${ids("#12")} / ${ids("#1")}`);
  check("case and accents do not count", q.fold("Perché PROBE") === "perche probe" && ids("MERGED").length === 1);
  check("an empty query is no search", q.isEmpty(q.parseQuery("   ")) && q.search(frameModel, q.parseQuery(""), names).length === 0);
  const all = q.search(frameModel, q.parseQuery("probe"), names);
  const [early, late] = [...all].sort((a, b) => a.t - b.t);
  check("`.` and `,` walk the moments found in time", q.stepWithin(all, -1, 1) === early && q.stepWithin(all, early.index, 1) === late
    && q.stepWithin(all, late.index, 1) === late && q.stepWithin(all, -1, -1) === late);
  const naive = frameModel.nodes.filter((n) => n.milestone.title.includes("#1")).length;
  const caught = naive > 0 && ids("#1").length === 0;
  for (const [name, ok, detail] of checks) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
  console.log(`negative control, #1 as a substring of the title: ${caught ? "caught" : "NOT CAUGHT"}`);
  return checks.every(([, ok]) => ok) && caught;
}

/**
 * «Copy for chat» (#487) against the prototype's `pacchetto`: given the prototype's words, its kind and day, and its
 * page of the snapshot in place of the sidecar's path, the product's `pack` writes the same text, byte for byte, on
 * every milestone, the long one whose summary is shortened and whose references go included. The negative control:
 * a package that drops the references before it shortens the summary must be caught on the long one.
 */
async function packChecks(m, fixture) {
  const p = await import("../src/lib/moviola/pack.ts");
  const w = fixture.packWords;
  const words = {
    head: (a) => `regia · ${a.project} · ${w.node} ${a.id} (${a.kind}, ${a.day})`,
    looking: (what) => w.looking + what,
    references: (list) => w.references + list,
    none: w.none,
    more: (n) => `${w.more}${n}`,
    question: w.question,
    skill: w.skill,
  };
  const different = [];
  let longest = null;
  for (const c of fixture.cases) {
    const model = m.prepare(c.timeline, 0);
    c.frames.forEach((f, i) => {
      const node = model.nodes[i];
      const moment = { project: c.timeline.project, id: node.milestone.id, kind: f.said.kind, day: f.said.day, title: node.milestone.title };
      const refs = p.packReferences(node.milestone.links, f.said.page);
      const text = p.pack(moment, p.packSummary(c.snapshots[node.milestone.id] ?? null), refs, words);
      if (text !== f.pack) different.push(`${c.name}/${node.milestone.id}`);
      if (!longest || f.pack.length > longest.f.pack.length) longest = { f, moment, refs, summary: p.packSummary(c.snapshots[node.milestone.id] ?? null) };
    });
  }
  const shortened = longest.f.pack.length <= p.PACK_LIMIT && longest.f.pack.includes(w.more);
  console.log(`chat packages: ${fixture.cases.reduce((n, c) => n + c.frames.length, 0)}, the prototype's byte for byte: ${different.length ? `NO (${different.join(", ")})` : "yes"}`);
  console.log(`the longest, ${longest.f.pack.length} characters, shortened with its references counted: ${shortened ? "yes" : "NO"}`);
  // The control: the references go before the summary is shortened.
  let naive = "";
  for (let n = longest.refs.length; n >= 0 && !naive; n--) {
    const text = p.pack({ ...longest.moment }, longest.summary, longest.refs.slice(0, n), { ...words, more: () => "" });
    if (text.length <= p.PACK_LIMIT) naive = text;
  }
  const caught = naive !== longest.f.pack;
  console.log(`negative control, the references dropped before the summary is shortened: ${caught ? "caught" : "NOT CAUGHT"}`);
  return different.length === 0 && shortened && caught;
}

/** The product's frame in the recording's shape: the prototype's L3, read back by its sections. */
function neutral(fr) {
  const runs = (list) => list.map((line) => line.map((r) => (r.url === null ? { text: r.text } : { text: r.text, url: r.url })));
  const event = (e) => ({ title: e.title, status: e.status, reason: e.reason });
  const ids = (nodes) => nodes.map((n) => n.milestone.id);
  return {
    id: fr.node.milestone.id,
    title: fr.node.milestone.title,
    lane: fr.lane.id,
    health: fr.node.milestone.health,
    inBrief: fr.inBrief,
    why: {
      direction: fr.why.direction,
      opens: fr.why.opens && { title: fr.why.opens.title, reason: fr.why.opens.reason },
      closes: fr.why.closes.map(event),
    },
    changed: runs(fr.changed),
    openDecisions: runs(fr.openDecisions),
    risks: runs(fr.risks),
    nextStep: fr.next,
    claims: fr.claims,
    deviations: fr.deviations.map((d) => [d.plan, d.deviation, d.reason, d.impact]),
    process: fr.process,
    work: fr.work.map((w) => ({ task: w.task, aggregate: w.aggregate !== null })),
    refs: {
      issue: fr.links.issue.map((x) => ({ text: x.text, url: x.url })),
      commit: fr.links.commit.map((x) => ({ text: x.url ? x.text : x.hash || x.text, url: x.url })),
      report: fr.links.report.map((x) => ({ text: x.text, path: x.path, exists: x.exists })),
      projectUrl: fr.projectUrl,
    },
    parents: ids(fr.parents),
    children: ids(fr.children),
    previous: fr.previous ? fr.previous.milestone.id : null,
    following: fr.following ? fr.following.milestone.id : null,
  };
}

/**
 * The frame (L3, #487) against the prototype's: for every milestone of every case, the product's `frame` over the
 * core's timeline and sidecar says what the prototype's `disegnaL3` drew, section by section, with every `#N` of
 * the three linked sections at the same address. The negative controls: a frame read with the next milestone's
 * sidecar, and a `#N` split without the prototype's look-ahead (so `#14a` links), must both be caught.
 */
async function frameChecks(m, fixture) {
  const f = await import("../src/lib/moviola/frame.ts");
  const recordedKeys = (frame) => Object.fromEntries(Object.entries(frame).filter(([key]) => key !== "pack" && key !== "said"));
  let ok = true;
  let compared = 0;
  for (const c of fixture.cases) {
    const model = m.prepare(c.timeline, 0);
    const found = [];
    c.frames.forEach((expected, i) => {
      const fr = f.frame(model, i, c.snapshots[expected.id] ?? null);
      differences(recordedKeys(expected), fr ? neutral(fr) : null, `${c.name}/${expected.id}`, found);
      compared += 1;
    });
    const linked = c.frames.reduce((sum, fr) => sum + [...fr.changed, ...fr.openDecisions, ...fr.risks].flat().filter((r) => r.url).length, 0);
    console.log(`${c.name}: ${c.frames.length} frames, ${linked} #N links: ${found.length ? "DIFFERENT" : "the prototype's"}`);
    for (const line of found.slice(0, 12)) console.log(`  ${line}`);
    ok &&= found.length === 0 && c.frames.length === c.timeline.milestones.length;
  }
  const frameCase = fixture.cases.find((c) => c.name === "frame");
  const sections = ["changed", "openDecisions", "risks"].every((key) => frameCase.frames.some((fr) => fr[key].flat().some((r) => r.url)));
  console.log(`frames compared: ${compared}; a #N link in each of the three sections: ${sections ? "yes" : "NO"}`);

  // Negative controls.
  const model = m.prepare(frameCase.timeline, 0);
  const first = frameCase.frames[0];
  const swapped = f.frame(model, 0, frameCase.snapshots[frameCase.frames[1].id]);
  const caughtSidecar = differences(recordedKeys(first), neutral(swapped), "frame", []).length > 0;
  const naive = (text, issues) => text.split(/(#\d+)/).filter(Boolean).map((piece) => {
    const hit = issues.find((x) => x.text === piece);
    return hit && hit.url ? { text: piece, url: hit.url } : { text: piece, url: null };
  });
  const loose = neutral(f.frame(model, 0, frameCase.snapshots[first.id]));
  loose.risks = frameCase.snapshots[first.id].risks.map((text) => naive(text, model.nodes[0].milestone.links.issue))
    .map((line) => line.map((r) => (r.url === null ? { text: r.text } : r)));
  const caughtNaive = differences(first.risks, loose.risks, "frame", []).length > 0;
  console.log(`negative control, a frame with the next milestone's sidecar: ${caughtSidecar ? "caught" : "NOT CAUGHT"}`);
  console.log(`negative control, #N split without the look-ahead: ${caughtNaive ? "caught" : "NOT CAUGHT"}`);
  return ok && sections && caughtSidecar && caughtNaive;
}

/** A timeline of `n` moments on the main lane, ten minutes apart, two in three of them decisions: the crowded
 * lane of the labels' check. */
function crowded(n) {
  const id = (i) => `01CROWD${String(i).padStart(19, "0")}`;
  const milestones = Array.from({ length: n }, (_, i) => ({
    id: id(i),
    ts: new Date(Date.UTC(2026, 9, 1, 9, 10 * i)).toISOString(),
    kind: i % 3 === 0 ? "milestone" : "decision",
    title: `A synthetic turning point with a long title, number ${i}`,
    direction: "main",
    parents: i ? [id(i - 1)] : [],
    health: "healthy",
    opens_direction: null,
    refs: { issue: [], commit: [], report: [] },
    process: null,
    author: null,
    links: { issue: [], commit: [], report: [] },
  }));
  return { project: "crowd", project_url: null, main_direction: "main", milestones, directions: [], aggregates: [] };
}

/** A label's plate as the renderer draws it (canvas2d.ts): 3 px either side of its text, 8 above and below. */
const plateOf = (labels, k) => [labels.sx[k] - 3, labels.sy[k] - 8, labels.sx[k] + labels.w[k] + 3, labels.sy[k] + 8];
const overlap = (a, b) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];

/**
 * What a set of labels must hold (#489): their plates, where they are drawn, cover neither one another nor a
 * reserved place, and stay on the stage's height; turning points only, at most the cap, none below the zoom of
 * labels. The spike tested one 16 px row and the place on the right, then drew a label near the stage's right edge on
 * its node's left: two plates could overlap, and its own invariant, one label per cell, said nothing.
 */
function labelProblems(s, scene, labels, cam, view, reserved = []) {
  const problems = [];
  if (labels.n > s.MAX_LABELS) problems.push(`${labels.n} labels over the cap of ${s.MAX_LABELS}`);
  if (cam.z < s.LABEL_ZOOM && labels.n > 0) problems.push(`${labels.n} labels below the zoom of labels`);
  for (let k = 0; k < labels.n; k++) {
    const i = labels.id[k];
    const plate = plateOf(labels, k);
    if (scene.kind[i] === 0) problems.push(`a label on node ${i}, which is not a turning point`);
    if (plate[1] < 0 || plate[3] > view.h) problems.push(`the label of node ${i} off the stage's height`);
    for (let j = 0; j < k; j++) if (overlap(plate, plateOf(labels, j))) problems.push(`the labels of nodes ${labels.id[j]} and ${i} overlap`);
    for (const place of reserved) if (overlap(plate, place)) problems.push(`the label of node ${i} over a reserved place`);
  }
  return problems;
}

/** The renderer spike's picker, as the product had it until #489: left to right, one 16 px row of 8 px cells, the
 * place on the right tested, and the label moved to its node's left at drawing time when it would leave the stage. */
function spikeLabels(s, scene, cam, view, width) {
  const out = s.newLabels();
  const cols = Math.ceil(view.w / 8);
  const rows = Math.ceil(view.h / 16);
  const occupied = new Uint8Array(cols * rows);
  const lo = s.lowerBound(scene.x, cam.cx - view.w / 2 / cam.z - 40);
  const hi = s.lowerBound(scene.x, cam.cx + view.w / 2 / cam.z + 40);
  for (let k = s.lowerBound(scene.labeled, lo); k < scene.labeled.length && out.n < s.MAX_LABELS; k++) {
    const i = scene.labeled[k];
    if (i >= hi) break;
    const sx = s.toScreenX(cam, view, scene.x[i]) + 12;
    const sy = s.toScreenY(cam, view, scene.y[i]);
    const c0 = Math.max(0, Math.floor(sx / 8));
    const c1 = Math.min(cols - 1, Math.floor((sx + width(i)) / 8));
    const r = Math.floor((sy - 8) / 16);
    if (r < 0 || r >= rows) continue;
    let free = true;
    for (let c = c0; c <= c1; c++) if (occupied[r * cols + c]) free = false;
    if (!free) continue;
    for (let c = c0; c <= c1; c++) occupied[r * cols + c] = 1;
    const w = width(i);
    out.id[out.n] = i;
    out.sx[out.n] = sx + w > view.w - 4 ? sx - 24 - w : sx;
    out.sy[out.n] = sy;
    out.w[out.n] = w;
    out.n += 1;
  }
  return out;
}

/** A timeline of `n` moments on two lanes, the kinds in turn: the labels' ranks. */
function ranked(n) {
  const kinds = ["decision", "gate", "exception", "exploration", "rejoin", "abandonment", "milestone"];
  const id = (i) => `01RANK${String(i).padStart(20, "0")}`;
  const milestones = Array.from({ length: n }, (_, i) => ({
    id: id(i),
    ts: new Date(Date.UTC(2026, 9, 1, 9, 10 * i)).toISOString(),
    kind: kinds[i % kinds.length],
    title: `A synthetic moment of rank, number ${i}`,
    direction: i % 2 ? "side" : "main",
    parents: i ? [id(i - 1)] : [],
    health: i % 11 === 5 ? "at_risk" : "healthy",
    opens_direction: null,
    refs: { issue: [], commit: [], report: [] },
    process: null,
    author: null,
    links: { issue: [], commit: [], report: [] },
  }));
  return { project: "rank", project_url: null, main_direction: "main", milestones, directions: [], aggregates: [] };
}

/** The candidates a set of labels left out, each without a reason: a left out turning point in view is blocked by
 * a placed plate of a rank at least its own, by a reserved place, by the stage's height, or by the cap. */
function unblocked(s, scene, labels, cam, view, width, rank, reserved) {
  const placed = new Set(Array.from(labels.id.subarray(0, labels.n)));
  if (labels.n >= s.MAX_LABELS) return [];
  const out = [];
  for (const i of scene.labeled) {
    if (placed.has(i)) continue;
    const nx = s.toScreenX(cam, view, scene.x[i]);
    const sy = s.toScreenY(cam, view, scene.y[i]);
    if (nx < -40 || nx > view.w + 40 || Math.abs(scene.y[i] - cam.cy) > view.h / 2 / cam.z + 20) continue;
    if (sy - 8 < 0 || sy + 8 > view.h) continue;
    const w = width(i);
    const sx = nx + 12 + w > view.w - 4 ? nx - 12 - w : nx + 12;
    const plate = [sx - 3, sy - 8, sx + w + 3, sy + 8];
    // The picker tests whole 8x16 cells: a plate that shares a cell with another is blocked too.
    const grown = [Math.floor(plate[0] / 8) * 8, Math.floor(plate[1] / 16) * 16, (Math.floor(plate[2] / 8) + 1) * 8, (Math.floor(plate[3] / 16) + 1) * 16];
    let blocked = reserved.some((place) => overlap(grown, place));
    for (let k = 0; k < labels.n && !blocked; k++) {
      const p = plateOf(labels, k);
      const cells = [Math.floor(p[0] / 8) * 8, Math.floor(p[1] / 16) * 16, (Math.floor(p[2] / 8) + 1) * 8, (Math.floor(p[3] / 16) + 1) * 16];
      if (overlap(grown, cells) && rank(labels.id[k]) >= rank(i)) blocked = true;
    }
    if (!blocked) out.push(i);
  }
  return out;
}

/**
 * The level of detail (#489, Acceptance 2), on x10's synthetic story at the stage's size: at every zoom under the
 * zoom of groups, from the whole stage in view up, every milestone is in exactly one group, of one lane, within one
 * stretch of `BIN` px; the groups in view hold every milestone in view; each scene edge between two groups is one
 * group edge; the pointer finds each group in view at its middle; a group's mark is never under the pixel. The negative
 * control is the renderer spike's level of detail, which drew a node only with a radius of a pixel or more on screen:
 * at x10's overview it drew none of the milestones in view.
 */
async function lodChecks(m, s) {
  const l = await import("../src/lib/moviola/lod.ts");
  const c = await import("../src/lib/moviola/camera.ts");
  const { syntheticTimeline } = await import("../harness/synthetic.ts");
  const checks = [];
  const check = (name, ok, detail = "") => checks.push([name, ok, detail]);
  const model = m.prepare(syntheticTimeline(10), 0);
  const view = { w: 1280, h: 720 };
  const lay = m.layout(model, view.w, view.h, null);
  const scene = s.buildScene(model, lay);
  const extent = { w: lay.width, h: lay.height };
  const fit = c.fitZoom(extent, view);
  const zooms = [fit, fit * 1.3, fit * 2, 0.2, 0.3, l.GROUP_ZOOM * 0.999];
  /** The milestones in view: a node's centre inside the view. */
  const inViewNodes = (cam) => {
    const out = [];
    for (let k = 0; k < scene.n; k++) {
      if (Math.abs(scene.x[k] - cam.cx) <= view.w / 2 / cam.z && Math.abs(scene.y[k] - cam.cy) <= view.h / 2 / cam.z) out.push(k);
    }
    return out;
  };
  const problems = [];
  let groupsAtFit = 0;
  for (const z of zooms) {
    const cam = c.clampCamera({ cx: lay.width / 2, cy: lay.height / 2, z }, extent, view);
    const cut = l.groupZoom(cam.z);
    const g = l.groupsAt(scene, model, cut);
    if (z === fit) groupsAtFit = g.n;
    const tag = `z ${cam.z.toFixed(3)}`;
    const count = new Uint8Array(scene.n);
    for (let q = 0; q < g.n; q++) {
      const members = Array.from(g.members.subarray(g.start[q], g.start[q + 1]));
      if (members.length === 0) problems.push(`${tag}: group ${q} empty`);
      if (q > 0 && g.x0[q] < g.x0[q - 1]) problems.push(`${tag}: groups not by x0 at ${q}`);
      for (const k of members) {
        count[k] += 1;
        if (g.of[k] !== q) problems.push(`${tag}: node ${k} in group ${q} says ${g.of[k]}`);
        if (scene.lane[k] !== g.lane[q]) problems.push(`${tag}: node ${k} of lane ${scene.lane[k]} in a group of lane ${g.lane[q]}`);
      }
      const bins = new Set(members.map((k) => Math.floor(scene.x[k] / (l.BIN / cut))));
      if (bins.size !== 1) problems.push(`${tag}: group ${q} over ${bins.size} stretches`);
      // Contiguous in its lane: no milestone of the lane between its first and last outside it.
      for (let k = 0; k < scene.n; k++) {
        if (scene.lane[k] === g.lane[q] && scene.x[k] > g.x0[q] && scene.x[k] < g.x1[q] && g.of[k] !== q) problems.push(`${tag}: node ${k} inside group ${q} but not in it`);
      }
      const centre = (g.x0[q] + g.x1[q]) / 2;
      if (l.groupAt(g, centre, g.y[q], 12 / cam.z) < 0) problems.push(`${tag}: the pointer misses group ${q} at its middle`);
    }
    const twice = count.findIndex((v) => v !== 1);
    if (twice >= 0) problems.push(`${tag}: node ${twice} in ${count[twice]} groups`);
    const nodes = inViewNodes(cam);
    const shown = l.inView(g, cam, view);
    const counted = new Set();
    const [lo, hi] = l.groupRange(g, cam.cx - view.w / 2 / cam.z, cam.cx + view.w / 2 / cam.z);
    for (let q = lo; q < hi; q++) {
      if (g.x1[q] < cam.cx - view.w / 2 / cam.z || Math.abs(g.y[q] - cam.cy) > view.h / 2 / cam.z) continue;
      counted.add(q);
    }
    const lost = nodes.filter((k) => !counted.has(g.of[k]));
    if (lost.length) problems.push(`${tag}: ${lost.length} milestones in view in no group in view`);
    if (shown.groups !== counted.size || shown.moments < nodes.length) problems.push(`${tag}: the screen's words say ${shown.groups} groups and ${shown.moments} moments for ${counted.size} groups and ${nodes.length} in view`);
    const pairs = new Set();
    for (let i = 0; i < g.edges.n; i++) pairs.add(`${g.edges.a[i]} ${g.edges.b[i]}`);
    if (pairs.size !== g.edges.n) problems.push(`${tag}: a group edge twice`);
    for (let i = 0; i < scene.edges.n; i++) {
      const a = g.of[scene.edges.a[i]];
      const b = g.of[scene.edges.b[i]];
      if (a !== b && !pairs.has(`${a} ${b}`)) problems.push(`${tag}: the edge ${i} between groups ${a} and ${b} has no group edge`);
    }
  }
  check("every milestone in exactly one group of its lane, within one stretch, contiguous, found by the pointer, at six zooms from the whole stage", problems.length === 0,
    problems.length ? problems.slice(0, 4).join("; ") : `${groupsAtFit} groups for ${scene.n} milestones at the overview, zoom ${fit.toFixed(3)}`);
  check("a group's mark is never under the pixel: a group of one at a radius of 2.5 px, a larger one at least 6 px tall",
    l.SINGLE_RADIUS >= 2.5 && l.groupHalfHeight(2) >= 3, `${l.SINGLE_RADIUS}, ${l.groupHalfHeight(2)}`);
  check("the groups start under the zoom where a glyph's radius is 2.5 px", Math.abs(l.GROUP_ZOOM * 7 - 2.45) < 0.01, `${l.GROUP_ZOOM}`);
  // The negative control: the spike's sub-pixel omission at x10's overview.
  const cam = c.clampCamera({ cx: lay.width / 2, cy: lay.height / 2, z: fit }, extent, view);
  const nodes = inViewNodes(cam);
  const spikeDrawn = nodes.filter(() => 7 * cam.z >= 1);
  const caught = nodes.length > 0 && spikeDrawn.length < nodes.length;
  for (const [name, ok, detail] of checks) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
  console.log(`negative control, the spike's level of detail at x10's overview (${spikeDrawn.length} of ${nodes.length} milestones in view drawn): ${caught ? "caught" : "NOT CAUGHT"}`);
  return checks.every(([, ok]) => ok) && caught;
}

async function renderer(ui, m, s, fixture) {
  const c = await import("../src/lib/moviola/camera.ts");
  const t = await import("../src/lib/moviola/text.ts");
  const checks = [];
  const check = (name, ok, detail = "") => checks.push([name, ok, detail]);

  // The rings, against the prototype.
  const offsets = fixture.rings.offsets.map((_, i) => m.ringOffset(i));
  const halves = fixture.rings.halfThickness.map((_, i) => m.halfThickness(i));
  check("rings above and below the glyph, as yAnello", JSON.stringify(offsets) === JSON.stringify(fixture.rings.offsets), JSON.stringify(offsets));
  check("a lane's half thickness, as semiSpessore", JSON.stringify(halves) === JSON.stringify(fixture.rings.halfThickness), JSON.stringify(halves));

  // The labels (#489), on a crowded lane, with the lanes' names and the days' band reserved: their plates as drawn,
  // the left-hand ones near the stage's right edge included. The controls: a label on every turning point in view,
  // and the renderer spike's picker, both caught overlapping.
  const model = m.prepare(crowded(120), 0);
  const lay = m.layout(model, 1280, 720, null);
  const scene = s.buildScene(model, lay);
  const view = { w: 1280, h: 720 };
  const width = (i) => model.nodes[scene.node[i]].milestone.title.length * 6.4;
  const cam = { cx: 640, cy: 360, z: 1 };
  const reserved = [[0, 0, view.w, 22], [4, lay.laneY[0] - 17, 120, lay.laneY[0] + 15]];
  const labels = s.pickLabels(scene, cam, view, width, s.newLabels(), null, undefined, reserved);
  const found = labelProblems(s, scene, labels, cam, view, reserved);
  check("labels on a crowded lane: turning points only, no two plates over each other or over a name, at most the cap", labels.n > 0 && found.length === 0,
    `${labels.n} labels${found.length ? `; ${found.slice(0, 3).join("; ")}` : ""}`);
  // A turning point near the stage's right edge: its label on its left, inside the stage, as the renderer draws it.
  const edge = { cx: scene.x[1] - (view.w - 80 - view.w / 2), cy: 360, z: 1 };
  const left = s.pickLabels(scene, edge, view, width, s.newLabels());
  const nx = s.toScreenX(edge, view, scene.x[1]);
  check("a label that would leave the stage goes to its node's left, inside the stage", left.n >= 1 && left.id[0] === 1 && left.sx[0] + left.w[0] === nx - 12 && left.sx[0] >= 0,
    `${left.n} labels, the first at ${left.sx[0]?.toFixed(0)} for a node at ${nx.toFixed(0)}`);
  const far = s.pickLabels(scene, { ...cam, z: s.LABEL_ZOOM / 2 }, view, width, s.newLabels());
  check("no label below the zoom of labels", far.n === 0, `${far.n}`);
  const every = s.newLabels();
  for (const i of scene.labeled) {
    if (every.n >= s.MAX_LABELS || scene.x[i] > cam.cx + view.w / 2) break;
    every.id[every.n] = i;
    every.sx[every.n] = s.toScreenX(cam, view, scene.x[i]) + 12;
    every.sy[every.n] = s.toScreenY(cam, view, scene.y[i]);
    every.w[every.n] = width(i);
    every.n += 1;
  }
  const caught = labelProblems(s, scene, every, cam, view).length > 0;
  const spike = spikeLabels(s, scene, cam, view, width);
  const spikeFound = labelProblems(s, scene, spike, cam, view);
  const caughtSpike = spikeFound.some((p) => p.includes("overlap"));
  // The ranks: on two lanes of every kind in turn, a turning point left out is blocked by a plate of a rank at least
  // its own, a reserved place, the stage's height or the cap; and the highest rank is never left out for a lower one.
  const rm = m.prepare(ranked(160), 0);
  const rl = m.layout(rm, 1280, 720, null);
  const rs = s.buildScene(rm, rl);
  const rw = (i) => rm.nodes[rs.node[i]].milestone.title.length * 6.4;
  const urgent = (i) => rm.nodes[rs.node[i]].milestone.health !== "healthy";
  const rank = (i) => s.labelRank(rs, i, urgent(i));
  const rcam = { cx: 900, cy: rl.height / 2, z: 0.7 };
  const rlabels = s.pickLabels(rs, rcam, view, rw, s.newLabels(), null, rank, reserved);
  const free = unblocked(s, rs, rlabels, rcam, view, rw, rank, reserved);
  const kinds = new Set(Array.from(rlabels.id.subarray(0, rlabels.n)).map((i) => rs.kind[i]));
  check("labels by rank: every turning point left out is blocked by a label of its rank or higher, a name, the edge or the cap",
    rlabels.n > 0 && free.length === 0 && labelProblems(s, rs, rlabels, rcam, view, reserved).length === 0,
    `${rlabels.n} labels of ${kinds.size} kinds${free.length ? `; left out with nothing in the way: ${free.slice(0, 5).join(", ")}` : ""}`);
  // The spike's picker on the same lanes leaves out a higher rank for a lower one met first.
  const caughtRank = unblocked(s, rs, spikeLabels(s, rs, rcam, view, rw), rcam, view, rw, rank, []).length > 0;

  // The camera.
  const extent = { w: 3000, h: 600 };
  const box = { w: 1000, h: 500 };
  const home = c.homeCamera(extent, box);
  check("home: zoom 1, the stage's left and top edges on the view's", home.z === 1 && home.cx === 500 && home.cy === 250, JSON.stringify(home));
  const zoomed = c.zoomAround(home, 700, 200, 1.5, extent, box);
  const still = [s.toWorldX(zoomed, box, 700), s.toWorldY(zoomed, box, 200)];
  check("a zoom keeps the point under the pointer", Math.abs(still[0] - 700) < 1e-9 && Math.abs(still[1] - 200) < 1e-9, JSON.stringify(still));
  const out = c.zoomAround(home, 500, 250, 0.01, extent, box);
  check("zooming out stops where the whole stage is in view", Math.abs(out.z - 1 / 3) < 1e-9 && out.cx === 1500, JSON.stringify(out));
  const dragged = c.panBy(home, 5000, -5000, extent, box);
  check("a drag past an edge stops at it", dragged.cx === 500 && dragged.cy === 350, JSON.stringify(dragged));
  const shown = c.reveal(home, 2900, 300, 48, extent, box);
  check("the camera moves to show a node out of view", s.toScreenX(shown, box, 2900) <= box.w - 48 + 1e-9, JSON.stringify(shown));

  // Lanes and days, on alpha.
  const alpha = fixture.cases.find((x) => x.name === "alpha");
  const am = m.prepare(alpha.timeline, 0);
  const al = m.layout(am, 1280, 720, null);
  const shapes = am.lanes.every((lane) => {
    const e = m.laneEnvelope(am, al, lane);
    if (!e) return lane.nodes.length === 0;
    const xs = e.points.map((p) => p.x);
    const sorted = xs.every((x, i) => i === 0 || x >= xs[i - 1]);
    const start = e.x0 <= al.x[lane.nodes[0]];
    const end = lane.closed !== null ? e.x1 === al.xAt(lane.closed) : e.x1 >= al.x[lane.nodes.at(-1)];
    return sorted && start && end && e.points.length === lane.nodes.length + 2;
  });
  check("each lane's envelope runs from its opening to its close, through its nodes", shapes);
  const days = m.dayStarts(am, (ms) => new Date(ms).toISOString().slice(0, 10));
  check("one day label per day with a milestone", days.length === 6 && days[0] === 0, JSON.stringify(days));

  // The portfolio and the catalog.
  const order = t.portfolioOrder([...fixture.portfolio].reverse()).map((p) => p.id);
  check("the projects with a story first", JSON.stringify(order) === JSON.stringify(["beta", "alpha", "gamma"]), JSON.stringify(order));
  check("a registry title loses its [id] prefix", t.projectName({ id: "x", title: "[x] A title" }) === "A title" && t.projectName({ id: "x", title: null }) === "x");
  const catalog = readFileSync(join(ui, "../../crates/regia-i18n/locales/en/moviola.ftl"), "utf8");
  const ids = [...Object.values(t.KIND_ID), ...Object.values(t.HEALTH_ID), ...Object.values(t.LANE_ID)];
  const missing = ids.filter((id) => !new RegExp(`^${id} =`, "m").test(catalog));
  check("a catalog id for every kind, health and lane state of the core", missing.length === 0, JSON.stringify(missing));

  for (const [name, ok, detail] of checks) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
  console.log(`negative control, a label on every turning point in view: ${caught ? "caught" : "NOT CAUGHT"}`);
  console.log(`negative control, the spike's picker (one row, the right-hand place): ${caughtSpike ? "caught" : "NOT CAUGHT"}${spikeFound.length ? `, ${spikeFound[0]}` : ""}`);
  console.log(`negative control, the spike's picker by rank (left to right only): ${caughtRank ? "caught" : "NOT CAUGHT"}`);
  return checks.every(([, ok]) => ok) && caught && caughtSpike && caughtRank;
}
