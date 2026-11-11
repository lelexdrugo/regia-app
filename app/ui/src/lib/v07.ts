// Test only: the threat model's V07 on the DOM (§3.7, points 2 and 3). Every case of the core's hostile corpus
// is rendered, and the DOM it became is checked: only the renderer's own elements and attributes, the hostile
// text shown as text, no script of the content run, no CSP violation, nothing loaded and no link opened without
// a click. `smoke.ts` runs it on each engine with the app's renderer, under the app's CSP; the harness runs it in
// Chrome, with a deliberately unsafe renderer as its negative control (app/ui/prove).
import { flushSync, mount, unmount } from "svelte";

import type { Block, Document as Tree, Inline } from "./bindings";
import DocumentView from "./content/Document.svelte";
import type { Catalog } from "./i18n";

export interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

export interface CorpusCase {
  name: string;
  document: Tree;
  /** Text the DOM must show as text, markup included. */
  shows: string[];
}

/** Renders one case into `target`, and returns what takes it away. */
export type Renderer = (target: HTMLElement, test: CorpusCase, onopenlink: (url: string) => void) => () => void;

/** The renderer of `content/`, as a view uses it. */
export function appRenderer(strings: Catalog): Renderer {
  return (target, test, onopenlink) => {
    const view = mount(DocumentView, { target, props: { document: test.document, origin: "agent", strings, onopenlink } });
    flushSync();
    return () => void unmount(view);
  };
}

/** The elements `content/` builds, and no other. */
const ELEMENTS = new Set([
  "SECTION", "DIV", "P", "H3", "H4", "H5", "H6", "BLOCKQUOTE", "PRE", "CODE", "OL", "UL", "LI",
  "TABLE", "THEAD", "TBODY", "TR", "TH", "TD", "HR", "SPAN", "EM", "STRONG", "BR", "BUTTON",
]);

/** Its classes. Svelte adds a scoping class of its own, `svelte-<hash>`. */
const CLASSES = new Set(["document", "origin", "content", "truncated", "link", "host", "refused", "note", "image"]);

function attributeProblem(element: Element, attribute: Attr): string | null {
  const where = `<${element.tagName.toLowerCase()} ${attribute.name}>`;
  switch (attribute.name) {
    case "class": {
      const foreign = [...element.classList].filter((name) => !CLASSES.has(name) && !name.startsWith("svelte-"));
      return foreign.length === 0 ? null : `a class from the content: ${where} ${foreign.join(" ")}`;
    }
    case "data-raw":
      return attribute.value === "" ? null : `a value from the content: ${where}`;
    case "type":
      return element.tagName === "BUTTON" && attribute.value === "button" ? null : `an attribute outside the list: ${where}`;
    default:
      return `an attribute outside the list: ${where}`;
  }
}

/** What in this DOM did not come from the renderer's lists: an element, an attribute, a class. */
export function domProblems(root: Element): string[] {
  const problems: string[] = [];
  for (const element of root.querySelectorAll("*")) {
    if (!ELEMENTS.has(element.tagName)) {
      problems.push(`an element outside the list: <${element.tagName.toLowerCase()}>`);
    }
    for (const attribute of element.attributes) {
      const problem = attributeProblem(element, attribute);
      if (problem) {
        problems.push(problem);
      }
    }
  }
  return problems;
}

function linkIn(content: Inline[]): string | null {
  for (const inline of content) {
    if (inline.kind === "link" && inline.target.kind === "external") {
      return inline.target.url;
    }
    const inner = "content" in inline ? linkIn(inline.content) : null;
    if (inner) {
      return inner;
    }
  }
  return null;
}

/** The URL of the first link the core lets open, in the order the renderer draws them. */
function firstLink(blocks: Block[]): string | null {
  for (const block of blocks) {
    const found =
      block.kind === "paragraph" || block.kind === "heading"
        ? linkIn(block.content)
        : block.kind === "quote"
          ? firstLink(block.blocks)
          : block.kind === "list"
            ? block.items.map(firstLink).find((url) => url !== null) ?? null
            : block.kind === "table"
              ? [...block.head, ...block.rows.flat()].map(linkIn).find((url) => url !== null) ?? null
              : null;
    if (found) {
      return found;
    }
  }
  return null;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Renders every case at once, each in an element of its own, waits for what a hostile node would do later (an
 * image's error, a frame's load), then checks each case and the page. Nothing in this window calls the core: on
 * the engines that carry the IPC over fetch, a call would count as a resource.
 */
export async function v07(cases: CorpusCase[], render: Renderer): Promise<Check[]> {
  const opened: string[] = [];
  const violations: string[] = [];
  const onViolation = (event: SecurityPolicyViolationEvent) => {
    violations.push(`${event.effectiveDirective || event.violatedDirective} ${event.blockedURI}`);
  };
  document.addEventListener("securitypolicyviolation", onViolation);
  performance.clearResourceTimings();

  const host = document.createElement("div");
  document.body.append(host);
  const rendered = cases.map((test) => {
    const target = document.createElement("div");
    host.append(target);
    try {
      return { test, target, remove: render(target, test, (url) => opened.push(url)), error: "" };
    } catch (error) {
      return { test, target, remove: () => {}, error: String(error) };
    }
  });
  await sleep(500);

  const checks: Check[] = rendered.map(({ test, target, error }) => {
    const problems = error ? [`the renderer threw: ${error}`] : domProblems(target);
    const text = target.textContent ?? "";
    for (const shown of test.shows) {
      if (!text.includes(shown)) {
        problems.push(`not shown as text: ${shown}`);
      }
    }
    return { name: `v07: ${test.name}`, ok: problems.length === 0, detail: problems.slice(0, 3).join("; ") };
  });
  const loaded = performance.getEntriesByType("resource").map((entry) => entry.name);
  const pwned = "__pwned" in window;
  checks.push(
    { name: "v07: no script of the content ran", ok: !pwned, detail: pwned ? "window.__pwned exists" : "" },
    { name: "v07: no CSP violation while rendering", ok: violations.length === 0, detail: violations.slice(0, 5).join(", ") },
    { name: "v07: nothing loaded while rendering", ok: loaded.length === 0, detail: loaded.slice(0, 5).join(", ") },
    { name: "v07: no link opened without a click", ok: opened.length === 0, detail: opened.join(", ") },
  );

  // A click does call back, with the URL the core let through: the empty list above is not a dead callback.
  const clickable = rendered.find(({ target }) => target.querySelector("button"));
  const expected = clickable ? firstLink(clickable.test.document.blocks) : null;
  clickable?.target.querySelector("button")?.click();
  checks.push({
    name: "v07: a click opens the link the core let through",
    ok: expected !== null && opened.length === 1 && opened[0] === expected,
    detail: `clicked ${clickable?.test.name ?? "nothing"}: ${opened.join(", ") || "no call"}`,
  });

  for (const { remove } of rendered) {
    remove();
  }
  host.remove();
  document.removeEventListener("securitypolicyviolation", onViolation);
  return checks;
}
