// The synthetic timelines of the moviola's quick check and of its proofs (#486, #489): the renderer spike's seeded
// generator (commit 84871ea of regia-app's spike/moviola-renderer, src/dataset.js, and src/shape.json, the numeric
// shape of the real stores of two dossiers), in the core's shape. x1 is the spike's 27 milestones and 128 aggregates
// on 8 lanes over 18 days, x10 ten times the milestones and aggregates on 16 lanes over 91 days. Synthetic only: no
// real title, id or path. Pure, and types are all it imports, so the proofs' Node runs it as it is (prove/moviola.js).
import type { Aggregate, Direction, LinkedMilestone, MilestoneKind, Timeline } from "../src/lib/moviola/types";

const HOUR = 3600e3;

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
