// The frame of a moment in Chrome, on the harness (#487): the moment the address opens, and the one the stage
// chooses. The summary first when the sidecar has one, and the frame as before when it has none (Acceptance 3);
// a `#N` of each of the three sections that link them points at the issue the prototype points at, and a text
// with `<` stays text (Acceptance 3); for a moment with reviews, the checks, the defects found and closed and the
// models are on screen without opening anything, and for a task with no session and no tokens, the colleague
// without sync, the frame says the data is not here (Acceptance 1); «Then» and the steps choose another moment
// on the stage, the close button lets it go. The negative controls: the links' check on a frame whose notes were
// refused, and the summary's check on a moment with none, must both fail.
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { open, show } from "./views.js";

const FRAME = "aside.frame";
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** The frame as the DOM holds it: the moment, its state, its sections by heading, and what is visible. */
const READ = `(() => {
  const frame = document.querySelector(${JSON.stringify(FRAME)});
  if (!frame) return null;
  const sections = [...frame.querySelectorAll("section")].map((section) => ({
    heading: section.querySelector("h3")?.textContent.trim() ?? null,
    brief: section.classList.contains("frame-brief"),
    text: section.textContent,
    links: [...section.querySelectorAll("button.link[data-url]")].map((b) => ({ text: b.textContent.trim(), url: b.dataset.url })),
  }));
  const visible = (selector) => [...frame.querySelectorAll(selector)].filter((e) => {
    const box = e.getBoundingClientRect();
    return box.width > 0 && box.height > 0 && getComputedStyle(e).visibility !== "hidden";
  }).map((e) => e.textContent.trim());
  return {
    moment: frame.dataset.moment ?? null,
    notes: frame.dataset.frame ?? null,
    sections,
    elements: frame.querySelectorAll("script, img, iframe, svg, style, b, form").length,
    checks: visible("[data-check]"), models: visible("[data-model]"), defects: visible("[data-defects]"),
    absent: visible("[data-absent]"), status: document.querySelector("#moviola-status")?.dataset.selected ?? null,
  };
})()`;

const read = (page) => page.evaluate(READ);

async function waitFrame(page, moment) {
  await page.waitFor(`document.querySelector(${JSON.stringify(FRAME)})?.dataset.moment === ${JSON.stringify(moment)}
    && document.querySelector(${JSON.stringify(FRAME)})?.dataset.frame !== "loading"`, 5_000);
  return read(page);
}

/** Presses the first link of the section headed `heading` with text `text`, and reads the address the frame shows. */
const press = (page, heading, text) =>
  page.evaluate(`(() => {
    const section = [...document.querySelectorAll(${JSON.stringify(FRAME)} + " section")].find((s) => s.querySelector("h3")?.textContent.trim() === ${JSON.stringify(heading)});
    const button = section && [...section.querySelectorAll("button.link[data-url]")].find((b) => b.textContent.trim() === ${JSON.stringify(text)});
    if (!button) return null;
    button.click();
    return new Promise((resolve) => requestAnimationFrame(() => resolve(document.querySelector(".frame-address")?.textContent.trim() ?? null)));
  })()`);

const click = (page, selector) => page.evaluate(`document.querySelector(${JSON.stringify(selector)})?.click(), true`);

export const frame = {
  browser: true,
  async run({ ui, page, harness }) {
    const fixture = JSON.parse(readFileSync(join(ui, "harness/moviola.json"), "utf8"));
    const recorded = (name, id) => fixture.cases.find((c) => c.name === name).frames.find((f) => f.id === id);
    const en = Object.fromEntries([...readFileSync(join(ui, "../../crates/regia-i18n/locales/en/moviola.ftl"), "utf8").matchAll(/^([a-z][\w-]*) = (.*)$/gm)].map((m) => [m[1], m[2]]));
    const errors = page.consoleErrors.length;
    const checks = [];
    const check = (name, ok, detail = "") => checks.push([name, ok, detail]);

    // The moment the address opens: the summary first, then the why.
    const first = "01JCF000000000000000000000";
    await open(page, harness, `#/moviola/frame/${first}?data=demo&lang=en`);
    const f0 = await waitFrame(page, first);
    const order = f0.sections.map((s) => (s.brief ? "brief" : s.heading));
    check("the address opens its moment, chosen on the stage too", f0.moment === first && f0.status === first, `${f0.moment} ${f0.status}`);
    check("in brief first, above the why", order[0] === "brief" && order[1] === en["moviola-why"], order.slice(0, 3).join(" · "));
    check("the summary is the sidecar's", f0.sections[0].text.includes(recorded("frame", first).inBrief));

    // A #N of each of the three sections opens the prototype's issue.
    const expected = recorded("frame", first);
    const sections = [["moviola-changed", "changed"], ["moviola-open-decisions", "openDecisions"], ["moviola-risks", "risks"]];
    const linksOk = async (frameData) => {
      const results = [];
      for (const [id, key] of sections) {
        const want = expected[key].flat().filter((r) => r.url);
        const shown = frameData.sections.find((s) => s.heading === en[id])?.links ?? [];
        const sameLinks = JSON.stringify(shown.map((l) => [l.text, l.url])) === JSON.stringify(want.map((r) => [r.text, r.url]));
        const target = want[want.length - 1];
        const address = target ? await press(page, en[id], target.text) : null;
        results.push([en[id], sameLinks && target !== undefined && address === target.url, `${shown.length} links; ${target?.text} → ${address}`]);
      }
      return results;
    };
    for (const [name, ok, detail] of await linksOk(f0)) check(`a #N of «${name}» opens the right issue`, ok, detail);
    const hostile = f0.sections.find((s) => s.heading === en["moviola-changed"]);
    check("a text with < stays text", f0.elements === 0 && hostile.text.includes("<script>alert(1)</script> & a < b"), `${f0.elements} elements`);
    check("#14a is not a link, x#14 is", !f0.sections.some((s) => s.links.some((l) => l.text === "#14a")) && hostile !== undefined);

    // «Copy for chat» (Change 5): the package selected, five lines under 1,500 characters, pointers only.
    const PACK = `(() => { const a = document.querySelector(".frame-pack textarea");
      return a ? { text: a.value, focused: document.activeElement === a, all: a.selectionStart === 0 && a.selectionEnd === a.value.length } : null; })()`;
    await click(page, `${FRAME} .frame-copy`);
    await sleep(100);
    const packed = await page.evaluate(PACK);
    const lines = packed?.text.split("\n") ?? [];
    check("Copy for chat: the package, selected, five lines under 1,500 characters", packed?.focused && packed.all && lines.length === 5 && packed.text.length <= 1500, `${lines.length} lines, ${packed?.text.length} characters`);
    check("… the moment, its summary, its issues and its sidecar's path", lines[0] === `regia · frame · node ${first} (milestone, 2026-10-14)` && lines[1]?.includes(expected.inBrief)
      && lines[2]?.includes("https://github.com/acme/tasks/issues/12") && lines[2]?.includes(`.regia/moviola/snapshot/${first}.json`), lines.slice(0, 3).join(" | ").slice(0, 160));
    const long = "01JCF300000000000000000000";
    await show(page, `#/moviola/frame/${long}?data=demo&lang=en`);
    await waitFrame(page, long);
    await click(page, `${FRAME} .frame-copy`);
    await sleep(100);
    const longPack = await page.evaluate(PACK);
    check("… a long one shortened, its references counted", longPack !== null && longPack.text.length <= 1500 && /… and \d+ more/.test(longPack.text), `${longPack?.text.length} characters`);
    await show(page, `#/moviola/frame/${first}?data=demo&lang=en`);
    await waitFrame(page, first);

    // Reviews, and the task of a colleague without sync.
    const reviewed = "01JCM300000000000000000000";
    await show(page, `#/moviola/alpha/${reviewed}?data=demo&lang=en`);
    const m3 = await waitFrame(page, reviewed);
    check("a moment with reviews shows its checks without opening anything", m3.checks.some((c) => c.includes("revisione-avversaria") && c.includes("D1, D2")), JSON.stringify(m3.checks));
    check("… the defects found and closed", m3.defects.some((d) => /found 2 · closed 1/.test(d)), JSON.stringify(m3.defects));
    check("… and the models", m3.models.length > 0, JSON.stringify(m3.models));
    const bare = "01JCM5B0000000000000000000";
    await show(page, `#/moviola/alpha/${bare}?data=demo&lang=en`);
    const m5 = await waitFrame(page, bare);
    check("no session and no tokens: the frame says the data is not here", m5.absent.length === 2 && m5.absent[0] === en["moviola-no-session"] && m5.absent[1] === en["moviola-no-tokens"], JSON.stringify(m5.absent));

    // A moment with no summary reads as before; one with no notes says so.
    const second = "01JCF100000000000000000000";
    await show(page, `#/moviola/frame/${second}?data=demo&lang=en`);
    const f1 = await waitFrame(page, second);
    check("no summary: the frame starts with the why, as before", f1.sections[0].heading === en["moviola-why"] && !f1.sections.some((s) => s.brief), f1.sections.map((s) => s.heading).join(" · "));
    const third = "01JCF200000000000000000000";
    await show(page, `#/moviola/frame/${third}?data=demo&lang=en`);
    const f2 = await waitFrame(page, third);
    check("no sidecar: the frame says the chronicler wrote no notes", f2.notes === "none");

    // Around the moment: «Then», the steps and the close button move the stage's choice.
    await show(page, `#/moviola/frame/${first}?data=demo&lang=en`);
    await waitFrame(page, first);
    await click(page, `${FRAME} .frame-line .link.jump`);
    const then = await waitFrame(page, second);
    check("«Then» chooses the next moment on the stage", then.moment === second && then.status === second, `${then.moment} ${then.status}`);
    await click(page, `${FRAME} .frame-steps .btn:first-child`);
    const back = await waitFrame(page, first);
    check("the previous step goes back", back.moment === first && back.status === first);
    await click(page, `${FRAME} .frame-close`);
    await sleep(100);
    const closed = await read(page);
    check("the close button lets the moment go", closed?.moment === null && closed?.status === "", JSON.stringify(closed?.status));

    // Negative controls.
    await show(page, `#/moviola/frame/${second}?data=demo&notes=fail&lang=en`);
    const refused = await waitFrame(page, second);
    const caughtLinks = refused.notes === "failed" && (await linksOk(refused)).some(([, ok]) => !ok);
    const caughtBrief = !(f1.sections[0]?.brief === true);

    for (const [name, ok, detail] of checks) console.log(`${ok ? "ok " : "NO "} ${name}${detail ? `  → ${detail}` : ""}`);
    console.log(`negative control, the links' check on refused notes: ${caughtLinks ? "caught" : "NOT CAUGHT"}`);
    console.log(`negative control, the summary's check on a moment without one: ${caughtBrief ? "caught" : "NOT CAUGHT"}`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return checks.every(([, ok]) => ok) && caughtLinks && caughtBrief && newErrors.length === 0;
  },
};
