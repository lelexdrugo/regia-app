// The shell's keyboard, tried in Chrome on the harness (#475): the UX prototype's prove-keyboard, rewritten on roles
// and fixtures. Each case presses real keys and reads Chrome's accessibility tree, so it finds what a person
// reaches by its role and name, not by a class. The cases: g and a letter to each section, with the focus on its
// heading, and nothing with a modifier or a letter too late; the help on ? and away on Esc, with the focus back
// where it was; the help's words, those #439/D31 asked for among them, in both languages; the Tab order from the
// skip link, with a ring on every stop; the skip link; the settings changed with the arrow keys and saved; no
// transition under prefers-reduced-motion; the sidebar's quota in words, used and left (#439/D29); the moviola's
// search and moments (#487). The negative control: the same checks on the harness's control page, which has none
// of it, must fail, and so must the prototype's glossary against #439/D31's words and its "claude 6 %" against
// #439/D29, and / on the moviola's portfolio, which has no search.
import { format, messages } from "./messages.js";
import { all, find, focus, focused, named } from "./roles.js";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const GO = [
  ["d", "#/daily", "Dailies"],
  ["m", "#/moviola", "Moviola"],
  ["c", "#/console", "Console"],
  ["s", "#/settings", "Settings: General"],
  ["b", "#/brief", "Brief"],
];

/** The words #439/D31 found missing from the help, with the two it asked to tell apart, by their catalog ids. */
const D31 = ["decision", "gate", "memo", "chronicler", "idle", "project", "precheck", "session", "behalf"].map(
  (word) => `words-${word}-term`,
);

/** The terms of the UX prototype's help, in English: what the help had before. */
const PROTOTYPE = ["Gate", "Chain", "Unblocks", "Moviola", "Dailies", "Dossier", "Off the tracker", "Orchestrator", "Controller", "Worker", "Round", "Lock", "Fallback"];

/** The D31 words, as `catalog` spells them, that `terms` lacks. */
const lacking = (catalog, terms) => D31.map((id) => catalog[id]).filter((word) => !terms.includes(word));

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
  const dialog = (await all(page, "dialog"))[0];
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
  async run({ ui, page, harness }) {
    const catalogs = { en: messages(ui, "en"), it: messages(ui, "it") };
    const missing = (terms, language) => lacking(catalogs[language], terms);
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
    const titled = opened.dialog?.name === catalogs.en["keys-title"];
    check("? opens the help, a modal dialog named Keyboard", opened.dialog?.modal === true && titled, named(opened.dialog));
    check("the focus is inside the help", opened.focus?.role === "button" && opened.focus.name === "Close", named(opened.focus));
    check("the help's words have their meanings", opened.terms.length > 0 && opened.terms.length === opened.definitions, `${opened.terms.length} terms, ${opened.definitions} definitions`);
    check("the help has #439/D31's words, in English", missing(opened.terms, "en").length === 0, JSON.stringify(missing(opened.terms, "en")));
    // The moviola's part (#487): the prototype's guide in the page, made again as the help's section.
    const signs = ["kinds", "health", "rings", "badges", "undeclared", "lanes", "playhead", "joint"].map((w) => catalogs.en[`moviola-word-${w}-term`]);
    const moviolaKeys = await page.evaluate(`[...document.querySelectorAll("dialog kbd")].map((k) => k.textContent)`);
    const partOk = signs.every((term) => opened.terms.includes(term)) && ["/", ", .", "← →", "Esc"].every((k) => moviolaKeys.includes(k))
      && (await all(page, "heading")).some((h) => h.name === catalogs.en["moviola-help-title"]);
    check("the help has the moviola's part: its keys and the signs of its stage", partOk, signs.filter((term) => !opened.terms.includes(term)).join(", "));
    await page.press("Escape");
    await sleep(150);
    const back = await focused(page);
    check("Esc closes the help", !(await find(page, "dialog")), "");
    check("the focus is back where it was", back?.role === "link" && back.name === "Settings", named(back));
    await fresh(page, harness, "#/brief?data=demo&lang=it");
    const inItalian = await help(page);
    const italianMissing = missing(inItalian.terms, "it");
    check("the help has #439/D31's words, in Italian", italianMissing.length === 0, JSON.stringify(italianMissing));
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

    // The sidebar's quota says what is used and what is left (#439/D29), in words and as a meter.
    const says = (text, language) => text.includes(format(catalogs[language]["nav-quota-used-left"], { used: 6, left: 94 }));
    for (const language of ["en", "it"]) {
      await fresh(page, harness, `#/brief?data=demo&quota=6&lang=${language}`);
      const meter = (await all(page, "meter"))[0];
      const text = await page.evaluate(`document.querySelector("aside")?.innerText ?? ""`);
      check(`the quota says used and left, in ${language === "en" ? "English" : "Italian"}`, says(text, language) && meter !== undefined && meter.name !== "", named(meter));
    }
    const prototypeQuota = says("claude 6 %", "en");

    // The moviola's search and moments (#487), the UX prototype's cases: / finds, typing filters, Enter opens the
    // first found, `,` walks back within the search, Esc empties it; a search with no moment says why and offers to
    // clear the filters; leaving the screen forgets the search; ← on the range moves the playhead.
    const moviola = catalogs.en;
    const frameMoment = `document.querySelector("aside.frame")?.dataset.moment ?? null`;
    const value = `document.getElementById("search")?.value ?? null`;
    const rows = `[...document.querySelectorAll(".results-list .result")].map((b) => b.dataset.id)`;
    // Each step waits for what the next check reads, not for a time: a slower runner only waits longer.
    const until = (expression) => page.waitFor(expression, 5_000).catch(() => console.log(`  waited 5 s in vain for ${expression.slice(0, 80)}`));
    await fresh(page, harness, "#/moviola/alpha?data=demo&lang=en");
    await page.press("/");
    await until(`document.activeElement?.id === "search"`);
    const searchbox = await focused(page);
    check("/ goes to the search", searchbox?.role === "searchbox" && searchbox.name === moviola["moviola-search"], named(searchbox));
    await page.send("Input.insertText", { text: "probe" });
    await until(`${rows}.length > 0`);
    const hits = await page.evaluate(rows);
    check("typing filters the moments", hits.length > 0 && hits.length < 7, `${hits.length} of 7`);
    await page.press("Enter");
    await until(`(${frameMoment}) !== null`);
    const newest = await page.evaluate(frameMoment);
    check("Enter opens the first moment found, the newest", newest === hits[0] && (await focused(page))?.role !== "searchbox", `${newest}`);
    await page.press(",");
    await until(`(${frameMoment}) !== ${JSON.stringify(newest)}`);
    const before = await page.evaluate(frameMoment);
    check(", goes to the moment before, within the search", before !== newest && hits.includes(before), `${newest} → ${before}`);
    await page.evaluate(`document.getElementById("search").focus(), true`);
    await page.press("Escape");
    await until(`(${value}) === ""`);
    check("Esc empties the search", (await page.evaluate(value)) === "" && (await page.evaluate(rows)).length === 0);
    await page.send("Input.insertText", { text: "zzzz" });
    await until(`document.querySelector(".noresult") !== null`);
    const nothing = await page.evaluate(`document.querySelector(".noresult")?.textContent.trim() ?? ""`);
    // One «Clear the filters» for the search and the filters (#598), right after the field.
    const clears = await all(page, "button", moviola["moviola-filter-clear"]);
    check("a search with no moment says why, and offers one «Clear the filters»", nothing.includes("«zzzz»") && clears.length === 1, `${nothing.slice(0, 70)} · ${clears.length}`);
    await page.press("Tab");
    await page.press("Tab");
    const clearing = await focused(page);
    await page.press("Enter");
    await until(`(${value}) === ""`);
    check("«Clear the filters» from the keyboard empties the search", clearing?.name === moviola["moviola-filter-clear"] && (await page.evaluate(value)) === "", named(clearing));
    await page.send("Input.insertText", { text: "#99" });
    await until(`${rows}.length === 1`);
    await page.press("Enter");
    await until(`(${frameMoment}) === "01JCM600000000000000000000"`);
    check("a #number finds the moment that names the task", (await page.evaluate(frameMoment)) === "01JCM600000000000000000000", await page.evaluate(frameMoment));
    await page.evaluate(`document.getElementById("search").focus(), true`);
    await page.send("Input.insertText", { text: " probe" });
    await page.evaluate(`document.querySelector(".stage canvas").focus(), true`);
    await go(page, "b");
    await page.evaluate(`location.hash = "#/moviola/alpha"`);
    await page.waitFor(`document.documentElement.dataset.settled === "#/moviola/alpha"`, 5_000);
    check("leaving the screen forgets the search", (await page.evaluate(value)) === "", await page.evaluate(value));
    await page.evaluate(`document.querySelector(".scrub").focus(), true`);
    const t1 = await page.evaluate(`document.querySelector(".timecode").dataset.t`);
    await page.press("ArrowLeft");
    await until(`document.querySelector(".timecode").dataset.t !== ${JSON.stringify(t1)}`);
    const t2 = await page.evaluate(`document.querySelector(".timecode").dataset.t`);
    check("← on the range moves the playhead", Number(t2) < Number(t1), `${t1} → ${t2}`);

    // The filters and the project at the playhead (#598), from the keyboard alone: Tab from the search reaches the
    // kinds, Space hides one, the count says how many are left and `.` walks only those; the arrow keys choose the
    // advanced mode, a lane is chosen from its select, and back to basic clears it; «Clear the filters» shows every
    // kind again. Home on the range takes the playhead to the first moment: the panel says what the project was then,
    // and its last moment is a button that opens it.
    const gate = moviola["type-gate"];
    const counted = `document.getElementById("moviola-found")?.dataset.count ?? null`;
    const status = `document.getElementById("moviola-status").dataset.selected`;
    const rawKey = async (name, code, keyCode) => {
      await page.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: name, code, windowsVirtualKeyCode: keyCode });
      await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: name, code, windowsVirtualKeyCode: keyCode });
    };
    await fresh(page, harness, "#/moviola/alpha?data=demo&lang=en");
    await page.press("/");
    await until(`document.activeElement?.id === "search"`);
    let box = null;
    for (let i = 0; i < 14 && box?.name !== gate; i++) {
      await page.press("Tab");
      box = await focused(page);
    }
    check("Tab from the search reaches the kinds, each a checkbox", box?.role === "checkbox" && box.name === gate && box.checked === "true", named(box));
    await page.press(" ");
    await until(`(${counted}) === "6"`);
    const unchecked = await find(page, "checkbox", gate);
    check("Space hides the kind: its box unchecked, 6 moments of 7 counted", unchecked?.checked === "false" && (await page.evaluate(counted)) === "6", `${unchecked?.checked} · ${await page.evaluate(counted)}`);
    await page.evaluate(`document.querySelector(".stage canvas").focus(), true`);
    await rawKey("Home", "Home", 36);
    await until(`(${status}) === "01JCM100000000000000000000"`);
    const walked = [await page.evaluate(status)];
    for (let i = 0; i < 7; i++) {
      await page.press(".");
      await sleep(40);
      walked.push(await page.evaluate(status));
    }
    const kept = [...new Set(walked)];
    check("`.` walks the moments the filters keep, never the hidden gate", kept.length === 6 && !kept.includes("01JCM5A0000000000000000000"), kept.map((id) => id.slice(5, 7)).join(" "));
    await focus(page, await find(page, "radio", moviola["moviola-filter-basic"]));
    await page.press("ArrowRight");
    await until(`document.querySelector(".moviola-filters")?.dataset.mode === "advanced"`);
    const lanes = (await all(page, "combobox")).find((node) => node.name.startsWith(moviola["moviola-filter-lane"]));
    if (lanes) await focus(page, lanes);
    await page.press("ArrowDown");
    await until(`(${counted}) === "3"`);
    check("the arrow keys choose the advanced mode, and a lane from its select", (await page.evaluate(counted)) === "3", `${named(lanes)} · ${await page.evaluate(counted)}`);
    await focus(page, await find(page, "radio", moviola["moviola-filter-advanced"]));
    await page.press("ArrowLeft");
    await until(`(${counted}) === "6"`);
    const selects = (await all(page, "combobox")).filter((node) => node.name.startsWith(moviola["moviola-filter-lane"])).length;
    check("back to basic by the arrow keys lets the lane go, the kind stays hidden", (await page.evaluate(counted)) === "6" && selects === 0, `${await page.evaluate(counted)} · ${selects} lane selects`);
    await page.evaluate(`document.getElementById("search").focus(), true`);
    await page.press("Tab");
    const clearer = await focused(page);
    await page.press("Enter");
    await until(`(${counted}) === null`);
    check("«Clear the filters» from the keyboard shows every kind again", clearer?.name === moviola["moviola-filter-clear"] && (await find(page, "checkbox", gate))?.checked === "true", named(clearer));
    await page.evaluate(`document.querySelector(".scrub").focus(), true`);
    await rawKey("Home", "Home", 36);
    await until(`document.querySelector(".at-date")?.dataset.before === "1"`);
    const panel = await find(page, "region", moviola["moviola-at"]);
    const said = await page.evaluate(`(({ before, after, last }) => [before, after, last])(document.querySelector(".at-date").dataset)`);
    check("Home on the range: the panel at the playhead, 1 before and 6 after, the first moment the last", panel !== undefined && JSON.stringify(said) === '["1","6","01JCM100000000000000000000"]', `${named(panel)} · ${said}`);
    let jump = null;
    for (let i = 0; i < 8 && !jump?.name.includes("First synthetic step"); i++) {
      await page.press("Tab");
      jump = await focused(page);
    }
    await page.press("Enter");
    await until(`(${frameMoment}) === "01JCM100000000000000000000"`);
    check("Tab and Enter on the panel's last moment open it", jump?.role === "button" && (await page.evaluate(frameMoment)) === "01JCM100000000000000000000", named(jump));
    // The portfolio has no search: / finds nothing there.
    await fresh(page, harness, "#/moviola?data=demo&lang=en");
    await page.press("/");
    await sleep(100);
    const nowhere = (await focused(page))?.role !== "searchbox";

    // The control: a page with none of it.
    await fresh(page, harness, "#/control?data=planted&lang=en");
    const control = [];
    if ((await go(page, "s")).hash === "#/settings") control.push("g s opened the settings");
    if ((await help(page)).dialog) control.push("? opened a help");
    const first = (await tabs(page, 1))[0];
    if (named(first.node) === 'link "Skip to content"') control.push("Tab found a skip link");
    const caught = control.length === 0 && missing(PROTOTYPE, "en").length > 0 && !prototypeQuota && nowhere;

    for (const [name, ok, detail] of results) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
    const failed = results.filter(([, ok]) => !ok).length;
    console.log(`keyboard checks: ${results.length}; failed: ${failed}`);
    console.log(`negative control, the control page, the prototype's glossary and its "claude 6 %": ${caught ? "caught" : "NOT CAUGHT"} (prototype misses ${JSON.stringify(missing(PROTOTYPE, "en"))}${control.length ? `; passed: ${control.join(", ")}` : ""})`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return failed === 0 && caught && newErrors.length === 0;
  },
};
