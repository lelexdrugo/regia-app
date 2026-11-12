// The harness's page: one view of the app on one fixture, chosen by the address, for the proofs of prove/
// (README.md). #/<view>?data=<state>&lang=<it|en|xx>. Once the view is drawn, <html data-settled> holds the
// address, and the proofs wait for it. The app's sections are the whole shell on that fixture: moving between
// them with the same fixture leaves the shell mounted, as the app does, and the shell says when it has drawn.
// Test only: none of this is in the app's bundle.
import "../src/tokens.css";
import "../src/app.css";

import { mount, tick, unmount } from "svelte";

import tauri from "../../src-tauri/tauri.conf.json";
import type { BriefView, Document, Settings, SettingsChange } from "../src/lib/bindings";
import type { Done } from "../src/lib/brief";
import DocumentView from "../src/lib/content/Document.svelte";
import { type Catalog, catalog } from "../src/lib/i18n";
import { DENSITIES, DIRECTIONS, draw } from "../src/lib/look";
import { SECTIONS } from "../src/lib/route";
import { appRenderer, type Check, type CorpusCase, loadFonts, type Renderer, v07 } from "../src/lib/v07";
import App from "../src/App.svelte";
import briefs from "./brief.json";
import Control from "./Control.svelte";
import corpusFile from "./corpus.json";
import documentsFile from "./documents.json";
import { pseudo } from "./pseudo";
import Stopped from "./Stopped.svelte";

declare global {
  interface Window {
    /** What the V07 view found, for prove/v07.js. */
    harnessChecks?: Check[];
    /** Every change the shell asked to save, in order, for the proofs of the settings. */
    harnessSaved?: SettingsChange[];
  }
}

/**
 * The settings of the address (`dir`, `density`), as the app's index.html arrives drawn in them, and a save that
 * keeps what it was asked. `save=fail` refuses every save, as a system without a configuration folder would.
 */
function settingsOf(params: URLSearchParams): { settings: Settings; save: (change: SettingsChange) => Promise<Settings> } {
  const direction = DIRECTIONS.find((name) => name === params.get("dir")) ?? "montaggio";
  const density = DENSITIES.find((name) => name === params.get("density")) ?? "compact";
  let settings: Settings = { direction, density, language: null };
  draw(settings);
  window.harnessSaved = [];
  const save = (change: SettingsChange) => {
    window.harnessSaved?.push(change);
    if (params.get("save") === "fail") {
      return Promise.reject("this system has no configuration folder");
    }
    settings = { ...settings, ...change };
    return Promise.resolve(settings);
  };
  return { settings, save };
}

interface Source {
  name: string;
  markdown: string;
}

// The core wrote these trees, and app/src-tauri/src/ipc.rs keeps both files current: JSON widens their kinds
// to strings, so they are typed here, once.
const corpus = corpusFile as unknown as (CorpusCase & Source)[];
const documents = documentsFile as unknown as (Source & { document: Document })[];

// Written by hand in the shape of ipc.rs's BriefView: JSON widens its enums to strings, so they are typed here, once.
const fixtures = briefs as unknown as Record<"demo" | "worst" | "empty" | "one", BriefView> & {
  error: string;
  done: Record<string, Done[]>;
};
const BRIEFS: Record<string, BriefView> = { demo: fixtures.demo, worst: fixtures.worst, empty: fixtures.empty, one: fixtures.one };

/** 1,284 tasks, the UX prototype's longest queue: the demo's tasks over and over, numbered anew. */
function huge(): BriefView {
  const { waiting_on_owner, ready, in_progress } = fixtures.demo;
  const tasks = (from: BriefView["ready"], first: number, count: number) =>
    Array.from({ length: count }, (_, index) => ({ ...from[index % from.length]!, number: first + index }));
  return {
    ...fixtures.demo,
    waiting_on_owner: tasks(waiting_on_owner, 10_000, 412),
    session: [],
    ready: tasks(ready, 20_000, 640),
    in_progress: tasks(in_progress, 30_000, 232),
  };
}

function fixtureOf(data: string): BriefView {
  const view = data === "huge" ? huge() : BRIEFS[data];
  if (!view) {
    throw new Error(`no brief fixture named ${data}`);
  }
  return view;
}

/**
 * The brief's answer for the address. Two negative controls of prove/brief.js change what the view is given, as a
 * view with the mistake would show it: `sort=number`, each list sorted by number; `ported=all`, no section named
 * as not read.
 */
function brief(data: string, params: URLSearchParams): () => Promise<BriefView> {
  if (data === "loading") {
    return () => new Promise<BriefView>(() => {});
  }
  if (data === "error") {
    // The app's command rejects with the core's error, as text.
    return () => Promise.reject(fixtures.error);
  }
  let view = fixtureOf(data);
  if (params.get("sort") === "number") {
    const sorted = (tasks: BriefView["ready"]) => [...tasks].sort((a, b) => a.number - b.number);
    view = { ...view, waiting_on_owner: sorted(view.waiting_on_owner), session: sorted(view.session), ready: sorted(view.ready), in_progress: sorted(view.in_progress) };
  }
  if (params.get("ported") === "all") {
    view = { ...view, not_ported: [] };
  }
  const answer = view;
  return () => Promise.resolve(answer);
}

function documentOf(data: string): Document {
  if (data === "huge") {
    // A document past the core's cap of 256 KiB arrives cut, and flagged.
    const demo = documentOf("demo");
    return { blocks: Array.from({ length: 120 }, () => demo.blocks).flat(), truncated: true };
  }
  const found = documents.find((fixture) => fixture.name === data);
  if (!found) {
    throw new Error(`no document fixture named ${data}`);
  }
  return found.document;
}

/**
 * The negative control of the V07 proof: the content's markup taken as HTML, the mistake the app's renderer
 * exists to rule out. It leaves out the meta refresh, which would take the page away before the proof reads it.
 */
const unsafe: Renderer = (target, test) => {
  if (test.name !== "a meta refresh") {
    target.innerHTML = corpus.find((fixture) => fixture.name === test.name)?.markdown ?? ""; // V08: the V07 proof's negative control, in the harness only
  }
  return () => target.replaceChildren();
};

const root = document.getElementById("root") as HTMLElement;
let mounted: ReturnType<typeof mount>[] = [];
/** The fixture the mounted shell was given, while one is mounted. */
let shellOf: string | null = null;

function strings(language: string): Catalog {
  return language === "xx" ? pseudo(catalog("en", tauri.productName)) : catalog(language, tauri.productName);
}

/** The address the shell has drawn, read when it says so: after its own navigation, the address is a new one. */
function settle(): void {
  document.documentElement.dataset["settled"] = location.hash;
}

/** Each mount's number: a shell that is going away may still say it has drawn, and only the last one counts. */
let mounts = 0;

/** An address's view, its query and its fixture: what the harness mounts for it. */
function parts(route: string) {
  const [path = "", query = ""] = route.replace(/^#\/?/, "").split("?");
  const params = new URLSearchParams(query);
  const fixture = ["data", "lang", "dir", "density", "save", "help", "quota", "sort", "ported"].map((name) => params.get(name)).join("&");
  return { view: path.split("/")[0] ?? "", query, params, fixture };
}

async function show(): Promise<void> {
  const route = location.hash;
  const { view, query, params, fixture } = parts(route);
  const data = params.get("data") ?? "demo";
  const language = params.get("lang") ?? "en";
  const shell = (SECTIONS as readonly string[]).includes(view);
  // The shell's own links and keys write a bare address: the fixture stays the mounted one.
  if (shell && shellOf !== null && (query === "" || shellOf === fixture)) {
    return;
  }

  // Only this mount, and only while the address is still of its fixture (or bare, after the shell's own move),
  // may say it has drawn: a shell on its way out hears the next address too.
  const mine = ++mounts;
  const drawn = () => {
    const now = parts(location.hash);
    if (mine === mounts && (now.query === "" || now.fixture === fixture)) {
      settle();
    }
  };
  delete document.documentElement.dataset["settled"];
  delete window.harnessChecks;
  await Promise.all(mounted.map((component) => unmount(component)));
  mounted = [];
  shellOf = null;
  root.replaceChildren();
  const catalogOf = strings(language);
  document.documentElement.lang = catalogOf.language;
  const onopenlink = () => {};

  if (shell) {
    shellOf = fixture;
    // In the pseudo-locale every language's catalog is wrapped, so the other one's name is a message too.
    const catalogFor = (other: string) => (language === "xx" ? pseudo(strings(other)) : strings(other));
    // help=1 opens the help as ? does, once the screen is drawn, for the proofs that read it.
    let helped = params.get("help") !== "1";
    const onsettled = async () => {
      if (!helped) {
        helped = true;
        dispatchEvent(new KeyboardEvent("keydown", { key: "?" }));
        await tick();
      }
      drawn();
    };
    // quota=<used> shows the sidebar's quota on a fixture (#439/D29): the app has no source for it yet.
    const used = params.get("quota");
    const quota = used === null ? null : { service: "claude", used: Number(used) };
    // What the app decided today: none in the app yet (#477), some in the demo's and the worst case's fixtures.
    const done = fixtures.done[data] ?? [];
    const props = { strings: catalogOf, catalogFor, product: tauri.productName, load: brief(data, params), done, ...settingsOf(params), quota, onsettled };
    mounted.push(mount(App, { target: root, props }));
    if (view === "brief" && data === "loading") {
      await tick();
      drawn();
    }
    return;
  }
  switch (view) {
    case "document":
      mounted.push(
        mount(DocumentView, { target: root, props: { document: documentOf(data), origin: "agent", strings: catalogOf, onopenlink } }),
      );
      break;
    case "corpus":
      for (const test of corpus) {
        const target = document.createElement("div");
        root.append(target);
        mounted.push(mount(DocumentView, { target, props: { document: test.document, origin: "unknown", strings: catalogOf, onopenlink } }));
      }
      break;
    case "control":
      mounted.push(mount(Control, { target: root, props: { strings: catalogOf } }));
      break;
    case "stopped":
      mounted.push(mount(Stopped, { target: root, props: { brief: fixtureOf(data) } }));
      break;
    case "v07":
      window.harnessChecks = await v07(corpus, params.get("renderer") === "unsafe" ? unsafe : appRenderer(catalogOf));
      break;
  }
  await tick();
  drawn();
}

// One address at a time: a new one waits until the last is drawn, so two mounts never interleave.
let showing: Promise<void> = Promise.resolve();
const next = () => {
  showing = showing.then(show, show);
};
addEventListener("hashchange", next);
// Every face first, as the app has them once its shell is drawn: the V07 proof counts what loads while it renders.
showing = loadFonts().then(show);
