// What a screen reader gets. SVG exposes its elements natively (role="img" + aria-label on every milestone, see renderers/svg.js).
// Canvas 2D, WebGL2 and WebGPU draw pixels only: they need a visually hidden DOM list for the milestones in view.
// This measures the cost of rebuilding that list when the view settles (not per frame).
import { lowerBound, nodeText, KIND_NAMES } from './dataset.js';
import { VIEW } from './scenario.js';

const MAX_ITEMS = 200;

export function buildShadowList(host, scene, cam) {
  const { x, y, labeled, kind } = scene.nodes;
  const halfW = VIEW.w / 2 / cam.z, halfH = VIEW.h / 2 / cam.z;
  const lo = lowerBound(x, cam.cx - halfW), hi = lowerBound(x, cam.cx + halfW);
  let ul = host.querySelector('ul.sr-only');
  if (!ul) { ul = document.createElement('ul'); ul.className = 'sr-only'; ul.setAttribute('aria-label', 'moviola, milestones in view'); host.appendChild(ul); }
  const frag = document.createDocumentFragment();
  let n = 0;
  for (const i of labeled) {
    if (i < lo) continue; if (i >= hi) break;
    if (Math.abs(y[i] - cam.cy) > halfH) continue;
    const li = document.createElement('li');
    li.textContent = `${KIND_NAMES[kind[i]]}: ${nodeText(scene, i)}`;
    frag.appendChild(li);
    if (++n >= MAX_ITEMS) break;
  }
  ul.replaceChildren(frag);
  return n;
}

export function measureShadowList(host, scene, cam, reps = 50) {
  // the timer resolution is coarse (0.1 ms): time the whole loop and divide
  let items = 0; const t = performance.now();
  for (let r = 0; r < reps; r++) items = buildShadowList(host, scene, cam);
  const ms = (performance.now() - t) / reps;
  host.querySelector('ul.sr-only')?.remove();
  return { items, ms };
}
