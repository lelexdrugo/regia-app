import "./app.css";
// First, so that its listeners see every error and CSP violation of a smoke run.
import { runSmoke } from "./lib/smoke";

import { mount } from "svelte";

import App from "./App.svelte";
import { catalog } from "./lib/i18n";
import { getAppInfo, getBrief } from "./lib/ipc";

async function start(): Promise<void> {
  const info = await getAppInfo();
  const strings = catalog(info.language);
  document.documentElement.lang = strings.language;
  document.title = strings.message("app-title");
  const target = document.getElementById("root");
  if (!target) {
    throw new Error("index.html has no #root");
  }
  mount(App, {
    target,
    props: {
      strings,
      load: getBrief,
      onsettled: () => {
        if (info.smoke) {
          void runSmoke(strings.language);
        }
      },
    },
  });
}

void start();
