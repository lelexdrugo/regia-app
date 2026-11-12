// The moviola's model, layout and scene against the prototype's viewer: on the fixtures of harness/moviola.json
// (the synthetic dossiers of fixtures/moviola/ as the core gives them, and the case of the dossier's
// test_moviola_viewer.py), the same lanes in the same order, the same nodes in each lane with their rings and
// badges, the same x and y at three stage sizes, with and without a window, and the same time scale with its
// joints, as prove/moviola-record.mjs recorded them from the prototype. Then the scene: nodes ascending in x,
// each found by the grid where it is drawn, one edge per parent. The negative controls: the lanes in the order
// of the directions' file, which the prototype's static graph uses, and the columns moved by one pixel, must
// both be caught. No browser: Node runs the TypeScript as it is, its types stripped.
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
    return ok && caughtOrder && caughtMoved;
  },
};
