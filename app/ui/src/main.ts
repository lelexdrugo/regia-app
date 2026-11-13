import "./tokens.css";
import "./app.css";
// First, so that its listeners see every error and CSP violation of a smoke run, and it reads the page as it
// arrived.
import { restartInNero, runSmoke } from "./lib/smoke";

import { mount } from "svelte";

import App from "./App.svelte";
import { catalog } from "./lib/i18n";
import { getAppInfo, getBrief, getSettings, getTimeline, listProjects, setSettings } from "./lib/ipc";
import { draw } from "./lib/look";

async function start(): Promise<void> {
  const [info, settings] = await Promise.all([getAppInfo(), getSettings()]);
  if (info.smoke && (await restartInNero(settings))) {
    return;
  }
  // index.html arrived drawn in these already (settings.rs); this only confirms them.
  draw(settings);
  const strings = catalog(info.language, info.product_name);
  document.documentElement.lang = strings.language;
  document.title = strings.message("app-title");
  const target = document.getElementById("root");
  if (!target) {
    throw new Error("index.html has no #root");
  }
  let smoked = false;
  mount(App, {
    target,
    props: {
      strings,
      catalogFor: (language) => catalog(language, info.product_name),
      product: info.product_name,
      load: getBrief,
      settings,
      save: setSettings,
      moviola: { projects: listProjects, timeline: (project: string) => getTimeline(project) },
      onsettled: () => {
        // The first screen drawn is the brief: what app-smoke compares with the CLI.
        if (info.smoke && !smoked) {
          smoked = true;
          void runSmoke(strings);
        }
      },
    },
  });
}

void start();
