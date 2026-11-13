// The camera over the stage: the spike's `{cx, cy, z}` (scene.ts), kept so the stage never leaves the view. It
// replaces the prototype's time window (`impostaFinestra`, `zoomAttorno`): zooming is the camera's, and the layout
// of the stage is the prototype's at the view's size. Pure, so the proofs run it in Node.
import { type Camera, toWorldX, toWorldY, type View } from "./scene";

export const Z_MAX = 4;

export type Extent = { w: number; h: number };

/** The zoom that shows the whole stage, never above 1: the prototype's layout already fits a small one. */
export function fitZoom(extent: Extent, view: View): number {
  return Math.min(1, view.w / Math.max(extent.w, 1), view.h / Math.max(extent.h, 1));
}

/** The zoom within its bounds, and the stage in view: centred along an axis where it is smaller than the view. */
export function clampCamera(cam: Camera, extent: Extent, view: View): Camera {
  const z = Math.min(Z_MAX, Math.max(fitZoom(extent, view), cam.z));
  const halfW = view.w / 2 / z;
  const halfH = view.h / 2 / z;
  const cx = extent.w <= 2 * halfW ? extent.w / 2 : Math.min(extent.w - halfW, Math.max(halfW, cam.cx));
  const cy = extent.h <= 2 * halfH ? extent.h / 2 : Math.min(extent.h - halfH, Math.max(halfH, cam.cy));
  return { cx, cy, z };
}

/** At rest: zoom 1, the stage's left edge on the view's, its top on the view's top. */
export function homeCamera(extent: Extent, view: View): Camera {
  return clampCamera({ cx: view.w / 2, cy: view.h / 2, z: 1 }, extent, view);
}

/** Zoom by `factor` keeping the world point under the screen point (sx, sy) where it is. */
export function zoomAround(cam: Camera, sx: number, sy: number, factor: number, extent: Extent, view: View): Camera {
  const wx = toWorldX(cam, view, sx);
  const wy = toWorldY(cam, view, sy);
  const z = Math.min(Z_MAX, Math.max(fitZoom(extent, view), cam.z * factor));
  return clampCamera({ cx: wx - (sx - view.w / 2) / z, cy: wy - (sy - view.h / 2) / z, z }, extent, view);
}

/** Move the stage by (dx, dy) screen pixels, as a hand drags it. */
export function panBy(cam: Camera, dx: number, dy: number, extent: Extent, view: View): Camera {
  return clampCamera({ cx: cam.cx - dx / cam.z, cy: cam.cy - dy / cam.z, z: cam.z }, extent, view);
}

/** The camera moved as little as needed for the world point (wx, wy) to sit `margin` pixels inside the view. */
export function reveal(cam: Camera, wx: number, wy: number, margin: number, extent: Extent, view: View): Camera {
  const halfW = view.w / 2 / cam.z - margin / cam.z;
  const halfH = view.h / 2 / cam.z - margin / cam.z;
  const cx = wx < cam.cx - halfW ? wx + halfW : wx > cam.cx + halfW ? wx - halfW : cam.cx;
  const cy = wy < cam.cy - halfH ? wy + halfH : wy > cam.cy + halfH ? wy - halfH : cam.cy;
  return clampCamera({ cx, cy, z: cam.z }, extent, view);
}
