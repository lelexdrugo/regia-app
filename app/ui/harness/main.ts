// The harness's page: one view of the app on one fixture, chosen by the address, for the proofs of prove/
// (README.md). #/<view>?data=<state>&lang=<it|en|xx>. Once the view is drawn, <html data-settled> holds the
// address, and the proofs wait for it. The app's sections are the whole shell on that fixture: moving between
// them with the same fixture leaves the shell mounted, as the app does, and the shell says when it has drawn.
// Test only: none of this is in the app's bundle.
import "../src/tokens.css";
import "../src/app.css";

import { mount, tick, unmount } from "svelte";

import tauri from "../../src-tauri/tauri.conf.json";
import type { BriefView, Document } from "../src/lib/bindings";
import DocumentView from "../src/lib/content/Document.svelte";
import { type Catalog, catalog } from "../src/lib/i18n";
import { SECTIONS } from "../src/lib/route";
import { appRenderer, type Check, type CorpusCase, loadFonts, type Renderer, v07 } from "../src/lib/v07";
import App from "../src/App.svelte";
import briefs from "./brief.json";
import Control from "./Control.svelte";
import corpusFile from "./corpus.json";
import documentsFile from "./documents.json";
import { pseudo } from "./pseudo";

declare global {
  interface Window {
    /** What the V07 view found, for prove/v07.js. */
    harnessChecks?: Check[];
  }
}

interface Source {
  name: string;
  markdown: string;
}

// The core wrote these trees, and app/src-tauri/src/ipc.rs keeps both files current: JSON widens their kinds
// to strings, so they are typed here, once.
const corpus = corpusFile as unknown as (CorpusCase & Source)[];
const documents = documentsFile as unknown as (Source & { document: Document })[];

const BRIEFS: Record<string, BriefView> = { demo: briefs.demo, worst: briefs.worst, empty: briefs.empty, one: briefs.one };

/** 1,284 tasks, the UX prototype's longest queue: the demo's titles over and over. */
function huge(): BriefView {
  const { waiting_on_owner, ready, in_progress } = briefs.demo;
  const titles = [...waiting_on_owner, ...ready, ...in_progress].map((task) => task.title);
  const tasks = (first: number, count: number) =>
    Array.from({ length: count }, (_, index) => ({ number: first + index, title: titles[index % titles.length] ?? "" }));
  return { waiting_on_owner: tasks(10_000, 412), ready: tasks(20_000, 640), in_progress: tasks(30_000, 232), graph: null };
}

function brief(data: string): () => Promise<BriefView> {
  if (data === "loading") {
    return () => new Promise<BriefView>(() => {});
  }
  if (data === "error") {
    // The app's command rejects with the core's error, as text.
    return () => Promise.reject(briefs.error);
  }
  const view = data === "huge" ? huge() : BRIEFS[data];
  if (!view) {
    throw new Error(`no brief fixture named ${data}`);
  }
  return () => Promise.resolve(view);
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

async function show(): Promise<void> {
  const route = location.hash;
  const [path = "", query = ""] = route.replace(/^#\/?/, "").split("?");
  const view = path.split("/")[0] ?? "";
  const params = new URLSearchParams(query);
  const data = params.get("data") ?? "demo";
  const language = params.get("lang") ?? "en";
  const shell = (SECTIONS as readonly string[]).includes(view);
  const fixture = `${data}&${language}`;
  if (shell && shellOf === fixture) {
    return;
  }

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
    mounted.push(mount(App, { target: root, props: { strings: catalogOf, product: tauri.productName, load: brief(data), onsettled: settle } }));
    if (view === "brief" && data === "loading") {
      await tick();
      settle();
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
    case "v07":
      window.harnessChecks = await v07(corpus, params.get("renderer") === "unsafe" ? unsafe : appRenderer(catalogOf));
      break;
  }
  await tick();
  settle();
}

addEventListener("hashchange", () => void show());
// Every face first, as the app has them once its shell is drawn: the V07 proof counts what loads while it renders.
void loadFonts().then(show);
