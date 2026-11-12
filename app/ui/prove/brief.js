// The brief (#476): what the queue shows of the core's brief, read on the page against harness/brief.json. The order
// is the fixture's, group by group, as no field sorts it; the due marks and the measures are the core's; each
// warning of the controller shows its own cause (#439/D16); a section this version does not read says so; every
// block says its state; `data-brief` marks the first brief on screen; Retry reads again; hostile text stays text.
// The negative controls: the same fixture sorted by number, as a view that orders would show it; the prototype's
// stopped line with its one cause; the brief without `not_ported`, as a view that shows an absence; the control
// page, which has no brief at all.
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { format, messages } from "./messages.js";
import { open, show } from "./views.js";

/** The page as the checks read it: each row with its list and place, each group, each warning, the marker. */
const READ = `(() => {
  const text = (node) => (node?.textContent ?? "").replace(/\\s+/g, " ").trim();
  const rows = [...document.querySelectorAll("li[data-section]")].map((row) => ({
    section: row.dataset.section,
    number: Number(row.dataset.number),
    rank: Number(row.dataset.rank),
    text: text(row),
  }));
  const groups = [...document.querySelectorAll("section.qgroup")].map((group) => ({
    heading: text(group.querySelector("h2")),
    rows: [...group.querySelectorAll("li[data-section]")].map((row) => Number(row.dataset.number)),
    skeleton: group.querySelector(".skeleton") !== null,
    text: text(group),
  }));
  return {
    marker: document.querySelector("[data-brief]")?.dataset.brief ?? null,
    rows,
    groups,
    alerts: [...document.querySelectorAll(".alert")].map(text),
    raw: [...document.querySelectorAll("[data-raw]")].map(text),
    status: text(document.querySelector("[role=status]")),
    alert: text(document.querySelector("[role=alert]")),
    retry: [...document.querySelectorAll("button")].map(text),
    pwned: window.__pwned ?? null,
    markup: document.querySelectorAll("#root img, #root script, #root svg, #root iframe").length,
    all: document.getElementById("root")?.innerText ?? "",
  };
})()`;

/** The variants of a select message of the brief's catalog, by key: `brief-not-ported` has one per section. */
function variants(ui, language, id) {
  const source = readFileSync(join(ui, "../../crates/regia-i18n/locales", language, "brief.ftl"), "utf8");
  const block = source.split(`${id} = `)[1]?.split(/\n(?=\S)/)[0] ?? "";
  return Object.fromEntries([...block.matchAll(/\*?\[(\w+)\] (.+)/g)].map((match) => [match[1], match[2].trim()]));
}

/** The day of the app's due marks, as lib/brief.ts writes it. */
function day(value, language) {
  const [year, month, date] = value.split("-").map(Number);
  return new Intl.DateTimeFormat(language, { day: "numeric", month: "short", timeZone: "UTC" }).format(new Date(Date.UTC(year, month - 1, date, 12)));
}

function numbers(tasks) {
  return tasks.map((task) => task.number);
}

/** The core's lists, back from the rows' places, and each group in order: the fixture's, untouched. */
function order(page, brief) {
  const problems = [];
  for (const section of ["waiting_on_owner", "ready", "in_progress"]) {
    const placed = page.rows.filter((row) => row.section === section).sort((a, b) => a.rank - b.rank).map((row) => row.number);
    if (JSON.stringify(placed) !== JSON.stringify(numbers(brief[section]))) problems.push(`${section}: ${placed} for ${numbers(brief[section])}`);
  }
  const session = new Set(numbers(brief.session));
  const expected = [
    numbers(brief.waiting_on_owner).filter((number) => !session.has(number)),
    numbers(brief.session),
    numbers(brief.ready),
    numbers(brief.in_progress),
  ].filter((list) => list.length > 0);
  const shown = page.groups.map((group) => group.rows).filter((list) => list.length > 0);
  if (JSON.stringify(shown) !== JSON.stringify(expected)) problems.push(`groups ${JSON.stringify(shown)} for ${JSON.stringify(expected)}`);
  return problems;
}

/** Every warning of the controller shows the cause the fixture gives it, in its own line. */
function causes(page, brief) {
  const controller = brief.controller;
  const reasons = [controller?.budget?.reason, ...(controller?.to_open ?? []).map((x) => x.reason), ...(controller?.to_resume ?? []).map((x) => x.reason)];
  return reasons.filter(Boolean).filter((reason) => !page.alerts.some((alert) => alert.includes(reason))).map((reason) => `no line with its cause: ${reason.slice(0, 50)}`);
}

/** A note in the place of each section the fixture names as not read, and none for the others. */
function unported(page, brief, notes) {
  const problems = [];
  for (const [section, note] of Object.entries(notes)) {
    const named = brief.not_ported.includes(section === "other" ? "outside_tracker" : section);
    if (named !== page.all.includes(note)) problems.push(`${section}: ${named ? "not said" : "said, but it is read"}`);
  }
  return problems;
}

export const brief = {
  browser: true,
  async run({ ui, page, harness }) {
    const fixtures = JSON.parse(readFileSync(join(ui, "harness/brief.json"), "utf8"));
    const errors = page.consoleErrors.length;
    const results = [];
    const check = (name, problems) => results.push([name, problems]);
    const read = async (route) => {
      await show(page, route);
      return page.evaluate(READ);
    };
    await open(page, harness, "#/brief?data=demo&lang=en");

    // The order: the fixtures' lists are in an order no field gives (the worst case puts 2147483647 second).
    for (const data of ["demo", "worst", "one"]) {
      check(`${data}: the core's order, group by group`, order(await read(`#/brief?data=${data}&lang=en`), fixtures[data]));
    }

    // The due marks and the measures, as the core gave them, in both languages.
    for (const language of ["en", "it"]) {
      const words = messages(ui, language);
      const rows = Object.fromEntries((await read(`#/brief?data=demo&lang=${language}`)).rows.map((row) => [row.number, row.text]));
      const wanted = [
        [101, format(words["queue-due"], { day: day("2026-11-13", language) })],
        [102, format(words["queue-overdue"], { day: day("2026-11-09", language) })],
        [121, format(words["queue-due"], { day: day("2026-11-12", language) })],
        [101, `${format(words["queue-unlocks-total"], { n: 3, total: 5 })} · ${format(words["queue-chain"], { n: 4 })}`],
        [102, `${format(words["queue-unlocks"], { n: 1 })} · ${format(words["queue-chain"], { n: 2 })}`],
        [103, `${format(words["queue-unlocks-total"], { n: 0, total: 2 })} · ${format(words["queue-chain"], { n: 2 })}`],
        [131, words["queue-cycle"]],
      ];
      const problems = wanted.filter(([number, text]) => !rows[number]?.includes(text)).map(([number, text]) => `#${number} lacks "${text}"`);
      // A date far away is not marked, and a task that unblocks nothing has no measure, as in the CLI's text.
      const far = format(words["queue-due"], { day: day("2026-12-20", language) });
      if (rows[104]?.includes(far)) problems.push("#104 is marked, and its date is far");
      if (rows[112]?.includes(format(words["queue-unlocks"], { n: 0 }))) problems.push("#112 has a measure, and unblocks nothing");
      check(`demo, ${language}: the due marks and the measures`, problems);
    }

    // #439/D16: the cause of each warning is its own.
    const demoPage = await read("#/brief?data=demo&lang=en");
    const worstPage = await read("#/brief?data=worst&lang=en");
    check("demo: each warning of the controller with its own cause", causes(demoPage, fixtures.demo));
    check("worst: each warning of the controller with its own cause", causes(worstPage, fixtures.worst));

    // Sections this version does not read: said in their place, from not_ported.
    const notes = variants(ui, "en", "brief-not-ported");
    for (const data of ["one", "empty", "demo"]) {
      check(`${data}: not_ported said in place`, unported(await read(`#/brief?data=${data}&lang=en`), fixtures[data], notes));
    }

    // Every block in every state, and the marker of the first brief.
    const en = messages(ui, "en");
    const loading = await read("#/brief?data=loading&lang=en");
    check("loading: marked loading, said, and a skeleton in each block", [
      ...(loading.marker === "loading" ? [] : [`marker ${loading.marker}`]),
      ...(loading.status === en["brief-loading"] ? [] : [`status "${loading.status}"`]),
      ...(loading.groups.filter((group) => group.skeleton).length === 3 ? [] : ["not three skeletons"]),
    ]);
    const failed = await read("#/brief?data=error&lang=en");
    check("error: marked failed, the reason as text of its own, Retry, and each block not read", [
      ...(failed.marker === "failed" ? [] : [`marker ${failed.marker}`]),
      ...(failed.alert.startsWith(en["brief-error-lead"]) && failed.raw.includes(fixtures.error) ? [] : [`alert "${failed.alert.slice(0, 60)}"`]),
      ...(failed.retry.includes(en["error-retry"]) ? [] : ["no Retry"]),
      ...(failed.groups.filter((group) => group.text.includes(en["brief-not-read"])).length === 3 ? [] : ["not three blocks not read"]),
    ]);
    const empty = await read("#/brief?data=empty&lang=en");
    check("empty: marked ready, and each block says it is empty", [
      ...(empty.marker === "ready" ? [] : [`marker ${empty.marker}`]),
      ...["brief-empty", "brief-ready-empty", "brief-running-empty"].filter((id) => !empty.all.includes(en[id])).map((id) => `no ${id}`),
    ]);
    for (const data of ["demo", "worst", "one", "huge"]) {
      const marker = (await read(`#/brief?data=${data}&lang=en`)).marker;
      check(`${data}: marked ready`, marker === "ready" ? [] : [`marker ${marker}`]);
    }

    // Retry reads again: the marker goes back to loading, then to what the read gave.
    await show(page, "#/brief?data=error&lang=en");
    const markers = await page.evaluate(`new Promise((resolve) => {
      const screen = document.querySelector("[data-brief]");
      const seen = [];
      // Each change's value before it, then the last one: the observer may hear both changes at once.
      const observer = new MutationObserver((records) => seen.push(...records.map((record) => record.oldValue)));
      observer.observe(screen, { attributeFilter: ["data-brief"], attributeOldValue: true });
      [...document.querySelectorAll("button")].find((button) => button.textContent.trim() === ${JSON.stringify(en["error-retry"])})?.click();
      setTimeout(() => resolve([...seen, screen.dataset.brief]), 300);
    })`);
    const sequence = JSON.stringify(markers);
    check("Retry reads again", sequence === JSON.stringify(["failed", "loading", "failed"]) ? [] : [`markers ${sequence}`]);

    // Hostile text: titles, projects, reasons and folders stay text.
    const hostile = fixtures.worst.waiting_on_owner[2].title;
    check("worst: hostile text stays text", [
      ...(worstPage.pwned === null ? [] : ["a script ran"]),
      ...(worstPage.markup === 0 ? [] : [`${worstPage.markup} elements from markup`]),
      ...(worstPage.raw.includes(hostile) ? [] : ["the hostile title is not shown as text"]),
    ]);

    // The negative controls, each with its check.
    const controls = [
      ["a view that orders by number", order(await read("#/brief?data=worst&lang=en&sort=number"), fixtures.worst)],
      ["the prototype's stopped line, one cause for all", causes(await read("#/stopped?data=worst&lang=en"), fixtures.worst)],
      ["a view that shows an absence", unported(await read("#/brief?data=one&lang=en&ported=all"), fixtures.one, notes)],
      ["the control page: no marker", (await read("#/control?data=planted&lang=en")).marker === null ? ["no marker"] : []],
    ];

    let failures = 0;
    for (const [name, problems] of results) {
      console.log(`${problems.length === 0 ? "ok  " : "FAIL"} ${name}${problems.length ? `: ${problems.slice(0, 3).join("; ")}` : ""}`);
      failures += problems.length === 0 ? 0 : 1;
    }
    console.log(`brief checks: ${results.length}; failed: ${failures}`);
    const missed = controls.filter(([, problems]) => problems.length === 0).map(([name]) => name);
    console.log(`negative controls, ${controls.length}: ${missed.length === 0 ? "all caught" : `missed ${JSON.stringify(missed)}`}`);
    for (const [name, problems] of controls) console.log(`  ${name}: ${problems.slice(0, 2).join("; ")}`);
    const newErrors = page.consoleErrors.slice(errors);
    console.log(`errors in the console: ${newErrors.length}`);
    for (const error of [...new Set(newErrors)].slice(0, 10)) console.log(`  ${error}`);
    return failures === 0 && missed.length === 0 && newErrors.length === 0;
  },
};
