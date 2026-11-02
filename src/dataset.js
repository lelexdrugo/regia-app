// Synthetic moviola dataset: seeded, deterministic, shaped after the real stores.
// The numeric shape (nodes, lanes, kinds, parent counts, title lengths) is in shape.json,
// derived by scripts/dataset-stats.mjs from the real `.regia/moviola/` of regia and bytesized.
import shape from './shape.json';

export const SEED = 20261102;
export const DAY = 100;          // world units per day
export const LANE_PITCH = 44;    // world units between lane centres
export const KIND_COLORS = [0x8a8272, 0xe8a33d, 0xf0c66b, 0xe0643c, 0xc9483a, 0x5fa8a0, 0x7d9e5a, 0x6b6560];
export const KIND_CSS = KIND_COLORS.map((c) => '#' + c.toString(16).padStart(6, '0'));
export const KIND_NAMES = ['task', 'milestone', 'decision', 'gate', 'exception', 'exploration', 'return', 'abandon'];
// shape code per kind: 0 circle, 1 diamond, 2 square
const KIND_SHAPE = [0, 0, 1, 2, 2, 1, 0, 2];
const MILESTONE_KINDS = ['milestone', 'decisione', 'gate', 'eccezione', 'esplorazione', 'rientro', 'abbandono'];
const KIND_INDEX = { milestone: 1, decisione: 2, gate: 3, eccezione: 4, esplorazione: 5, rientro: 6, abbandono: 7 };

function mulberry32(a) {
  return () => {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const WORDS = ('revisione avversaria contratto moviola snapshot decisione gate report round difetti chiusi controller worker dispatch ' +
  'passo orchestratore scrivania benchmark renderer finestra tracker issue progetto task stato schema aggregato milestone direzione ' +
  'esplorazione rientro abbandono prova costo quota modello effort sottoagenti ricognizione coda lease lock cursore eventi precheck ' +
  'playbook paper esperimento ipotesi analisi sintesi bozza traduzione facciata catalogo lingue installer firma rilascio sicurezza').split(' ');

const normal = (rnd) => Math.sqrt(-2 * Math.log(1 - rnd())) * Math.cos(2 * Math.PI * rnd());

export function dimensions(scale) {
  const lanes = Math.round(shape.lanes * Math.pow(scale, 0.3));
  const nodes = Math.round(shape.nodes * scale);
  const perLaneDay = shape.nodes / (shape.lanes * shape.spanDays);
  const days = Math.max(Math.round(shape.spanDays), Math.round(nodes / (lanes * perLaneDay)));
  return { lanes, nodes, days };
}

export function generate(scale, seed = SEED) {
  const rnd = mulberry32(seed ^ (scale * 7919));
  const { lanes: L, nodes: N, days } = dimensions(scale);
  const M = Math.round((N * shape.milestones) / shape.nodes);

  // lane weights: the measured shares for the first lanes, then a thin tail
  const w = Array.from({ length: L }, (_, i) => shape.laneShare[i] ?? 0.04 / (1 + (i - shape.laneShare.length) * 0.25));
  const wsum = w.reduce((a, b) => a + b, 0);
  const cum = []; let acc = 0; w.forEach((v, i) => { acc += v / wsum; cum[i] = acc; });
  const pickLane = () => { const r = rnd(); let i = 0; while (i < L - 1 && r > cum[i]) i++; return i; };

  // work comes in bursts: Gaussian clusters of working days
  const bursts = Math.max(3, Math.round(days / 4));
  const centres = Array.from({ length: bursts }, () => rnd() * days);
  const pickDay = () => Math.min(days - 0.001, Math.max(0, centres[(rnd() * bursts) | 0] + normal(rnd) * 1.2));

  const kindWeights = MILESTONE_KINDS.map((k) => shape.milestoneKinds[k] || 0);
  const kwSum = kindWeights.reduce((a, b) => a + b, 0);
  const pickKind = () => { let r = rnd() * kwSum; for (let i = 0; i < kindWeights.length; i++) { r -= kindWeights[i]; if (r <= 0) return KIND_INDEX[MILESTONE_KINDS[i]]; } return 1; };

  const x = new Float32Array(N), y = new Float32Array(N), r = new Float32Array(N);
  const kind = new Uint8Array(N), shapeCode = new Uint8Array(N), lane = new Uint16Array(N), rounds = new Uint8Array(N);
  const titles = new Array(N).fill(null);
  const round2 = (shape.rounds[2] || 0) / shape.aggregates;

  for (let i = 0; i < N; i++) {
    const isMs = i < M;
    const l = pickLane();
    lane[i] = l;
    x[i] = pickDay() * DAY;
    y[i] = l * LANE_PITCH + LANE_PITCH / 2 + (rnd() - 0.5) * 12;
    if (isMs) {
      kind[i] = pickKind();
      r[i] = 7;
      const target = Math.max(12, Math.min(shape.titleLength.max, Math.exp(Math.log(shape.titleLength.p50) + 0.55 * normal(rnd))));
      let t = `#${100 + ((rnd() * 900) | 0)} `;
      while (t.length < target) t += WORDS[(rnd() * WORDS.length) | 0] + ' ';
      titles[i] = t.trim().slice(0, shape.titleLength.max);
    } else {
      kind[i] = 0;
      r[i] = 4;
      rounds[i] = rnd() < round2 ? 2 : 1;
    }
    shapeCode[i] = KIND_SHAPE[kind[i]];
  }

  // order by x so that ranges (playhead, label sweep) are binary-searchable
  const order = Array.from({ length: N }, (_, i) => i).sort((a, b) => x[a] - x[b] || a - b);
  const perm = (arr) => { const c = new arr.constructor(N); for (let i = 0; i < N; i++) c[i] = arr[order[i]]; return c; };
  const sx = perm(x), sy = perm(y), sr = perm(r), skind = perm(kind), sshape = perm(shapeCode), slane = perm(lane), srounds = perm(rounds);
  const stitles = order.map((o) => titles[o]);

  // edges: every milestone points to 0, 1 or 2 earlier milestones (measured distribution), preferring its own lane
  const msIdx = []; for (let i = 0; i < N; i++) if (stitles[i]) msIdx.push(i);
  const pc = shape.parentsPerMilestone; const pcTotal = (pc[0] || 0) + (pc[1] || 0) + (pc[2] || 0);
  const ex1 = [], ey1 = [], ex2 = [], ey2 = [];
  for (let k = 1; k < msIdx.length; k++) {
    const child = msIdx[k];
    const rr = rnd() * pcTotal;
    const np = rr < (pc[0] || 0) ? 0 : rr < (pc[0] || 0) + (pc[1] || 0) ? 1 : 2;
    for (let p = 0; p < np; p++) {
      let cand = msIdx[Math.max(0, k - 1 - ((rnd() * 4) | 0))];
      if (p === 0) { for (let b = k - 1; b >= Math.max(0, k - 12); b--) if (slane[msIdx[b]] === slane[child]) { cand = msIdx[b]; break; } }
      ex1.push(sx[cand]); ey1.push(sy[cand]); ex2.push(sx[child]); ey2.push(sy[child]);
    }
  }

  // lane ribbons: one rectangle per lane and day, thickness = tasks open that day (parallelism)
  const counts = new Map();
  for (let i = 0; i < N; i++) { if (stitles[i]) continue; const key = slane[i] * 100000 + Math.floor(sx[i] / DAY); counts.set(key, (counts.get(key) || 0) + srounds[i]); }
  const bx = [], by = [], bw = [], bh = [], bl = [];
  for (const [key, c] of [...counts.entries()].sort((a, b) => a[0] - b[0])) {
    const l = Math.floor(key / 100000), d = key % 100000;
    bx.push(d * DAY); bw.push(DAY); bl.push(l);
    const h = Math.min(LANE_PITCH - 6, 6 + 4 * c); bh.push(h); by.push(l * LANE_PITCH + LANE_PITCH / 2 - h / 2);
  }

  const f32 = (a) => Float32Array.from(a);
  const scene = {
    scale, seed, N, M, lanes: L, days,
    extent: { x0: 0, y0: 0, x1: days * DAY, y1: L * LANE_PITCH },
    nodes: { x: sx, y: sy, r: sr, kind: skind, shape: sshape, lane: slane, rounds: srounds, titles: stitles, labeled: Uint32Array.from(msIdx) },
    edges: { n: ex1.length, x1: f32(ex1), y1: f32(ey1), x2: f32(ex2), y2: f32(ey2) },
    bands: { n: bx.length, x: f32(bx), y: f32(by), w: f32(bw), h: f32(bh), lane: Uint16Array.from(bl) },
  };
  scene.grid = buildGrid(scene);
  return scene;
}

export function nodeText(scene, i) {
  const t = scene.nodes.titles[i];
  if (t) return t;
  return `#${1000 + i} · task · ${scene.nodes.rounds[i]} round${scene.nodes.rounds[i] > 1 ? 's' : ''}`;
}

const CELL = 32;
function buildGrid(scene) {
  const { x, y } = scene.nodes; const N = scene.N;
  const cw = Math.ceil(scene.extent.x1 / CELL) + 2, ch = Math.ceil(scene.extent.y1 / CELL) + 2;
  const start = new Uint32Array(cw * ch + 1);
  const cell = new Uint32Array(N);
  for (let i = 0; i < N; i++) { const c = Math.floor(y[i] / CELL) * cw + Math.floor(x[i] / CELL); cell[i] = c; start[c + 1]++; }
  for (let c = 0; c < cw * ch; c++) start[c + 1] += start[c];
  const fill = start.slice(0, cw * ch), items = new Uint32Array(N);
  for (let i = 0; i < N; i++) items[fill[cell[i]]++] = i;
  return { cw, ch, start, items };
}

// nearest node within `radius` world units of (wx, wy), or -1
export function hitTest(scene, wx, wy, radius) {
  const g = scene.grid, { x, y } = scene.nodes;
  const c0 = Math.max(0, Math.floor((wx - radius) / CELL)), c1 = Math.min(g.cw - 1, Math.floor((wx + radius) / CELL));
  const r0 = Math.max(0, Math.floor((wy - radius) / CELL)), r1 = Math.min(g.ch - 1, Math.floor((wy + radius) / CELL));
  let best = -1, bd = radius * radius;
  for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) {
    const cell = r * g.cw + c;
    for (let k = g.start[cell]; k < g.start[cell + 1]; k++) {
      const i = g.items[k]; const dx = x[i] - wx, dy = y[i] - wy; const d = dx * dx + dy * dy;
      if (d < bd) { bd = d; best = i; }
    }
  }
  return best;
}

// first index with arr[i] >= v (arr sorted ascending)
export function lowerBound(arr, v) { let lo = 0, hi = arr.length; while (lo < hi) { const m = (lo + hi) >> 1; if (arr[m] < v) lo = m + 1; else hi = m; } return lo; }
