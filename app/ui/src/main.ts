import "./tokens.css";
import "./app.css";
// First, so that its listeners see every error and CSP violation of a smoke run.
import { runSmoke } from "./lib/smoke";

import { mount } from "svelte";

import { catalog } from "./lib/i18n";
import { getAppInfo, getBrief } from "./lib/ipc";
import App from "./App.svelte";

async function start(): Promise<void> {
  const info = await getAppInfo();
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
      product: info.product_name,
      load: getBrief,
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
