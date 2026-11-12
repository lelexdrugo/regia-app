// Records the prototype's answers for the moviola proof (prove/moviola.js) into harness/moviola.json: the lanes,
// nodes, layouts and time scale that the prototype's viewer computes on the fixtures, beside the same fixtures in
// the product's shape. It is the prototype's side, as crates/regia-conformance/oracle/moviola_oracle.py is for the
// core, so it speaks the prototype's keys; the product's sources never do (the guard of regia-conformance).
//
//   REGIA_HOME=<a copy of the dossier> node prove/moviola-record.mjs
//
// The inputs: fixtures/moviola/oracle.json, the views the prototype's server gives on the synthetic dossiers of
// fixtures/moviola/, translated to the core's timeline by the table of crates/regia-conformance/tests/moviola.rs,
// which checks that the core gives exactly that; and the case of the dossier's tools/tests/test_moviola_viewer.py.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const UI = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ROOT = resolve(UI, "../..");
const home = process.env.REGIA_HOME;
if (!home) {
  console.error("REGIA_HOME: the dossier whose tools/moviola/static/moviola.js is the oracle");
  process.exit(2);
}

// The prototype's viewer in a context with a DOM only as large as loading needs (test_moviola_viewer.py, CARICA).
const context = {
  console, Intl, URLSearchParams, location: { hash: "" }, window: {},
  document: { addEventListener() {}, getElementById() { return null; } },
  ResizeObserver: class { observe() {} disconnect() {} },
};
vm.createContext(context);
vm.runInContext(readFileSync(join(home, "tools/moviola/static/moviola.js"), "utf8"), context, { filename: "moviola.js" });
const run = (code) => vm.runInContext(code, context);

// crates/regia-conformance/tests/moviola.rs: KEYS, MORE_KEYS, DROPPED and word().
const KEYS = Object.fromEntries([
  ["progetto", "project"], ["project_url", "project_url"], ["milestones", "milestones"], ["direzioni", "directions"],
  ["aggregati", "aggregates"], ["id", "id"], ["ts", "ts"], ["tipo", "kind"], ["titolo", "title"], ["direzione", "direction"],
  ["genitori", "parents"], ["salute", "health"], ["apre_direzione", "opens_direction"], ["rif", "refs"], ["processo", "process"],
  ["autore", "author"], ["link", "links"], ["issue", "issue"], ["commit", "commit"], ["report", "report"], ["round", "round"],
  ["controlli", "checks"], ["difetti", "defects"], ["trovati", "found"], ["chiusi", "closed"], ["chiude", "closes"],
  ["ruolo", "role"], ["agente", "agent"], ["modello", "model"], ["sessione", "session"], ["testo", "text"], ["url", "url"],
  ["hash", "hash"], ["percorso", "path"], ["esiste", "exists"], ["aperta_da", "opened_by"], ["stato", "status"],
  ["motivo", "reason"], ["task", "task"], ["finestra", "window"], ["sottoagenti_dichiarati", "declared_subagents"],
  ["modelli", "models"], ["effort", "effort"], ["sessioni", "sessions"], ["via", "via"], ["esito", "outcome"],
  ["token", "tokens"], ["in", "input"], ["out", "output"], ["cache_lettura", "cache_read"], ["cache_scrittura", "cache_write"],
  ["milestone", "milestones"], ["ultima", "last"], ["dal", "from"], ["al", "to"],
]);
const DROPPED = new Set(["milestones/[]/schema", "milestones/[]/progetto", "milestones/[]/snapshot", "milestones/[]/link/pagina",
  "direzioni/[]/schema", "aggregati/[]/schema"]);
const KINDS = { milestone: "milestone", decisione: "decision", gate: "gate", eccezione: "exception", esplorazione: "exploration",
  rientro: "rejoin", abbandono: "abandonment" };
const HEALTH = { "in salute": "healthy", "a rischio": "at_risk", "in attesa del titolare": "waiting_on_owner" };
const STATUS = { attiva: "active", abbandonata: "abandoned", rientrata: "rejoined" };
const REFERENCES = { relativo: "relative", dati: "data", assoluto: "absolute", fuori: "outside" };

function word(path, text) {
  const table = path.endsWith("link/report/[]/tipo") ? REFERENCES
    : path === "milestones/[]/tipo" || path === "ultima/tipo" ? KINDS
      : path === "milestones/[]/salute" || path === "salute" ? HEALTH
        : path === "direzioni/[]/stato" ? STATUS : null;
  if (!table) return text;
  if (!(text in table)) throw new Error(`${path}: ${text} is not in the table`);
  return table[text];
}

function translated(value, path = "") {
  const child = (key) => (path ? `${path}/${key}` : key);
  if (Array.isArray(value)) return value.map((item) => translated(item, child("[]")));
  if (value && typeof value === "object") {
    const out = {};
    for (const [key, item] of Object.entries(value)) {
      const at = child(key);
      if (DROPPED.has(at)) continue;
      if (!(key in KEYS)) throw new Error(`${at}: a key neither translated nor dropped`);
      out[KEYS[key]] = translated(item, at);
    }
    return out;
  }
  return typeof value === "string" ? word(path, value) : value;
}

// What the core writes where the prototype writes nothing: every key, `null` or `[]` for none (records.rs).
const filled = (defaults, value) => ({ ...defaults, ...(value ?? {}) });
function product(view) {
  const t = translated(Object.fromEntries(Object.entries(view).filter(([key]) => key !== "snapshot")));
  const links = (l) => filled({ issue: [], commit: [], report: [] }, l);
  return {
    project: t.project,
    project_url: t.project_url ?? null,
    // The implicit direction, which the prototype's viewer names in its code (moviola.rs checks the core's).
    main_direction: "principale",
    milestones: t.milestones.map((m) => filled({ opens_direction: null, process: null, author: null }, {
      ...m,
      refs: filled({ issue: [], commit: [], report: [] }, m.refs),
      process: m.process ? filled({ round: null, checks: [], defects: null }, m.process) : null,
      author: m.author ? filled({ role: null, agent: null, model: null, session: null }, m.author) : null,
      links: links(m.links),
    })),
    directions: t.directions.map((d) => filled({ opened_by: null, reason: null }, d)),
    aggregates: t.aggregates.map((a) => ({
      ...filled({ project: null, window: [], round: null, declared_subagents: null, models: [], checks: [], defects: null, tokens: null }, a),
      models: (a.models ?? []).map((m) => filled({ agent: null, model: null, effort: null, sessions: null, via: null }, m)),
      checks: (a.checks ?? []).map((c) => filled({ kind: null, outcome: null, defects: [], model: null }, c)),
      defects: a.defects ? filled({ found: null, closed: null, closes: [] }, a.defects) : null,
    })),
  };
}

// The prototype's answers on one view, in the product's names.
function expected(view, windows) {
  context.dati = view;
  context.finestre = windows;
  const answer = JSON.parse(run(String.raw`(() => {
    S.P = prepara(dati);
    const tono = c => c.id === "principale" ? { kind: "main" } : c.colore === TENUE ? { kind: "abandoned" }
      : { kind: "branch", hue: COLORI_CORSIA.indexOf(c.colore) };
    const lanes = S.P.corsie.map(c => ({ id: c.id, index: c.k, status: c.stato, recorded: !!c.d, opened: c.aperta,
      closed: c.chiusa, closedBy: c.chiusaDa ? c.chiusaDa.id : null, tone: tono(c), nodes: c.nodi.map(m => m.id) }));
    const nodes = S.P.ms.map(m => ({ id: m.id, lane: m.corsia.k, t: m.t, children: m.figli,
      rings: m.anelli.map(a => ({ declared: !a.nd, model: a.voce ? a.voce.modello : null })),
      badges: m.badge, declared: m.dichiarati }));
    const tempi = S.P.ms.map(m => m.t);
    const campioni = [S.P.t0 - 7 * ORA, S.P.t0 - 3 * ORA, ...tempi, ...tempi.slice(1).map((t, i) => (t + tempi[i]) / 2),
      S.P.t1 + 3 * ORA, S.P.t1 + 7 * ORA];
    const layouts = finestre.map(([w, h, f]) => {
      S.finestra = null;
      if (f) impostaFinestra(Date.parse(f[0]), Date.parse(f[1]));
      const L = layoutA(S.P, w, h);
      const xs = S.P.ms.map(m => L.pos[m.id].x);
      return { width: w, height: h, window: S.finestra ? { from: S.finestra.dal, to: S.finestra.al } : null,
        x: xs, y: S.P.ms.map(m => L.pos[m.id].y), laneY: L.ys, step: L.passo, left: L.sx, i0: L.i0, i1: L.i1,
        clip: L.ritaglio, stageWidth: L.larghezza, stageHeight: L.altezza,
        xAt: campioni.map(t => [t, L.xT(t)]), tAt: xs.length > 1 ? [xs[0] - 5, ...xs.map(x => x + 7), xs[xs.length - 1] + 5].map(x => [x, L.tX(x)]) : [] };
    });
    S.finestra = null;
    const scala = scalaTempo(tempi, 0, 800);
    const scale = { x0: 0, x1: 800, x: campioni.map(t => [t, scala(t)]), joints: scala.giunte.map(g => ({ x: g.x, from: g.da, to: g.a })) };
    return JSON.stringify({ lanes, nodes, layouts, scale });
  })()`));
  for (const lane of answer.lanes) lane.status = STATUS[lane.status];
  return answer;
}

// tools/tests/test_moviola_viewer.py, PROVA: five milestones, a direction that comes back, one task with a check.
const node = (i, ts, tipo, direzione, extra = {}) => ({ id: `M${i}`, ts, tipo, titolo: `node ${i}`, direzione,
  salute: "in salute", genitori: i ? [`M${i - 1}`] : [], rif: { issue: [] }, link: { issue: [], commit: [], report: [] }, ...extra });
const viewer = { progetto: "p", project_url: null, milestones: [
  node(0, "2026-10-14T10:00:00Z", "milestone", "principale"),
  node(1, "2026-10-15T10:00:00Z", "esplorazione", "x"),
  node(2, "2026-10-15T12:00:00Z", "milestone", "x", { salute: "a rischio", processo: { round: null, controlli: [], difetti: { trovati: 3, chiusi: null } } }),
  node(3, "2026-10-19T09:00:00Z", "rientro", "principale"),
  node(4, "2026-10-20T09:00:00Z", "decisione", "principale", { rif: { issue: [7] } })],
direzioni: [{ id: "x", titolo: "X", stato: "rientrata", motivo: "because", aperta_da: "M1" }],
aggregati: [{ task: 7, round: 1, modelli: [{ agente: "claude", modello: "claude-opus-5-5", sessioni: 1 }],
  controlli: [{ tipo: "test", esito: "ok" }] }] };

const oracle = JSON.parse(readFileSync(join(ROOT, "fixtures/moviola/oracle.json"), "utf8"));
const sizes = [[800, 400, null], [1280, 720, null], [800, 400, ["2026-10-15T09:00:00Z", "2026-10-19T10:00:00Z"]]];
const cases = [
  { name: "viewer", view: viewer },
  { name: "alpha", view: oracle.views.alpha },
  { name: "beta", view: oracle.views.beta },
].map(({ name, view }) => ({ name, timeline: product(view), expected: expected(view, sizes) }));
const portfolio = oracle.portfolio.map((entry) => filled({ title: null }, translated(entry)));

const out = { recorded: "node prove/moviola-record.mjs, from the dossier's tools/moviola/static/moviola.js", portfolio, cases };
writeFileSync(join(UI, "harness/moviola.json"), JSON.stringify(out, null, 1) + "\n");
console.log(`harness/moviola.json: ${cases.map((c) => `${c.name} ${c.timeline.milestones.length}`).join(", ")}`);
