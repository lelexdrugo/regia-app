// The bench of the list of moments (#490, `benchList`): its costs on a stage with its list, on a synthetic timeline
// (harness/synthetic.ts), against the dossier's bound of 1 ms, for prove/bench.mjs --list. Test only. The time per
// frame is not measured here any more: #486's quick check of it, the renderer spike's harness on this stage, gave
// way to the product's bench, crates/regia-moviola-bench (`npm run bench` there, in the product's webview; #683).
import type { Catalog } from "../src/lib/i18n";
import { fitZoom } from "../src/lib/moviola/camera";
import { prepare } from "../src/lib/moviola/model";
import { Stage } from "../src/lib/moviola/stage";
import { listText } from "../src/lib/moviola/listbox";
import { day, dayKey, LANE_ID, laneName, timecode } from "../src/lib/moviola/text";
import { syntheticTimeline } from "./synthetic";

type Stats = { median: number; p95: number; p99: number; max: number };
const quantile = (sorted: number[], p: number) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] ?? 0;
function stats(values: number[]): Stats {
  const s = [...values].sort((a, b) => a - b);
  return { median: quantile(s, 0.5), p95: quantile(s, 0.95), p99: quantile(s, 0.99), max: s[s.length - 1] ?? 0 };
}

const raf = () => new Promise<number>((resolve) => requestAnimationFrame(resolve));

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

