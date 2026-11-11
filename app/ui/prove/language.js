// The language changes without reloading and without losing the place (#475, Acceptance 3): on the brief of 1,284
// tasks, scrolled far down, the sidebar's language is changed with the arrow keys. After it the page is the same
// page (a mark left on it is still there, and the browser saw no navigation), at the same address and scroll,
// with the focus on the language chosen, in the other language's words, and the choice was saved. Then back.
// The negative control: the same change made the way an app without it would, a reload in the other language,
// must lose the place.
import { messages } from "./messages.js";
import { find, focus, focused, named } from "./roles.js";
import { open } from "./views.js";

const ROUTE = "#/brief?data=huge&lang=it";

/**
 * Where the person is: what the change must keep. The place is the row at the top of the screen and how far
 * down it sits, not the scroll offset: when the text above it gets shorter, Chrome moves the offset to keep the
 * rows still (scroll anchoring), and that is the place kept.
 */
const PLACE = `(() => {
  const screen = document.querySelector("main .screen");
  const top = screen?.getBoundingClientRect().top ?? 0;
  const row = [...document.querySelectorAll("li[data-number]")].find((item) => item.getBoundingClientRect().top >= top);
  return {
    mark: window.provePlace ?? null,
    hash: location.hash,
    scroll: Math.round(screen?.scrollTop ?? -1),
    row: row?.dataset.number ?? null,
    offset: row ? Math.round(row.getBoundingClientRect().top - top) : null,
    language: document.documentElement.lang,
    title: document.title,
    saved: JSON.stringify(window.harnessSaved ?? []),
  };
})()`;

function judge(before, after, chosen, words) {
  const still = after.place.row === before.place.row && Math.abs(after.place.offset - before.place.offset) <= 2;
  return [
    ["the same page: the mark left on it is there", after.place.mark === "here"],
    ["no navigation and no load", after.navigations === 0],
    ["the same address", after.place.hash === before.place.hash],
    ["the same rows in view, where they were", still && before.place.scroll > 1000],
    ["the focus on the language chosen", after.focus?.role === "radio" && after.focus?.name === chosen && after.focus?.checked === "true"],
    ["<html lang> and the title in the language chosen", after.place.language === words.language && after.place.title === words.title],
    ["the sidebar in the language chosen", after.link === words.link],
    ["the brief's headings in the language chosen", after.heading === words.heading],
  ];
}

/** A language's words the check reads on the page: the title, a link of the sidebar, a heading of the brief. */
function wordsOf(ui, language) {
  const catalog = messages(ui, language);
  return { language, title: catalog["app-title"], link: catalog["nav-daily"], heading: catalog["brief-in-progress"] };
}

/** The node's name in whichever of `both` languages the page shows it. */
async function either(page, role, both, key) {
  for (const words of both) {
    const node = await find(page, role, words[key]);
    if (node) return node.name;
  }
  return undefined;
}

async function observe(page, navigations, both) {
  return {
    place: await page.evaluate(PLACE),
    navigations: navigations.length,
    focus: await focused(page),
    link: await either(page, "link", both, "link"),
    heading: await either(page, "heading", both, "heading"),
  };
}

export const language = {
  browser: true,
  async run({ ui, page, harness }) {
    const ENGLISH = wordsOf(ui, "en");
    const ITALIAN = wordsOf(ui, "it");
    const both = [ENGLISH, ITALIAN];
    const self = { en: messages(ui, "en")["language-self"], it: messages(ui, "it")["language-self"] };
    const errors = page.consoleErrors.length;
    const navigations = [];
    page.on("Page.frameNavigated", (params) => navigations.push(params.frame.url));
    page.on("Page.loadEventFired", () => navigations.push("load"));
    await page.send("Accessibility.enable");
    await open(page, harness, ROUTE);
    await page.evaluate(`document.querySelector("main .screen").scrollTop = 6000; window.provePlace = "here"; true`);
    const results = [];

    // Italian to English, then back, each with the arrow keys on the sidebar's group.
    const steps = [
      { from: self.it, key: "ArrowDown", chosen: self.en, words: ENGLISH, saved: { language: "en" } },
      { from: self.en, key: "ArrowUp", chosen: self.it, words: ITALIAN, saved: { language: "it" } },
    ];
    for (const step of steps) {
      const radio = await find(page, "radio", step.from);
      if (!radio) {
        results.push([`${step.from} → ${step.chosen}: the radio ${step.from} is there`, false]);
        continue;
      }
      await focus(page, radio);
      const before = await observe(page, navigations, both);
      navigations.length = 0;
      await page.press(step.key);
      await page.waitFor(`document.documentElement.lang === ${JSON.stringify(step.words.language)}`, 5_000).catch(() => {});
      const after = await observe(page, navigations, both);
      for (const [name, ok] of judge(before, after, step.chosen, step.words)) results.push([`${step.from} → ${step.chosen}: ${name}`, ok]);
      const saved = JSON.parse(after.place.saved);
      results.push([`${step.from} → ${step.chosen}: saved`, JSON.stringify(saved.at(-1)) === JSON.stringify(step.saved)]);
      console.log(`${step.from} → ${step.chosen} with ${step.key}: focus on ${named(after.focus)}; row #${before.place.row} at ${before.place.offset} px → #${after.place.row} at ${after.place.offset} px (scroll ${before.place.scroll} → ${after.place.scroll}); navigations ${after.navigations}`);
    }

    // The control: the change as a reload in the other language.
    const before = await observe(page, navigations, both);
    navigations.length = 0;
    await page.evaluate(`history.replaceState(null, "", location.href.replace("lang=it", "lang=en")); location.reload(); true`);
    await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(ROUTE.replace("lang=it", "lang=en"))}`);
    const after = await observe(page, navigations, both);
    const control = judge(before, after, self.en, ENGLISH).filter(([, ok]) => !ok).map(([name]) => name);
    const caught = ["the same page: the mark left on it is there", "no navigation and no load", "the same rows in view, where they were"].every((name) =>
      control.includes(name),
    );

    for (const [name, ok] of results) console.log(`${ok ? "ok " : "NO "} ${name}`);
    const failed = results.filter(([, ok]) => !ok).length;
    console.log(`changes of language checked: ${steps.length}; checks: ${results.length}; failed: ${failed}`);
    console.log(`negative control, a reload in the other language: ${caught ? "caught" : "NOT CAUGHT"} ${JSON.stringify(control)}`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return failed === 0 && caught && newErrors.length === 0;
  },
};
