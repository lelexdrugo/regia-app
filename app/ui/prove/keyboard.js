// The shell's keyboard, tried in Chrome on the harness (#475): the UX prototype's prove-keyboard, rewritten on roles
// and fixtures. Each case presses real keys and reads Chrome's accessibility tree, so it finds what a person
// reaches by its role and name, not by a class. The cases: g and a letter to each section, with the focus on its
// heading, and nothing with a modifier or a letter too late; the help on ? and away on Esc, with the focus back
// where it was; the help's words, those #439/D31 asked for among them, in both languages; the Tab order from the
// skip link, with a ring on every stop; the skip link; the settings changed with the arrow keys and saved; no
// transition under prefers-reduced-motion. The negative control: the same checks on the harness's control page,
// which has none of it, must fail, and so must the prototype's glossary against #439/D31's words.
import { all, find, focus, focused, named } from "./roles.js";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const GO = [
  ["d", "#/daily", "Dailies"],
  ["m", "#/moviola", "Moviola"],
  ["c", "#/console", "Console"],
  ["s", "#/settings", "Settings: General"],
  ["b", "#/brief", "Brief"],
];

/** The words #439/D31 found missing from the help, with the two it asked to tell apart. */
const D31 = {
  en: ["Decision", "Gate", "Memo", "Chronicler", "Idle", "Project", "Precheck", "ctx_…", "--on-behalf-of-owner"],
  it: ["Decisione", "Gate", "Memo", "Cronista", "Fermi", "Project", "Precheck", "ctx_…", "--on-behalf-of-owner"],
};

/** The terms of the UX prototype's help, in English: what the help had before. */
const PROTOTYPE = ["Gate", "Chain", "Unblocks", "Moviola", "Dailies", "Dossier", "Off the tracker", "Orchestrator", "Controller", "Worker", "Round", "Lock", "Fallback"];

const missing = (terms, language) => D31[language].filter((word) => !terms.includes(word));

const RING = `(() => {
  const element = document.activeElement;
  if (!element || element === document.body) return false;
  const style = getComputedStyle(element);
  return style.outlineStyle !== "none" && parseFloat(style.outlineWidth) >= 1;
})()`;

const hash = (page) => page.evaluate("location.hash");

/**
 * A real load: Page.navigate to an address that differs only after # does not reload the page, so each load has a
 * query of its own, which the harness does not read.
 */
let loads = 0;
async function fresh(page, harness, route) {
  loads += 1;
  await page.goto(`${harness}?load=${loads}${route}`);
  await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(route)}`);
}

/** g and a letter, then where the page went and what holds the focus. */
async function go(page, letter, modifiers = 0) {
  await page.press("g", modifiers);
  await page.press(letter, modifiers);
  await sleep(150);
  return { hash: await hash(page), focus: await focused(page) };
}

/** The help, as ? leaves it: the dialog, its terms and definitions, and the focus. */
async function help(page) {
  await page.press("?");
  await sleep(150);
  const dialog = await find(page, "dialog", "Keyboard") ?? await find(page, "dialog", "Tastiera");
  return {
    dialog,
    focus: await focused(page),
    terms: (await all(page, "term")).map((node) => node.name),
    definitions: (await all(page, "definition")).length,
  };
}

/** The first `count` stops of Tab from the top of the page. */
async function tabs(page, count) {
  await page.evaluate("document.activeElement?.blur(); window.getSelection()?.removeAllRanges(); true");
  const stops = [];
  for (let index = 0; index < count; index++) {
    await page.press("Tab");
    stops.push({ node: await focused(page), ring: await page.evaluate(RING) });
  }
  return stops;
}

export const keyboard = {
  browser: true,
  async run({ page, harness }) {
    const errors = page.consoleErrors.length;
    await page.send("Accessibility.enable");
    const results = [];
    const check = (name, ok, detail = "") => results.push([name, ok, detail]);

    // g and a letter.
    await fresh(page, harness, "#/brief?data=demo&lang=en");
    for (const [letter, to, title] of GO) {
      const after = await go(page, letter);
      await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(to)}`, 5_000).catch(() => {});
      const heading = await focused(page);
      check(`g ${letter} opens ${to}, the focus on its heading`, after.hash === to && heading?.role === "heading" && heading.name === title, named(heading));
    }
    check("Ctrl+g Ctrl+d goes nowhere", (await go(page, "d", 2)).hash === "#/brief");
    await page.press("g");
    await sleep(1_400);
    await page.press("d");
    await sleep(150);
    check("g, then a letter after 1.2 s, goes nowhere", (await hash(page)) === "#/brief");

    // The help: ? opens it over the page, Esc closes it, the focus goes back.
    const opener = await find(page, "link", "Settings");
    await focus(page, opener);
    const opened = await help(page);
    check("? opens the help, a modal dialog named Keyboard", opened.dialog?.modal === true, named(opened.dialog));
    check("the focus is inside the help", opened.focus?.role === "button" && opened.focus.name === "Close", named(opened.focus));
    check("the help's words have their meanings", opened.terms.length > 0 && opened.terms.length === opened.definitions, `${opened.terms.length} terms, ${opened.definitions} definitions`);
    check("the help has #439/D31's words, in English", missing(opened.terms, "en").length === 0, JSON.stringify(missing(opened.terms, "en")));
    await page.press("Escape");
    await sleep(150);
    const back = await focused(page);
    check("Esc closes the help", !(await find(page, "dialog")), "");
    check("the focus is back where it was", back?.role === "link" && back.name === "Settings", named(back));
    await fresh(page, harness, "#/brief?data=demo&lang=it");
    const italian = await help(page);
    check("the help has #439/D31's words, in Italian", missing(italian.terms, "it").length === 0, JSON.stringify(missing(italian.terms, "it")));
    await page.press("Escape");

    // Tab from the top: the skip link first, then the sidebar in order, a ring on every stop.
    await fresh(page, harness, "#/brief?data=demo&lang=en");
    const stops = await tabs(page, 9);
    const order = stops.map(({ node }) => named(node));
    const expected = ['link "Skip to content"', 'link "regia: go to the brief"', 'link "Brief"', 'link "Dailies"', 'link "Moviola"', 'link "Console"', 'link "Settings"', 'button "Help"', 'radio "English"'];
    check("Tab: the skip link, the sidebar in order, then the language", expected.every((stop, index) => order[index] === stop), order.join(" · "));
    check("Tab: a visible ring on every stop", stops.every((stop) => stop.ring), stops.filter((stop) => !stop.ring).map(({ node }) => named(node)).join(", "));

    // The skip link leads to the screen's heading.
    await fresh(page, harness, "#/brief?data=demo&lang=en");
    await tabs(page, 1);
    await page.press("Enter");
    await sleep(100);
    const skipped = await focused(page);
    check("the skip link puts the focus on the screen's heading", skipped?.role === "heading" && skipped.name === "Brief", named(skipped));

    // The settings, with the arrow keys: Black after Room and Sheet, then the comfortable density, each saved.
    await fresh(page, harness, "#/settings?data=demo&lang=en");
    await focus(page, await find(page, "radio", "Room"));
    await page.press("ArrowRight");
    await page.press("ArrowRight");
    await focus(page, await find(page, "radio", "Compact"));
    await page.press("ArrowRight");
    await sleep(150);
    const look = await page.evaluate(`({ dir: document.documentElement.dataset.dir, density: document.documentElement.dataset.density, saved: window.harnessSaved })`);
    const chosen = (await all(page, "radio")).filter((node) => node.checked === "true").map((node) => node.name);
    check("the arrow keys choose Black and Comfortable, drawn at once", look.dir === "nero" && look.density === "comfortable", `${look.dir} ${look.density}`);
    check("each choice is saved", JSON.stringify(look.saved.slice(-2)) === JSON.stringify([{ direction: "nero" }, { density: "comfortable" }]), JSON.stringify(look.saved));
    check("the radios say what is chosen", ["Black", "Comfortable"].every((name) => chosen.includes(name)), chosen.join(", "));

    // No motion when the system asks for none.
    const moving = await page.evaluate(`getComputedStyle(document.querySelector("aside nav a")).transitionDuration`);
    await page.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
    const still = await page.evaluate(`getComputedStyle(document.querySelector("aside nav a")).transitionDuration`);
    await page.send("Emulation.setEmulatedMedia", { features: [] });
    check("prefers-reduced-motion: no transition", /^0s(, 0s)*$/.test(still) && !/^0s(, 0s)*$/.test(moving), `${moving} → ${still}`);

    // The control: a page with none of it.
    await fresh(page, harness, "#/control?data=planted&lang=en");
    const control = [];
    if ((await go(page, "s")).hash === "#/settings") control.push("g s opened the settings");
    if ((await help(page)).dialog) control.push("? opened a help");
    const first = (await tabs(page, 1))[0];
    if (named(first.node) === 'link "Skip to content"') control.push("Tab found a skip link");
    const caught = control.length === 0 && missing(PROTOTYPE, "en").length > 0;

    for (const [name, ok, detail] of results) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
    const failed = results.filter(([, ok]) => !ok).length;
    console.log(`keyboard checks: ${results.length}; failed: ${failed}`);
    console.log(`negative control, the control page and the prototype's glossary: ${caught ? "caught" : "NOT CAUGHT"} (prototype misses ${JSON.stringify(missing(PROTOTYPE, "en"))}${control.length ? `; passed: ${control.join(", ")}` : ""})`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return failed === 0 && caught && newErrors.length === 0;
  },
};
