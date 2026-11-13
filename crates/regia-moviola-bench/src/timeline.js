// The synthetic timelines the bench draws: the renderer spike's generator (commit 84871ea of regia-app's
// spike/moviola-renderer, src/dataset.js: seed 20261102, mulberry32, and src/shape.json, the numeric shape of the
// real stores of two dossiers) in the core's shape, a Timeline of milestones and aggregates, as
// app/ui/harness/bench.ts (#486) first wrote it. A task is a ring on the milestone that names it, not a node of its
// own: x1 is the spike's 155 nodes as 27 milestones and 128 aggregates. Synthetic only: no real title, id or path.
// Pure, no DOM: `npm test` runs it in Node.

export const SEED = 20261102;
const HOUR = 3600e3;

/** src/shape.json of the spike, in the core's words. */
export const SHAPE = {
  spanDays: 18.2,
  milestones: 27,
  aggregates: 128,
  lanes: 8,
  kinds: { decision: 5, abandonment: 1, milestone: 10, exploration: 2, exception: 7, rejoin: 1, gate: 1 },
  parents: [2, 21, 4],
  laneShare: [0.41, 0.33, 0.15, 0.04, 0.04, 0.04],
  titleP50: 55,
  titleMax: 152,
  twoRounds: 13 / 128,
};

const WORDS = (
  'review adversarial contract moviola snapshot decision gate report round defects closed controller worker dispatch ' +
  'step orchestrator desk benchmark renderer window tracker issue project task state schema aggregate milestone direction ' +
  'exploration return abandon proof cost quota model effort subagents survey queue lease lock cursor events precheck ' +
  'playbook paper experiment hypothesis analysis synthesis draft translation facade catalog languages installer signature release'
).split(' ');

function mulberry32(seed) {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** A scale is a number (x1, x10, x100: milestones, aggregates, lanes and days grow together) or a string like '10d':
 * the lanes and days of x10 with 8 times the milestones and aggregates (the spike's crowded view, D8 of #398). */
export function parseScale(scale) {
  const m = /^(\d+(?:\.\d+)?)(d?)$/.exec(String(scale));
  if (!m || Number(m[1]) <= 0) throw new Error(`bad scale ${scale}: a number, or a number and d (10d)`);
  return { base: Number(m[1]), crowd: m[2] ? 8 : 1 };
}

/** The spike's `dimensions`: lanes grow as the scale to the 0.3, days with the base count at the measured density. */
export function dimensions(scale) {
  const { base, crowd } = parseScale(scale);
  const lanes = Math.round(SHAPE.lanes * Math.pow(base, 0.3));
  const perLaneDay = SHAPE.milestones / (SHAPE.lanes * SHAPE.spanDays);
  const days = Math.max(SHAPE.spanDays, (SHAPE.milestones * base) / (lanes * perLaneDay));
  return { lanes, days, milestones: Math.round(SHAPE.milestones * base * crowd), aggregates: Math.round(SHAPE.aggregates * base * crowd) };
}

/** The timeline of one scale: milestones in bursts of working days, lanes as measured, parents mostly in their own
 * lane, and the tasks as aggregates on the milestones that name them. The same scale gives the same timeline. */
export function syntheticTimeline(scale) {
  const { base, crowd } = parseScale(scale);
  const { lanes, days, milestones: count, aggregates } = dimensions(scale);
  const rnd = mulberry32(SEED ^ (base * 7919 + crowd));
  const normal = () => Math.sqrt(-2 * Math.log(1 - rnd())) * Math.cos(2 * Math.PI * rnd());
  const weights = Array.from({ length: lanes }, (_, i) => SHAPE.laneShare[i] ?? 0.04 / (1 + (i - SHAPE.laneShare.length) * 0.25));
  const total = weights.reduce((a, b) => a + b, 0);
  const pickLane = () => {
    let r = rnd() * total;
    for (let i = 0; i < lanes; i++) if ((r -= weights[i]) <= 0) return i;
    return lanes - 1;
  };
  const centres = Array.from({ length: Math.max(3, Math.round(days / 4)) }, () => rnd() * days);
  const pickDay = () => Math.min(days - 0.001, Math.max(0, centres[Math.floor(rnd() * centres.length)] + normal() * 1.2));
  const kinds = Object.entries(SHAPE.kinds);
  const kindTotal = kinds.reduce((a, [, w]) => a + w, 0);
  const pickKind = () => {
    let r = rnd() * kindTotal;
    for (const [kind, w] of kinds) if ((r -= w) <= 0) return kind;
    return 'milestone';
  };
  const start = Date.UTC(2026, 6, 1);
  const raw = Array.from({ length: count }, () => {
    const length = Math.max(12, Math.min(SHAPE.titleMax, Math.exp(Math.log(SHAPE.titleP50) + 0.55 * normal())));
    let title = `#${100 + Math.floor(rnd() * 900)}`;
    while (title.length < length) title += ` ${WORDS[Math.floor(rnd() * WORDS.length)]}`;
    return { t: start + pickDay() * 24 * HOUR, lane: pickLane(), kind: pickKind(), title: title.slice(0, SHAPE.titleMax) };
  });
  raw.sort((a, b) => a.t - b.t);
  const laneId = (i) => (i === 0 ? 'main' : `branch-${i}`);
  const id = (i) => `01SYN${String(i).padStart(21, '0')}`;
  const [p0, p1, p2] = SHAPE.parents;
  const milestones = raw.map((m, i) => {
    const r = rnd() * (p0 + p1 + p2);
    const wanted = r < p0 ? 0 : r < p0 + p1 ? 1 : 2;
    const parents = [];
    for (let p = 0; p < wanted && i > 0; p++) {
      let candidate = Math.max(0, i - 1 - Math.floor(rnd() * 4));
      if (p === 0) {
        for (let b = i - 1; b >= Math.max(0, i - 12); b--) {
          if (raw[b].lane === m.lane) {
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
      health: m.kind === 'exception' ? 'at_risk' : m.kind === 'gate' ? 'waiting_on_owner' : 'healthy',
      opens_direction: null,
      refs: { issue: [], commit: [], report: [] },
      process: null,
      author: null,
      links: { issue: [], commit: [], report: [] },
    };
  });
  const model = { agent: 'claude', model: 'claude-opus-5-5', effort: 'high', sessions: 1, via: null };
  const tasks = Array.from({ length: aggregates }, (_, k) => {
    const task = 1000 + k;
    milestones[Math.floor(rnd() * milestones.length)]?.refs.issue.push(task);
    return { project: 'bench', task, window: [], round: rnd() < SHAPE.twoRounds ? 2 : 1, declared_subagents: 0, models: [model], checks: [], defects: null, tokens: null };
  });
  const directions = Array.from({ length: lanes - 1 }, (_, k) => ({
    id: laneId(k + 1),
    title: `Branch ${k + 1}`,
    opened_by: milestones.find((m) => m.direction === laneId(k + 1))?.id ?? null,
    status: k % 4 === 3 ? 'abandoned' : 'active',
    reason: null,
  }));
  return { project: `bench-x${scale}`, project_url: null, main_direction: 'main', milestones, directions, aggregates: tasks };
}
