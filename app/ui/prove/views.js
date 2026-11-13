// The views the app has, each in every state of its fixtures (harness/main.ts): what the catalog and limits
// proofs render. A new view adds its routes here.
export const VIEWS = [
  ...["demo", "worst", "empty", "one", "huge", "loading", "error"].map((data) => `#/brief?data=${data}`),
  ...["daily", "moviola", "console", "settings"].map((section) => `#/${section}?data=demo`),
  // The moviola: the portfolio in each state, then each project's lanes, one with no milestone, one unknown.
  ...["empty", "error", "loading"].map((data) => `#/moviola?data=${data}`),
  ...["alpha", "beta", "gamma", "viewer", "nowhere"].map((project) => `#/moviola/${project}?data=demo`),
  // The comfortable density is larger in every measure, and the setup may choose it (#483).
  "#/brief?data=worst&density=comfortable",
  "#/settings?data=demo&density=comfortable&dir=nero",
  // The help, open over a screen: the keys and the words.
  "#/settings?data=demo&help=1",
  // The sidebar's quota on a fixture (#439/D29), under and over the 75 % where nothing starts by itself.
  "#/brief?data=demo&quota=6",
  "#/console?data=demo&quota=82",
  ...["demo", "worst", "empty", "one", "huge"].map((data) => `#/document?data=${data}`),
  "#/corpus?data=hostile",
];

/** Opens `route` with a fresh page load, and waits until the harness has drawn it. */
export async function open(page, harness, route) {
  await page.goto(`${harness}${route}`);
  await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(route)}`);
}

/** Moves the loaded harness to `route`, and waits until it has drawn it. */
export async function show(page, route) {
  if ((await page.evaluate("location.hash")) === route) {
    return;
  }
  await page.evaluate(`location.hash = ${JSON.stringify(route)}`);
  await page.waitFor(`document.documentElement.dataset.settled === ${JSON.stringify(route)}`);
}
