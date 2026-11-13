// The scripted scenario of the dossier's docs/app/moviola/metodo.md, identical for every run: the renderer spike's
// src/scenario.js (commit 84871ea, `cameraAt` and its four phases, `hoverPath`), on the product's stage and its
// world (app/ui/src/lib/moviola: one column per milestone, lanes as rows), as app/ui/harness/bench.ts (#486)
// first moved it there. Pure functions of the stage and the phase's elapsed time.

export const PHASES = ['pan', 'zoom', 'playhead', 'hover'];

const Z_PAN = 0.6;
const PAN_SPEED = 3000; // world units per second
const Z_HOVER = 1.4;
const Z_MAX = 2.4;
const tri = (u) => 1 - Math.abs(2 * (u % 1) - 1); // 0..1..0
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

/** The camera at `ms` into a phase of `duration` ms. The playhead phase moves the stage's playhead, the hover phase
 * its pointer, as a hand would: the frame then draws the veil, the timecode and the ring of the node under it. */
export function cameraAt(stage, phase, ms, duration, path) {
  const { w, h } = stage.extent;
  const view = stage.stageView;
  const u = clamp(ms / duration, 0, 1);
  switch (phase) {
    case 'pan': {
      // constant speed back and forth along x at a working zoom, with a slow vertical drift
      const half = view.w / 2 / Z_PAN;
      const span = Math.max(1, w - 2 * half);
      const p = span > 1 ? tri((PAN_SPEED * ms) / 1000 / (2 * span)) : 0.5;
      return { cx: w <= 2 * half ? w / 2 : half + p * span, cy: h / 2 + Math.sin(u * Math.PI * 2) * Math.min(60, h / 4), z: Z_PAN };
    }
    case 'zoom': {
      // from the whole stage in view to the label zoom and back, log-spaced, on the centre of the stage
      const fit = Math.min(view.w / w, view.h / h);
      return { cx: w / 2, cy: h / 2, z: fit * Math.pow(Z_MAX / fit, tri(u)) };
    }
    case 'playhead': {
      // the camera follows the playhead as it scrubs the whole story and back
      const px = tri(u) * w;
      const layout = stage.currentLayout;
      if (layout) stage.t = layout.tAt(px);
      return { cx: clamp(px, 300, Math.max(300, w - 300)), cy: h / 2, z: 0.8 };
    }
    case 'hover': {
      // a fixed camera in the middle; the pointer goes out and back along the labelled nodes in view, three times
      const cam = { cx: w / 2, cy: h / 2, z: Z_HOVER };
      if (path.length >= 2) {
        const f = tri(u * 3) * (path.length - 1);
        const k = Math.min(path.length - 2, Math.floor(f));
        const a = path[k];
        const b = path[k + 1];
        stage.hoverAt(a.x + (b.x - a.x) * (f - k), a.y + (b.y - a.y) * (f - k), cam);
      }
      return cam;
    }
    default:
      throw new Error(`unknown phase ${phase}`);
  }
}

/** The labelled nodes the hover phase's pointer visits, in screen pixels, at most forty. */
export function hoverPath(stage, lowerBound) {
  const scene = stage.currentScene;
  const { w, h } = stage.extent;
  const view = stage.stageView;
  const cam = { cx: w / 2, cy: h / 2, z: Z_HOVER };
  const points = [];
  if (scene) {
    const lo = lowerBound(scene.x, cam.cx - view.w / 2 / cam.z);
    const hi = lowerBound(scene.x, cam.cx + view.w / 2 / cam.z);
    for (const i of scene.labeled) {
      if (i < lo || i >= hi) continue;
      const sy = (scene.y[i] - cam.cy) * cam.z + view.h / 2;
      if (sy < 30 || sy > view.h - 20) continue;
      points.push({ x: (scene.x[i] - cam.cx) * cam.z + view.w / 2, y: sy });
      if (points.length >= 40) break;
    }
  }
  if (points.length < 2) points.push({ x: 200, y: 200 }, { x: view.w - 200, y: view.h - 100 });
  return points;
}
