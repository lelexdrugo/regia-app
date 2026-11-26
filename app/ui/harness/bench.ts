// The quick check of the time per frame (#486, Acceptance 1; not the T5 milestone, which #488 measures): the
// renderer spike's harness, commit 84871ea of regia-app's spike/moviola-renderer (src/harness.js:
// `calibrateVsync`, `runPhase`, `stats`; src/scenario.js: `cameraAt`, its four phases; src/dataset.js: the seeded
// generator and the shape of the real stores), driving the product's stage on a synthetic timeline. The scales
// are the product's terms: x1 is the shape the spike measured (27 milestones and 128 aggregates on 8 lanes over
// 18 days), x10 ten times the milestones and aggregates on 16 lanes over 91 days. Tasks are rings here, not
// nodes as in the spike, so its numbers do not compare with these. Test only: #488 brings a bench to the product.
import type { Catalog } from "../src/lib/i18n";
import { fitZoom } from "../src/lib/moviola/camera";
import { prepare } from "../src/lib/moviola/model";
import { type Camera, lowerBound } from "../src/lib/moviola/scene";
import { Stage } from "../src/lib/moviola/stage";
import { day, dayKey, LANE_ID, laneName, listText, timecode } from "../src/lib/moviola/text";
import { syntheticTimeline } from "./synthetic";


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

/** A cost in milliseconds over its samples, with the mean of all of them: the clock of a page that is not isolated
 * ticks by 0.1 ms in Chromium, so a cost under that shows in the mean, not in a sample. */
export type Cost = { median: number; p95: number; max: number; mean: number; samples: number };

function cost(values: number[], total = values.reduce((a, b) => a + b, 0), count = values.length): Cost {
  return { ...stats(values), mean: total / Math.max(1, count), samples: count };
}

/** The list of moments at one level of detail (#490). */
export type ListLevel = {
  level: "detail" | "groups";
  zoom: number;
  /** The entries of the list, its groups of several moments, and the options in the page. */
  entries: number;
  groups: number;
  options: number;
  /** A frame at a still view, a moment chosen: the list's sync finds nothing changed. In batches of 50 calls. */
  still: Cost;
  /** The same with no moment chosen: the entry at the playhead is known already. */
  stillNoChoice: Cost;
  /** An arrow: the next moment chosen, the window of the list rebuilt around it, its options styled. */
  step: Cost;
  /** The order rebuilt, as a new filter asks, and the window with it. */
  order: Cost;
  /** The playhead moved with no moment chosen: the entry at the playhead found again, the window moved to it. */
  playhead: Cost;
  /** Windows rebuilt during the measure. */
  builds: number;
};

export type ListBench = {
  scale: number;
  milestones: number;
  lanes: number;
  view: [number, number];
  dpr: number;
  /** The conformance's bound (the dossier's docs/app/moviola/criteri.md, V-accessibilità): a rebuild of the list at
   * a still view under 1 ms. */
  bound_ms: number;
  levels: ListLevel[];
};

/**
 * The cost of the list of moments (#490, the dossier's V-accessibilità): a stage with its list on a synthetic
 * timeline, at the zoom of detail and, where the story is wide enough, at the zoom of groups (the whole stage). Each
 * cost is the stage's own path: `syncList` as a frame calls it, `step` as an arrow does, the order as a filter asks
 * for it, the playhead as the range moves it. The browser's work after the page's (the accessibility tree sent to the
 * system) is not in it.
 */
export async function benchList(root: HTMLElement, scale: number, strings: Catalog, repeat = 120): Promise<ListBench> {
  const timeline = syntheticTimeline(scale);
  const model = prepare(timeline);
  const host = document.createElement("div");
  host.className = "stage bench";
  host.style.width = `${Math.min(1280, innerWidth)}px`;
  host.style.height = `${Math.min(720 + 46, innerHeight)}px`;
  const canvas = document.createElement("canvas");
  canvas.tabIndex = 0;
  canvas.setAttribute("role", "listbox");
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
    list: listText(strings, model),
  });
  await stage.ready;
  const levels: ListLevel[] = [];
  for (const level of ["detail", "groups"] as const) {
    if (level === "detail") stage.home();
    else stage.drawNow({ cx: stage.extent.w / 2, cy: stage.extent.h / 2, z: fitZoom(stage.extent, stage.stageView) });
    await raf();
    await raf();
    const order = stage.listOrder;
    if (level === "groups" && !order.groups) break;
    const entries = order.index.length;
    const builds = stage.listBuilds;
    const at = (e: number) => order.index[Math.min(entries - 1, Math.max(0, e))] as number;
    const batches = (): number[] => {
      const out: number[] = [];
      for (let b = 0; b < 20; b++) {
        const a = performance.now();
        for (let i = 0; i < 50; i++) stage.syncList();
        out.push((performance.now() - a) / 50);
      }
      return out;
    };
    stage.select(at(entries >> 1));
    stage.syncList();
    const still = cost(batches());
    stage.select(-1);
    stage.syncList();
    const stillNoChoice = cost(batches());

    // Arrows from the start of the list: each one a new window, styled as the page would style it.
    stage.select(at(0));
    const steps: number[] = [];
    for (let i = 0; i < Math.min(repeat, entries - 1); i++) {
      const a = performance.now();
      stage.step(1);
      const id = canvas.getAttribute("aria-activedescendant");
      if (id) void getComputedStyle(document.getElementById(id) as HTMLElement).display;
      steps.push(performance.now() - a);
    }

    // The order: a new mask of the filters, the same moments, as a filter changed and changed back.
    const orders: number[] = [];
    for (let i = 0; i < Math.min(repeat, 60); i++) {
      stage.setEmphasis(model.nodes.map(() => true), -1);
      const a = performance.now();
      void stage.listOrder;
      stage.syncList();
      orders.push(performance.now() - a);
    }
    stage.setEmphasis(null, -1);

    // The playhead across the story with no moment chosen.
    stage.select(-1);
    const playheads: number[] = [];
    for (let i = 0; i < Math.min(repeat, 60); i++) {
      const t = model.t0 + ((model.t1 - model.t0) * ((i * 37) % 60)) / 60;
      const a = performance.now();
      stage.setPlayhead(t);
      playheads.push(performance.now() - a);
    }
    await raf();
    levels.push({
      level,
      zoom: stage.camera.z,
      entries,
      groups: order.several,
      options: canvas.querySelectorAll("[role=option]").length,
      still,
      stillNoChoice,
      step: cost(steps),
      order: cost(orders),
      playhead: cost(playheads),
      builds: stage.listBuilds - builds,
    });
  }
  stage.destroy();
  host.remove();
  return {
    scale,
    milestones: model.nodes.length,
    lanes: model.lanes.length,
    view: [stage.stageView.w, stage.stageView.h],
    dpr: devicePixelRatio,
    bound_ms: 1,
    levels,
  };
}

