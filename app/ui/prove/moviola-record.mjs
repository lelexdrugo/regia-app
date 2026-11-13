// Records the prototype's answers for the moviola proof (prove/moviola.js) into harness/moviola.json: the lanes,
// nodes, layouts and time scale that the prototype's viewer computes on the fixtures, beside the same fixtures in
// the product's shape; then, for every milestone, the frame the prototype draws (`disegnaL3`, read back from a
// small DOM) and its chat package (`pacchetto`), with the sidecars as the core sends them (#487). It is the
// prototype's side, as crates/regia-conformance/oracle/moviola_oracle.py is for the core, so it speaks the
// prototype's keys; the product's sources never do (the guard of regia-conformance).
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

// A DOM as small as the prototype's frame (`disegnaL3`) needs: elements with attributes and children, text as
// strings. The frame is drawn into `l3`, then read back (frameOf, below).
class Element {
  constructor(tag) {
    Object.assign(this, { tagName: tag, attributes: {}, childNodes: [], className: "", style: {}, dataset: {}, hidden: false, scrollTop: 0 });
  }
  set textContent(value) { this.childNodes = [String(value)]; }
  get textContent() { return this.childNodes.map((c) => (typeof c === "string" ? c : c.textContent)).join(""); }
  append(...nodes) { for (const node of nodes) this.childNodes.push(typeof node === "object" ? node : String(node)); }
  appendChild(node) { this.childNodes.push(node); return node; }
  replaceChildren(...nodes) { this.childNodes = []; this.append(...nodes); }
  setAttribute(name, value) { this.attributes[name] = String(value); }
  getAttribute(name) { return name in this.attributes ? this.attributes[name] : null; }
  addEventListener() {}
}
const l3 = new Element("section");

// The prototype's viewer in a context with a DOM only as large as loading and the frame need (test_moviola_viewer.py,
// CARICA).
const context = {
  console, Intl, URLSearchParams, location: { hash: "", origin: "https://origin.invalid" }, window: {},
  document: {
    addEventListener() {},
    getElementById: (id) => (id === "l3" ? l3 : null),
    createElement: (tag) => new Element(tag),
    createElementNS: (_, tag) => new Element(tag),
  },
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

// A frame of every kind the L3 reads (#487): a `#N` in each of the three sections that link them, one the milestone
// does not point at (the same repository as the others), one followed by a letter, markup in a text, `in_breve` on
// one sidecar only, a direction opened and then closed, a task with reviews and tokens, one with no session and no
// tokens (a colleague without sync), and a milestone with no sidecar.
const issue = (n) => ({ testo: `#${n}`, url: `https://github.com/acme/tasks/issues/${n}` });
const frame = { progetto: "frame", project_url: "https://github.com/users/acme/projects/7", milestones: [
  { id: "01JCF000000000000000000000", ts: "2026-10-14T10:00:00Z", tipo: "milestone", titolo: "#12 merged after review", direzione: "principale",
    salute: "in salute", genitori: [], rif: { issue: [12, 14], commit: ["abc1234"], report: [".regia/report/2026-10-14-12-review.md", ".regia/report/gone.md"] },
    processo: { round: 2, controlli: ["revisione-avversaria", "test"], difetti: { trovati: 2, chiusi: 2 } },
    link: { issue: [issue(12), issue(14)], commit: [{ testo: "abc1234", url: "https://github.com/acme/frame/commit/abc1234", hash: "abc1234" }],
      report: [{ testo: ".regia/report/2026-10-14-12-review.md", percorso: "<root>/projects/frame/.regia/report/2026-10-14-12-review.md", tipo: "relativo", esiste: true },
        { testo: ".regia/report/gone.md", percorso: "<root>/projects/frame/.regia/report/gone.md", tipo: "relativo", esiste: false }] } },
  { id: "01JCF100000000000000000000", ts: "2026-10-15T09:00:00Z", tipo: "esplorazione", titolo: "A probe opens its lane", direzione: "probe",
    apre_direzione: "probe", salute: "a rischio", genitori: ["01JCF000000000000000000000"], rif: { issue: [14] },
    link: { issue: [issue(14)], commit: [{ testo: "fedcba9", url: null, hash: "fedcba9876543210" }], report: [] } },
  { id: "01JCF200000000000000000000", ts: "2026-10-16T09:00:00Z", tipo: "abbandono", titolo: "The probe is dropped", direzione: "principale",
    salute: "in attesa del titolare", genitori: ["01JCF100000000000000000000"], rif: { issue: [] }, link: { issue: [], commit: [], report: [] } }],
direzioni: [{ id: "probe", titolo: "A probe", aperta_da: "01JCF100000000000000000000", stato: "abbandonata", motivo: "To try the other road." }],
aggregati: [
  { task: 12, finestra: ["2026-10-14T08:00:00Z", "2026-10-14T10:00:00Z"], round: 2, sottoagenti_dichiarati: 1,
    modelli: [{ agente: "claude", modello: "claude-opus-5-5", effort: "max", sessioni: 1 }, { agente: "codex", modello: "gpt-5.5-codex", effort: "high", sessioni: 1 },
      { agente: "claude", modello: "claude-opus-5-5", effort: null, sessioni: 1, via: "report" }],
    controlli: [{ tipo: "revisione-avversaria", esito: "difetti", modello: "gpt-5.5-codex", difetti: ["D1", "D2"] }, { tipo: "test", esito: "ok", difetti: [] }],
    difetti: { trovati: 2, chiusi: 2, chiude: ["#9/D3"] }, token: { in: 120000, out: 8000, cache_lettura: 900000, cache_scrittura: 40000 } },
  { task: 14, round: 1, sottoagenti_dichiarati: null, modelli: [], controlli: [], difetti: { trovati: null, chiusi: null }, token: null }],
snapshot: {
  "01JCF000000000000000000000": { cambiato: ["Merged #12 after two rounds.", "A text with <script>alert(1)</script> & a < b stays text."],
    direzione: "The review of #12 closed both defects.", decisioni_aperte: ["Pick the probe: #13 or #14."],
    rischi: ["#14 may slip; #14a and x#14 are not the same."], prossimo: "Open the probe of #14.", in_breve: "The review is done. The project is on time." },
  "01JCF100000000000000000000": { cambiato: [], direzione: "Try the other road.", decisioni_aperte: [], rischi: ["No #N here."], prossimo: "" },
  "01JCF200000000000000000000": null,
} };

// The sidecars as the core sends them (records.rs, Snapshot): every key, `null` or `[]` for none.
function sidecar(raw) {
  if (!raw) return null;
  const process = raw.processo ? { round: raw.processo.round ?? null, checks: raw.processo.controlli ?? [],
    defects: raw.processo.difetti ? { found: raw.processo.difetti.trovati ?? null, closed: raw.processo.difetti.chiusi ?? null } : null } : null;
  return { id: raw.id ?? null, ts: raw.ts ?? null, in_brief: raw.in_breve ?? null, changed: raw.cambiato ?? [], direction: raw.direzione ?? null,
    open_decisions: raw.decisioni_aperte ?? [], risks: raw.rischi ?? [], next: raw.prossimo ?? null, changed_claims: raw.claim_cambiati ?? [],
    deviations: (raw.deviazioni ?? []).map((d) => ({ plan: d.piano ?? null, deviation: d.deviazione ?? null, reason: d.motivo ?? null, impact: d.impatto ?? null })),
    process };
}

const isElement = (node) => typeof node === "object";
const classOf = (node) => (isElement(node) ? node.className || node.attributes.class || "" : "");
const declared = (node) => (node.childNodes.some((c) => classOf(c).includes("nd")) ? null : node.textContent);
const number = (text) => (text === null ? null : Number(text));

/** A list item's text as runs: text, and links with their address; empty text dropped, neighbours joined. */
function runs(nodes) {
  const out = [];
  for (const node of nodes) {
    const run = isElement(node) && node.tagName === "a" ? { text: node.textContent, url: node.attributes.href } : { text: isElement(node) ? node.textContent : node };
    if (!run.text) continue;
    const last = out[out.length - 1];
    if (last && !("url" in last) && !("url" in run)) last.text += run.text;
    else out.push(run);
  }
  return out;
}

const items = (nodes) => {
  const list = nodes.find((n) => isElement(n) && n.tagName === "ul");
  return list ? list.childNodes.map((li) => runs(li.childNodes)) : [];
};
const idOf = (href) => /\/n\/([^?]+)/.exec(href)[1];
// The references of a line: a link where the server resolved an address, the text (a commit's whole hash) where not.
const links = (line) => line.childNodes.filter((n) => isElement(n) && (n.tagName === "a" || n.tagName === "code"))
  .map((x) => ({ text: x.textContent, url: x.tagName === "a" ? x.attributes.href : null }));

/** The frame the prototype drew in `l3`, read back by its sections: the prototype's own words become the product's
 * names, so prove/moviola.js compares it with the product's `frame` (src/lib/moviola/frame.ts). */
function frameOf(m, P) {
  const sections = { head: [] };
  let current = "head";
  for (const node of l3.childNodes) {
    if (isElement(node) && node.tagName === "h3") sections[(current = node.textContent)] = [];
    else sections[current].push(node);
  }
  const head = sections.head;
  const chips = head.find((n) => isElement(n) && n.tagName === "div" && !classOf(n)).childNodes.filter(isElement);
  const why = { direction: null, opens: null, closes: [] };
  for (const p of sections["Perché"]) {
    const text = p.textContent;
    const opens = /^Apre la corsia «(.*)»\. (.*)$/s.exec(text);
    const closes = /^Chiude la corsia «(.*)» \((.*)\)\. (.*)$/s.exec(text);
    if (classOf(p).includes("perche")) why.direction = text || null;
    else if (opens) why.opens = { title: opens[1], reason: opens[2] || null };
    else if (closes) why.closes.push({ title: closes[1], status: STATUS[closes[2]], reason: closes[3] || null });
    else throw new Error(`a paragraph of the why the recorder does not know: ${text}`);
  }
  const row = sections["Processo (milestone)"][0].childNodes[1].childNodes;
  const refs = sections["Riferimenti"];
  const [issues, commits, reports, pages] = refs;
  const tail = refs.slice(4);
  const after = (prefix) => {
    const p = tail.find((n) => n.tagName === "p" && n.childNodes[0] === prefix);
    return p ? p.childNodes.filter((n) => isElement(n) && n.tagName === "a").map((a) => idOf(a.attributes.href)) : [];
  };
  const buttons = tail.find((n) => classOf(n) === "naviga").childNodes.map((b) => b.textContent);
  const deviation = (runs) => {
    const [, plan, deviation, reason, impact] = /^(.*) → (.*): (.*) \((.*)\)$/s.exec(runs.map((r) => r.text).join(""));
    return [plan, deviation, reason, impact].map((x) => (x === "null" || x === "undefined" ? null : x));
  };
  const reportList = reports.childNodes.find((n) => isElement(n) && n.tagName === "ul");
  return {
    id: m.id,
    title: head.find((n) => isElement(n) && n.tagName === "h2").textContent,
    lane: chips[0].textContent,
    health: HEALTH[chips[1].textContent] ?? null,
    inBrief: head.find((n) => classOf(n).includes("in-breve"))?.textContent ?? null,
    why,
    changed: items(sections["Cambiato"]),
    openDecisions: items(sections["Decisioni aperte"]),
    risks: items(sections["Rischi"]),
    nextStep: sections["Prossimo"][0].textContent || null,
    claims: sections["Claim cambiati"] ? items(sections["Claim cambiati"]).map((r) => r.map((x) => x.text).join("")) : [],
    deviations: sections["Deviazioni"] ? items(sections["Deviazioni"]).map(deviation) : [],
    process: { round: number(declared(row[0])), checks: declared(row[1]) === null ? [] : row[1].textContent.split(", "),
      found: number(declared(row[2])), closed: number(declared(row[3])) },
    work: sections["Chi ha lavorato, per task"].filter((n) => classOf(n) === "task").map((div) => ({
      task: Number(div.childNodes[0].textContent.trim().slice(1)),
      aggregate: !div.childNodes.some((c) => classOf(c) === "nd" && c.textContent.startsWith("nessun aggregato")) })),
    refs: {
      issue: links(issues),
      commit: links(commits),
      report: reportList ? reportList.childNodes.map((li) => ({ text: li.childNodes[0].textContent, path: li.childNodes[0].attributes.title ?? null,
        exists: !li.childNodes.some((c) => classOf(c) === "nd") })) : [],
      projectUrl: pages.childNodes.find((n) => isElement(n) && n.tagName === "a" && n.textContent === "Project")?.attributes.href ?? null,
    },
    parents: after("segue: "),
    children: after("seguito da: "),
    previous: buttons.some((b) => b.includes("precedente")) ? P.ms[m.i - 1].id : null,
    following: buttons.some((b) => b.includes("successivo")) ? P.ms[m.i + 1].id : null,
  };
}

// The prototype's frame and chat package of every milestone of a view.
function frames(view) {
  context.dati = { ...view, snapshot: view.snapshot ?? {} };
  run(`S.P = prepara(dati); S.v = "A"; S.t = S.P.t1; S.rotta = { progetto: S.P.progetto, dir: null, nodo: null };`);
  const P = run("S.P");
  return P.ms.map((m) => {
    run(`S.rotta.nodo = ${JSON.stringify(m.id)}; disegnaL3();`);
    const pack = run(`pacchetto(S.P.perId[${JSON.stringify(m.id)}], S.P.progetto, S.P.snapshot[${JSON.stringify(m.id)}], location.origin)`);
    return { ...frameOf(m, P), pack };
  });
}

const oracle = JSON.parse(readFileSync(join(ROOT, "fixtures/moviola/oracle.json"), "utf8"));
const sizes = [[800, 400, null], [1280, 720, null], [800, 400, ["2026-10-15T09:00:00Z", "2026-10-19T10:00:00Z"]]];
const cases = [
  { name: "viewer", view: viewer },
  { name: "alpha", view: oracle.views.alpha },
  { name: "beta", view: oracle.views.beta },
  { name: "frame", view: frame },
].map(({ name, view }) => ({
  name,
  timeline: product(view),
  snapshots: Object.fromEntries(view.milestones.map((m) => [m.id, sidecar(view.snapshot?.[m.id])])),
  expected: expected(view, sizes),
  frames: frames(view),
}));
const portfolio = oracle.portfolio.map((entry) => filled({ title: null }, translated(entry)));

// The rings' places above and below a glyph, and a lane's half thickness, for the first rings.
const rings = { offsets: run("[0, 1, 2, 3, 4, 5, 6, 7].map(yAnello)"), halfThickness: run("[0, 1, 2, 3, 4, 5].map(semiSpessore)") };
const out = { recorded: "node prove/moviola-record.mjs, from the dossier's tools/moviola/static/moviola.js", portfolio, rings, cases };
writeFileSync(join(UI, "harness/moviola.json"), JSON.stringify(out, null, 1) + "\n");
console.log(`harness/moviola.json: ${cases.map((c) => `${c.name} ${c.timeline.milestones.length}`).join(", ")}`);
