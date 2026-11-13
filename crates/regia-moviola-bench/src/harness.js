// The bench: the renderer spike's src/harness.js (commit 84871ea of regia-app's spike/moviola-renderer:
// `calibrateVsync`, `runPhase` with its watchdog, the repetitions with a rotating order, `renderOnce`, `probeGpu`)
// driving the product's renderer, app/ui's Stage (src/lib/moviola/stage.ts: the Canvas 2D of #486), imported as it
// is. Each configuration gets a new Stage on a new canvas; every phase starts from the stage at rest, the playhead
// on the last milestone and no node under the pointer.
import { TAPE } from '../../../app/ui/src/lib/moviola/canvas2d';
import { prepare } from '../../../app/ui/src/lib/moviola/model';
import { lowerBound } from '../../../app/ui/src/lib/moviola/scene';
import { Stage } from '../../../app/ui/src/lib/moviola/stage';
import { day, dayKey, LANE_ID, laneName, timecode } from '../../../app/ui/src/lib/moviola/text';
import { cameraAt, hoverPath, PHASES } from './scenario.js';
import { dimensions, syntheticTimeline } from './timeline.js';

/** The stage above the tape, in CSS pixels: the spike's 1280x720. */
export const VIEW = { w: 1280, h: 720 };

/** The product's renderer and the two deliberately slowed variants of the sanity check (criteri.md, E3):
 * `busyv` spins 1.6 refresh intervals of script time per frame; `shadow` sets a blurred shadow on the stage's 2D
 * context before each frame, which the renderer saves and restores but never resets, so every fill and stroke of
 * the frame casts one (the spike's put it on the nodes only). */
export const VARIANTS = {
  canvas2d: {},
  'canvas2d+busyv': { busy: 1.6 },
  'canvas2d+shadow': { shadow: true },
};

/** `main` is the measurement of T5 (docs/app/piano/traguardi.md), `sanity` its control, at x10. */
export const BLOCKS = {
  main: { renderers: ['canvas2d'], scales: null },
  sanity: { renderers: ['canvas2d', 'canvas2d+busyv', 'canvas2d+shadow'], scales: [10] },
};

const raf = () => new Promise((resolve) => requestAnimationFrame(resolve));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const quantile = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];

export function stats(values) {
  const s = [...values].sort((a, b) => a - b);
  if (!s.length) return { median: NaN, p95: NaN, p99: NaN, max: NaN };
  return { median: quantile(s, 0.5), p95: quantile(s, 0.95), p99: quantile(s, 0.99), max: s[s.length - 1] };
}

function busyWait(ms) {
  const end = performance.now() + ms;
  while (performance.now() < end) {
    // spin
  }
}

/** The display's refresh interval, measured: the median of a second of empty frames. */
async function calibrateVsync(ms = 1000) {
  const dts = [];
  let prev = await raf();
  const end = performance.now() + ms;
  while (performance.now() < end) {
    const t = await raf();
    dts.push(t - prev);
    prev = t;
  }
  return stats(dts).median;
}

/** A Stage on a new canvas, in a host the size of VIEW and the tape, with the catalog's words. */
function mount(root, timeline, strings) {
  const model = prepare(timeline);
  const host = document.createElement('div');
  host.className = 'stage';
  host.style.width = `${VIEW.w}px`;
  host.style.height = `${VIEW.h + TAPE}px`;
  const canvas = document.createElement('canvas');
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
  return { model, canvas, stage, ctx: canvas.getContext('2d', { alpha: false }) };
}

/** The stage at rest: the playhead on the last milestone, nothing under the pointer. */
function rest(stage) {
  stage.t = stage.model.t1;
  stage.hoverAt(-1, -1, stage.camera);
}

function runPhase(m, variant, phase, duration, path, repeat, vsync) {
  const { stage, ctx } = m;
  rest(stage);
  return new Promise((resolve, reject) => {
    const dts = [];
    const js = [];
    let t0 = null;
    let prev = null;
    let n = 0;
    let last = performance.now();
    const watchdog = setInterval(() => {
      if (performance.now() - last > 15000) {
        clearInterval(watchdog);
        reject(new Error(`requestAnimationFrame stalled for 15 s after ${dts.length} frames: the window is hidden or covered, or a frame took that long`));
      }
    }, 1000);
    const step = (ts) => {
      last = performance.now();
      if (t0 === null) t0 = ts;
      const ms = ts - t0;
      if (ms >= duration) {
        clearInterval(watchdog);
        if (prev !== null && n > 1) dts.push(ts - prev); // the last frame counts too
        resolve({ dts, js });
        return;
      }
      const a = performance.now();
      const cam = cameraAt(stage, phase, ms, duration, path);
      for (let k = 0; k < repeat; k++) {
        if (variant.shadow) {
          ctx.shadowBlur = 6 * devicePixelRatio;
          ctx.shadowColor = 'rgba(232,163,61,0.8)';
        }
        stage.drawNow(cam);
      }
      if (variant.busy) busyWait(variant.busy * vsync);
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

/** The spike's `renderOnce`: one camera of a phase, drawn twice across a frame, as a PNG. */
async function shot(m, phase, u, duration, path) {
  rest(m.stage);
  const cam = cameraAt(m.stage, phase, u * duration, duration, path);
  m.stage.drawNow(cam);
  await raf();
  m.stage.drawNow(cam);
  return new Promise((resolve) => m.canvas.toBlob(resolve, 'image/png'));
}

async function runBlock(name, cfg, strings, root, say, shots) {
  const { renderers } = BLOCKS[name];
  const scales = BLOCKS[name].scales ?? cfg.scales;
  const vsync = await calibrateVsync();
  const rows = [];
  const failures = [];
  const infos = [];
  const timelines = new Map();
  const timelineOf = (scale) => {
    if (!timelines.has(scale)) timelines.set(scale, syntheticTimeline(scale));
    return timelines.get(scale);
  };
  for (let rep = 1; rep <= cfg.reps; rep++) {
    for (const scale of scales) {
      // the order of the renderers rotates with the repetition, so an effect of order (GC, heat) does not always hit the same one
      const order = renderers.map((_, i) => renderers[(i + rep - 1) % renderers.length]);
      for (const id of order) {
        const tag = `${name}: ${id} x${scale} rep ${rep}/${cfg.reps}`;
        say(tag);
        root.replaceChildren();
        let m = null;
        try {
          const variant = VARIANTS[id];
          const t0 = performance.now();
          m = mount(root, timelineOf(scale), strings);
          await m.stage.ready;
          const initMs = performance.now() - t0;
          await raf();
          await raf();
          const path = hoverPath(m.stage, lowerBound);
          await runPhase(m, variant, 'pan', cfg.warmup_ms, path, 1, vsync); // warm-up, not recorded
          const pending = [];
          for (const phase of PHASES) {
            const { dts, js } = await runPhase(m, variant, phase, cfg.phase_ms, path, cfg.repeat, vsync);
            const d = stats(dts);
            const j = stats(js);
            pending.push({
              block: name, renderer: id, scale, rep, phase, repeat: cfg.repeat, frames: dts.length,
              dt_median: d.median, dt_p95: d.p95, dt_p99: d.p99, dt_max: d.max, js_median: j.median, js_p95: j.p95,
              dropped_pct: dts.length ? (100 * dts.filter((x) => x > 1.5 * vsync).length) / dts.length : NaN,
              init_ms: initMs,
              samples: { dt: dts.map((v) => Math.round(v * 10) / 10), js: js.map((v) => Math.round(v * 100) / 100) },
            });
          }
          if (rep === 1) {
            const dims = dimensions(scale);
            infos.push({
              block: name, renderer: id, scale, milestones: m.model.nodes.length,
              aggregates: m.model.timeline.aggregates.length, rings: m.model.nodes.reduce((a, node) => a + node.rings.length, 0),
              lanes: m.model.lanes.length, days: Math.round(dims.days), extent: [Math.round(m.stage.extent.w), Math.round(m.stage.extent.h)],
              view: [m.stage.stageView.w, m.stage.stageView.h], dpr: devicePixelRatio, init_ms: initMs,
            });
            if (name === 'main' && cfg.shots && (scale === 1 || scale === 100)) {
              shots.push({ name: `x${scale}-detail`, blob: await shot(m, 'hover', 0.4, cfg.phase_ms, path) });
              if (scale === 100) shots.push({ name: `x${scale}-overview`, blob: await shot(m, 'zoom', 0, cfg.phase_ms, path) });
            }
          }
          rows.push(...pending); // a configuration that failed half way leaves no rows: it is listed under failures
        } catch (e) {
          failures.push({ block: name, renderer: id, scale, rep, error: String(e?.message || e) });
          say(`${tag} FAILED: ${e?.message || e}`);
        } finally {
          try {
            m?.stage.destroy();
          } catch {
            // nothing else to do
          }
          root.replaceChildren();
          await sleep(150);
        }
      }
    }
  }
  return { name, vsync_ms: vsync, rows, failures, infos };
}

/** What renders, as the page sees it. WebKit answers a fixed "Apple GPU": the runner reads the system's GPU too. */
function probeGpu() {
  const c = document.createElement('canvas');
  const gl = c.getContext('webgl2') || c.getContext('webgl');
  if (!gl) return { webgl: false };
  const debug = gl.getExtension('WEBGL_debug_renderer_info');
  const out = {
    webgl: true,
    renderer: gl.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : gl.RENDERER),
    vendor: gl.getParameter(debug ? debug.UNMASKED_VENDOR_WEBGL : gl.VENDOR),
    version: gl.getParameter(gl.VERSION),
  };
  gl.getExtension('WEBGL_lose_context')?.loseContext();
  return out;
}

export function pageInfo(query) {
  return {
    webview: query.get('webview') || null,
    tauri: query.get('tauri') || null,
    userAgent: navigator.userAgent,
    dpr: devicePixelRatio,
    screen: [screen.width, screen.height],
    inner: [innerWidth, innerHeight],
    cores: navigator.hardwareConcurrency,
    gpu: probeGpu(),
  };
}

/** cfg: { blocks, scales, reps, phase_ms, warmup_ms, repeat, shots } */
export async function runBench(cfg, strings, root, say) {
  const shots = [];
  const blocks = [];
  for (const name of cfg.blocks) blocks.push(await runBlock(name, cfg, strings, root, say, shots));
  return { view: { w: VIEW.w, h: VIEW.h, tape: TAPE }, blocks, shots };
}
