// The quick check of the time per frame (#486, Acceptance 1; not the T5 milestone, which #488 measures): the
// renderer spike's harness, commit 84871ea of regia-app's spike/moviola-renderer (src/harness.js:
// `calibrateVsync`, `runPhase`, `stats`; src/scenario.js: `cameraAt`, its four phases; src/dataset.js: the seeded
// generator and the shape of the real stores), driving the product's stage on a synthetic timeline. The scales
// are the product's terms: x1 is the shape the spike measured (27 milestones and 128 aggregates on 8 lanes over
// 18 days), x10 ten times the milestones and aggregates on 16 lanes over 91 days. Tasks are rings here, not
// nodes as in the spike, so its numbers do not compare with these. Test only: #488 brings a bench to the product.
import type { Catalog } from "../src/lib/i18n";
import { HOUR, prepare } from "../src/lib/moviola/model";
import { type Camera, lowerBound } from "../src/lib/moviola/scene";
import { Stage } from "../src/lib/moviola/stage";
import { day, dayKey, LANE_ID, laneName, timecode } from "../src/lib/moviola/text";
import type { Aggregate, Direction, LinkedMilestone, MilestoneKind, Timeline } from "../src/lib/moviola/types";

/** src/shape.json of the spike: the numeric shape of the real stores of two dossiers. */
const SHAPE = {
  spanDays: 18.2,
  milestones: 27,
  aggregates: 128,
  lanes: 8,
  kinds: { decision: 5, abandonment: 1, milestone: 10, exploration: 2, exception: 7, rejoin: 1, gate: 1 } as Record<MilestoneKind, number>,
  parents: [2, 21, 4],
  laneShare: [0.41, 0.33, 0.15, 0.04, 0.04, 0.04],
  titleP50: 55,
  titleMax: 152,
  twoRounds: 13 / 128,
};
const SEED = 20261102;
const WORDS = (
  "review adversarial contract moviola snapshot decision gate report round defects closed controller worker dispatch " +
  "step orchestrator desk benchmark renderer window tracker issue project task state schema aggregate milestone direction " +
  "exploration return abandon proof cost quota model effort subagents survey queue lease lock cursor events precheck " +
  "playbook paper experiment hypothesis analysis synthesis draft translation facade catalog languages installer signature release"
).split(" ");

function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** The spike's `generate`, in the core's shape: milestones in bursts of working days, lanes as measured, parents
 * mostly in their own lane, and the tasks as aggregates on the milestones that name them. */
export function syntheticTimeline(scale: number): Timeline {
  const rnd = mulberry32(SEED ^ (scale * 7919 + 1));
  const normal = () => Math.sqrt(-2 * Math.log(1 - rnd())) * Math.cos(2 * Math.PI * rnd());
  const lanes = Math.round(SHAPE.lanes * Math.pow(scale, 0.3));
  const count = Math.round(SHAPE.milestones * scale);
  const perLaneDay = SHAPE.milestones / (SHAPE.lanes * SHAPE.spanDays);
  const days = Math.max(SHAPE.spanDays, count / (lanes * perLaneDay));
  const weights = Array.from({ length: lanes }, (_, i) => SHAPE.laneShare[i] ?? 0.04 / (1 + (i - SHAPE.laneShare.length) * 0.25));
  const total = weights.reduce((a, b) => a + b, 0);
  const pickLane = () => {
    let r = rnd() * total;
    for (let i = 0; i < lanes; i++) if ((r -= weights[i] as number) <= 0) return i;
    return lanes - 1;
  };
  const centres = Array.from({ length: Math.max(3, Math.round(days / 4)) }, () => rnd() * days);
  const pickDay = () => Math.min(days - 0.001, Math.max(0, (centres[Math.floor(rnd() * centres.length)] as number) + normal() * 1.2));
  const kinds = Object.entries(SHAPE.kinds) as [MilestoneKind, number][];
  const kindTotal = kinds.reduce((a, [, w]) => a + w, 0);
  const pickKind = (): MilestoneKind => {
    let r = rnd() * kindTotal;
    for (const [kind, w] of kinds) if ((r -= w) <= 0) return kind;
    return "milestone";
  };
  const start = Date.UTC(2026, 6, 1);
  const raw = Array.from({ length: count }, () => {
    const length = Math.max(12, Math.min(SHAPE.titleMax, Math.exp(Math.log(SHAPE.titleP50) + 0.55 * normal())));
    let title = `#${100 + Math.floor(rnd() * 900)}`;
    while (title.length < length) title += ` ${WORDS[Math.floor(rnd() * WORDS.length)]}`;
    return { t: start + pickDay() * 24 * HOUR, lane: pickLane(), kind: pickKind(), title: title.slice(0, SHAPE.titleMax) };
  });
  raw.sort((a, b) => a.t - b.t);
  const laneId = (i: number) => (i === 0 ? "main" : `branch-${i}`);
  const id = (i: number) => `01SYN${String(i).padStart(21, "0")}`;
  const milestones: LinkedMilestone[] = raw.map((m, i) => {
    const r = rnd() * (SHAPE.parents[0]! + SHAPE.parents[1]! + SHAPE.parents[2]!);
    const wanted = r < SHAPE.parents[0]! ? 0 : r < SHAPE.parents[0]! + SHAPE.parents[1]! ? 1 : 2;
    const parents: string[] = [];
    for (let p = 0; p < wanted && i > 0; p++) {
      let candidate = Math.max(0, i - 1 - Math.floor(rnd() * 4));
      if (p === 0) {
        for (let b = i - 1; b >= Math.max(0, i - 12); b--) {
          if (raw[b]?.lane === m.lane) {
            candidate = b;
            break;
          }
        }
      }
      if (!parents.includes(id(candidate))) parents.push(id(candidate));
    }
    return {
      id: id(i),
      ts: new Date(Math.round(m.t / 60e3) * 60e3).toISOString(),
      kind: m.kind,
      title: m.title,
      direction: laneId(m.lane),
      parents,
      health: m.kind === "exception" ? "at_risk" : m.kind === "gate" ? "waiting_on_owner" : "healthy",
      opens_direction: null,
      refs: { issue: [], commit: [], report: [] },
      process: null,
      author: null,
      links: { issue: [], commit: [], report: [] },
    };
  });
  const aggregates: Aggregate[] = Array.from({ length: Math.round(SHAPE.aggregates * scale) }, (_, k) => {
    const task = 1000 + k;
    milestones[Math.floor(rnd() * milestones.length)]?.refs.issue.push(task);
    const model = { agent: "claude", model: "claude-opus-5-5", effort: "high", sessions: 1, via: null };
    return { project: "bench", task, window: [], round: rnd() < SHAPE.twoRounds ? 2 : 1, declared_subagents: 0, models: [model], checks: [], defects: null, tokens: null };
  });
  const directions: Direction[] = Array.from({ length: lanes - 1 }, (_, k) => ({
    id: laneId(k + 1),
    title: `Branch ${k + 1}`,
    opened_by: milestones.find((m) => m.direction === laneId(k + 1))?.id ?? null,
    status: k % 4 === 3 ? "abandoned" : "active",
    reason: null,
  }));
  return { project: `bench-x${scale}`, project_url: null, main_direction: "main", milestones, directions, aggregates };
}

type Stats = { median: number; p95: number; p99: number; max: number };
const quantile = (sorted: number[], p: number) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] ?? 0;
function stats(values: number[]): Stats {
  const s = [...values].sort((a, b) => a - b);
  return { median: quantile(s, 0.5), p95: quantile(s, 0.95), p99: quantile(s, 0.99), max: s[s.length - 1] ?? 0 };
}

const raf = () => new Promise<number>((resolve) => requestAnimationFrame(resolve));

/** The display's interval, measured: the median of a second of empty frames. */
async function calibrateVsync(ms = 1000): Promise<number> {
  const dts: number[] = [];
  let prev = await raf();
  const end = performance.now() + ms;
  while (performance.now() < end) {
    const t = await raf();
    dts.push(t - prev);
    prev = t;
  }
  return stats(dts).median;
}

const PHASES = ["pan", "zoom", "playhead", "hover"] as const;
type Phase = (typeof PHASES)[number];
const tri = (u: number) => 1 - Math.abs(2 * (u % 1) - 1);
const clamp = (v: number, a: number, b: number) => Math.min(b, Math.max(a, v));

/** `cameraAt`: pure in the phase's elapsed time, the same for every run. Pan at zoom 0.6 back and forth; zoom
 * from the whole stage to 2.4 and back; the playhead across the story and back, the camera after it; the pointer
 * over the labelled nodes in view. */
function cameraAt(stage: Stage, phase: Phase, ms: number, duration: number, path: { x: number; y: number }[]): Camera {
  const { w, h } = stage.extent;
  const view = stage.stageView;
  const u = clamp(ms / duration, 0, 1);
  switch (phase) {
    case "pan": {
      const z = 0.6;
      const half = view.w / 2 / z;
      const span = Math.max(1, w - 2 * half);
      const p = span > 1 ? tri((3000 * ms) / 1000 / (2 * span)) : 0.5;
      return { cx: w <= 2 * half ? w / 2 : half + p * span, cy: h / 2 + Math.sin(u * Math.PI * 2) * Math.min(60, h / 4), z };
    }
    case "zoom": {
      const fit = Math.min(view.w / w, view.h / h);
      return { cx: w / 2, cy: h / 2, z: fit * Math.pow(2.4 / fit, tri(u)) };
    }
    case "playhead": {
      const px = tri(u) * w;
      const layout = stage.currentLayout;
      if (layout) stage.t = layout.tAt(px);
      return { cx: clamp(px, 300, Math.max(300, w - 300)), cy: h / 2, z: 0.8 };
    }
    case "hover": {
      const cam = { cx: w / 2, cy: h / 2, z: 1.4 };
      if (path.length >= 2) {
        const f = tri(u * 3) * (path.length - 1);
        const k = Math.min(path.length - 2, Math.floor(f));
        const a = path[k] as { x: number; y: number };
        const b = path[k + 1] as { x: number; y: number };
        stage.hoverAt(a.x + (b.x - a.x) * (f - k), a.y + (b.y - a.y) * (f - k), cam);
      }
      return cam;
    }
  }
}

/** The labelled nodes the hover phase's pointer visits, on screen, at most forty. */
function hoverPath(stage: Stage): { x: number; y: number }[] {
  const scene = stage.currentScene;
  const { w, h } = stage.extent;
  const view = stage.stageView;
  const cam = { cx: w / 2, cy: h / 2, z: 1.4 };
  if (!scene) return [];
  const lo = lowerBound(scene.x, cam.cx - view.w / 2 / cam.z);
  const hi = lowerBound(scene.x, cam.cx + view.w / 2 / cam.z);
  const points: { x: number; y: number }[] = [];
  for (const i of scene.labeled) {
    if (i < lo || i >= hi) continue;
    const sy = ((scene.y[i] as number) - cam.cy) * cam.z + view.h / 2;
    if (sy < 30 || sy > view.h - 20) continue;
    points.push({ x: ((scene.x[i] as number) - cam.cx) * cam.z + view.w / 2, y: sy });
    if (points.length >= 40) break;
  }
  if (points.length < 2) points.push({ x: 200, y: 200 }, { x: view.w - 200, y: view.h - 100 });
  return points;
}

function runPhase(stage: Stage, phase: Phase, duration: number, path: { x: number; y: number }[]): Promise<{ dts: number[]; js: number[] }> {
  return new Promise((resolve) => {
    const dts: number[] = [];
    const js: number[] = [];
    let t0: number | null = null;
    let prev: number | null = null;
    let n = 0;
    const step = (ts: number) => {
      if (t0 === null) t0 = ts;
      const ms = ts - t0;
      if (ms >= duration) {
        if (prev !== null && n > 1) dts.push(ts - prev);
        resolve({ dts, js });
        return;
      }
      const a = performance.now();
      stage.drawNow(cameraAt(stage, phase, ms, duration, path));
      const b = performance.now();
      if (prev !== null && ++n > 1) {
        dts.push(ts - prev);
        js.push(b - a);
      }
      prev = ts;
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
}

export type BenchResult = {
  scale: number;
  milestones: number;
  aggregates: number;
  lanes: number;
  view: [number, number];
  dpr: number;
  vsync_ms: number;
  phases: Record<string, { frames: number; dt: Stats; js: Stats; dropped_pct: number }>;
  all: { frames: number; dt: Stats; js: Stats; dropped_pct: number };
};

/** Mounts a stage on `root` at the window's size, up to the spike's 1280x720 and the tape, and measures it. */
export async function bench(root: HTMLElement, scale: number, strings: Catalog, phaseMs = 2000, warmupMs = 1000): Promise<BenchResult> {
  const timeline = syntheticTimeline(scale);
  const model = prepare(timeline);
  const host = document.createElement("div");
  host.className = "stage bench";
  host.style.width = `${Math.min(1280, innerWidth)}px`;
  host.style.height = `${Math.min(720 + 46, innerHeight)}px`;
  const canvas = document.createElement("canvas");
  host.append(canvas);
  root.append(host);
  const stage = new Stage(host, canvas, model, {
    text: {
      day: (t) => day(strings, t),
      timecode: (t) => timecode(strings, t),
      laneName: (lane) => laneName(strings, model, lane),
      laneStatus: (lane) => strings.message(LANE_ID[lane.status]),
    },
    dayKey,
  });
  await stage.ready;
  const vsync = await calibrateVsync();
  const path = hoverPath(stage);
  await runPhase(stage, "pan", warmupMs, path);
  const phases: BenchResult["phases"] = {};
  const every = { dts: [] as number[], js: [] as number[] };
  for (const phase of PHASES) {
    const { dts, js } = await runPhase(stage, phase, phaseMs, path);
    every.dts.push(...dts);
    every.js.push(...js);
    phases[phase] = { frames: dts.length, dt: stats(dts), js: stats(js), dropped_pct: (100 * dts.filter((x) => x > 1.5 * vsync).length) / Math.max(1, dts.length) };
  }
  const all = { frames: every.dts.length, dt: stats(every.dts), js: stats(every.js), dropped_pct: (100 * every.dts.filter((x) => x > 1.5 * vsync).length) / Math.max(1, every.dts.length) };
  stage.home();
  stage.setPlayhead(model.t1);
  await raf();
  return {
    scale,
    milestones: model.nodes.length,
    aggregates: timeline.aggregates.length,
    lanes: model.lanes.length,
    view: [stage.stageView.w, stage.stageView.h],
    dpr: devicePixelRatio,
    vsync_ms: vsync,
    phases,
    all,
  };
}

