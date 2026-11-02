// Everything from the tracker reaches the DOM through textContent, never as markup: titles are
// written by people and by agents, and are untrusted.
const { invoke } = window.__TAURI__.core;

const root = document.getElementById("root");

function element(tag, text) {
  const node = document.createElement(tag);
  node.textContent = text;
  return node;
}

function section(strings, headingId, key, tasks) {
  const block = document.createElement("section");
  block.append(element("h2", strings[headingId]));
  if (tasks.length === 0) {
    block.append(element("p", strings["brief-nothing"]));
    return block;
  }
  const list = document.createElement("ul");
  for (const task of tasks) {
    const item = element("li", `#${task.number} ${task.title}`);
    item.dataset.section = key;
    item.dataset.number = String(task.number);
    list.append(item);
  }
  block.append(list);
  return block;
}

function graphWarnings(strings, graph) {
  if (!graph) {
    return [];
  }
  const list = (numbers) => numbers.map((number) => `#${number}`).join(", ");
  const warnings = graph.cycles.map((cycle) => strings["brief-cycle"].replace("{reason}", list(cycle)));
  if (graph.truncated.length > 0) {
    warnings.push(strings["brief-truncated"].replace("{reason}", list(graph.truncated)));
  }
  return warnings.map((text) => element("p", text));
}

function report(language) {
  const sections = {};
  for (const item of document.querySelectorAll("li[data-section]")) {
    (sections[item.dataset.section] ??= []).push(Number(item.dataset.number));
  }
  for (const key of ["waiting_on_owner", "ready", "in_progress"]) {
    sections[key] ??= [];
  }
  return {
    language,
    sections,
    headings: [...document.querySelectorAll("h1, h2")].map((node) => node.textContent),
    images: document.querySelectorAll("img").length,
    scripts_inline: document.querySelectorAll("script:not([src])").length,
    text: document.body.innerText,
  };
}

async function start() {
  const { language, strings, smoke } = await invoke("bootstrap");
  document.documentElement.lang = language;
  document.title = strings["app-title"];
  root.replaceChildren(element("h1", strings["app-title"]), element("p", strings["brief-loading"]));
  try {
    const brief = await invoke("get_brief");
    root.replaceChildren(
      element("h1", strings["app-title"]),
      element("p", strings["brief-partial"]),
      ...graphWarnings(strings, brief.graph),
      section(strings, "brief-waiting", "waiting_on_owner", brief.waiting_on_owner),
      section(strings, "brief-ready", "ready", brief.ready),
      section(strings, "brief-in-progress", "in_progress", brief.in_progress),
    );
  } catch (error) {
    root.replaceChildren(
      element("h1", strings["app-title"]),
      element("p", strings["brief-error"].replace("{reason}", String(error))),
    );
  }
  if (smoke) {
    await invoke("smoke_report", { report: report(language) });
  }
}

start();
