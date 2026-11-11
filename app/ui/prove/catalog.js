// No string written in the code: the UX prototype's prove-i18n, on the app's views. Every view, in every state,
// is rendered in the pseudo-locale xx (harness/pseudo.ts): text with letters must come from the catalog, wrapped
// in ⟦ ⟧, or sit in an element marked data-raw, which holds text from others. The negative control: the harness's
// control page writes a string in the code. As information only, it counts the catalog's messages that no source
// names yet: most belong to screens that come with later slices.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

import { open, show, VIEWS } from "./views.js";

const SCAN = `(() => {
  const found = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = node.textContent.trim();
    const parent = node.parentElement;
    if (!/\\p{L}/u.test(text) || !parent || parent.closest("[data-raw], script, style")) continue;
    if (!text.includes("⟦") && !text.includes("⟧")) found.push(text.slice(0, 60));
  }
  for (const element of document.querySelectorAll("[aria-label], [placeholder], [title], [alt]")) {
    if (element.closest("[data-raw]")) continue;
    for (const name of ["aria-label", "placeholder", "title", "alt"]) {
      const value = element.getAttribute(name);
      if (value && /\\p{L}/u.test(value) && !value.includes("⟦")) found.push("@" + name + " " + value.slice(0, 50));
    }
  }
  return [...new Set(found)];
})()`;

function ids(ui, language) {
  const folder = join(ui, "../../crates/regia-i18n/locales", language);
  return readdirSync(folder)
    .filter((file) => file.endsWith(".ftl"))
    .flatMap((file) => [...readFileSync(join(folder, file), "utf8").matchAll(/^([a-z][\w-]*)\s*=/gm)].map((match) => match[1]));
}

function sources(folder) {
  return readdirSync(folder).flatMap((name) => {
    const path = join(folder, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.(svelte|ts)$/.test(name) ? [readFileSync(path, "utf8")] : [];
  });
}

export const catalog = {
  browser: true,
  async run({ ui, page, harness }) {
    const errors = page.consoleErrors.length;
    const failures = {};
    await open(page, harness, `${VIEWS[0]}&lang=xx`);
    for (const view of VIEWS) {
      const route = `${view}&lang=xx`;
      await show(page, route);
      const found = await page.evaluate(SCAN);
      if (found.length > 0) failures[route] = found;
    }
    await show(page, "#/control?data=planted&lang=xx");
    const control = await page.evaluate(SCAN);
    const caught = control.some((text) => text.startsWith("Written in the code"));

    const english = ids(ui, "en");
    const italian = ids(ui, "it");
    const code = sources(join(ui, "src")).join("\n");
    const named = english.filter((id) => code.includes(`"${id}"`));
    const sameIds = [...english].sort().join() === [...italian].sort().join();
    console.log(`views and states rendered in the pseudo-locale: ${VIEWS.length}`);
    console.log(`messages: ${english.length} in en, ${italian.length} in it, the same ids: ${sameIds ? "yes" : "no"}`);
    console.log(`named in src/: ${named.length}; not yet (information only: their screens come later): ${english.length - named.length}`);
    const failed = Object.keys(failures).length;
    console.log(`strings outside the catalog: ${failed === 0 ? "none" : `${failed} views`}`);
    for (const [route, found] of Object.entries(failures)) console.log(`  ${route}: ${JSON.stringify(found.slice(0, 5))}`);
    console.log(`negative control, a string written in the code: ${caught ? "caught" : "NOT CAUGHT"} ${JSON.stringify(control)}`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return failed === 0 && caught && sameIds && newErrors.length === 0;
  },
};
