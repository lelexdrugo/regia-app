# The app's frontend

Svelte 5, TypeScript and Vite, as a single-page app without SvelteKit (ADR 0001 §13). It runs in the app's webview, under the CSP and the capability of `app/src-tauri`, and talks to the core only through the app's commands.

## Build it before Cargo

`tauri::generate_context!` embeds `dist/` when the app crate compiles, so every `cargo build` or `cargo test` of the workspace needs it. Node 22 or later:

```sh
cd app/ui
npm ci --ignore-scripts
npm run build      # writes dist/
npm run check      # types and Svelte's compile-time checks, warnings included
npm run prove      # the harness and its proofs, below
```

Without `dist/index.html` the app's `build.rs` stops with this instruction. After changing the frontend, `npm run build` again: the next `cargo build` embeds the new files. There is no development server: the CSP is enforced only on the embedded files, so they are what the tests run on (threat model §3.5).

Dependencies install no scripts (`.npmrc`, threat model V23). A new dependency is a choice to justify, with its licence: the webview runs it with the app's permissions. The harness and the proofs add none.

## Text from others

Titles, documents, comments and transcripts are written by people and by agents, and arrive raw from the tracker. They never pass through the catalog, and they reach the DOM only as text:

- the core normalizes them and parses markdown into a closed tree (`regia-core`'s `text`, `markdown` and `link` modules); `src/lib/content/` renders the tree, element by element, with no attribute from the content, and a link is a button that calls back with the URL the core checked (threat model §3.7);
- an element that holds such text carries `data-raw`, so the catalog proof can tell it from the catalog's strings;
- V08: no line of `src/` or `index.html` may name `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `DOMParser`, `eval`, `Function`, a timer given a string, `{@html}` or the newer APIs that parse HTML, unless it gives its reason on the same line (`V08: <reason>`). `app/src-tauri/tests/security.rs` checks it in every `cargo test`; the one use is `smoke.ts`'s probe of the CSP.

## The harness and the proofs

`npm run prove` builds the harness and runs seven proofs in one headless Chrome. Each checks the app, then a negative control of its own, and fails if the app fails or if the control passes. They run in CI, in the Linux job, with the runner's Chrome.

- The **harness** (`harness/`, built by `vite.harness.config.ts` into the workspace's `target/ui-harness`, never into `dist/`) mounts the app's own components on fixtures: `#/<view>?data=<state>&lang=<it|en|xx>`. `prove/serve.js` serves it with `tauri.conf.json`'s policy, with `connect-src 'none'` for the IPC it does not have. `xx` is a pseudo-locale (`harness/pseudo.ts`): each message wrapped in ⟦ ⟧ and 40% longer.
- **contrast** (`prove/contrast.js`): first, `src/tokens.css` is still the dossier's file, by its SHA-256; then the 30 pairs of the UX prototype, in the design system's three directions. Controls: one digit changed in the copy, one token made fainter.
- **catalog** (`prove/catalog.js`): every view and state in `xx`, with no text outside the catalog or `data-raw`. Control: a string written in the code. It also counts, as information, the messages no source names yet.
- **limits** (`prove/limits.js`): every view and state, in it, en and xx, at 1440×900, 1024×700 and 720×450, against content outside the window, cut controls, text squeezed into a column, raw values, parameters left in braces and markup that became elements. Control: a page with three of those.
- **v07** (`prove/v07.js`): the core's hostile corpus through the app's renderer, judged by `src/lib/v07.ts`, which `app-smoke` also runs on each engine; the browser and the server witness every request. Control: the same corpus through a deliberately unsafe renderer.
- **language** (`prove/language.js`): on the brief of 1,284 tasks scrolled far down, the arrow keys change the sidebar's language and back, and the page keeps its place: no reload or navigation, the same address, the same rows where they were on screen, the focus on the language chosen, the words of the other language, the choice saved. Control: the change made as a reload.
- **keyboard** (`prove/keyboard.js`): the shell's keys, pressed as a keyboard sends them and read in Chrome's accessibility tree, by role and name: `g` and a letter, `?` and Esc with the focus coming back, the help's words (#439/D31) in both languages, the Tab order from the skip link with a ring on every stop, the settings with the arrow keys, `prefers-reduced-motion`, the sidebar's quota in words (#439/D29). Controls: the control page, which has none of it, and the prototype's glossary and quota.
- **brief** (`prove/brief.js`): the brief against `harness/brief.json`. Each group in the fixture's order, which no field gives, and the core's lists back from the rows' places; the due marks and the measures in both languages, a far date and a task that unblocks nothing left unmarked, as in the CLI's text; each warning of the controller with its own cause (#439/D16); a note in the place of each section `not_ported` names, and none for the others; every block's state; the marker; Retry; hostile text as text. Controls: the fixture sorted by number (`sort=number`, a view that orders), the prototype's stopped line with its one cause (`#/stopped`), the brief without `not_ported` (`ported=all`, a view that shows an absence), and the control page, with no marker.

`CHROME_PATH` picks the browser, which runs with `--disable-ipc-flooding-protection`: the proofs move the harness more than 200 times in 10 s, and Chrome would then ignore the address without a word; otherwise `prove/cdp.js` looks where runners and desktops keep Chrome or Chromium, and tries a desktop Edge last, since an Edge that is already open can take the launch over. `node prove/index.js limits` runs one proof.

The fixtures are synthetic. `harness/brief.json` is written by hand in the shape of `ipc.rs`'s `BriefView`, within the core's caps (200 characters, one line): `demo` and `worst` fill every block, the controller's, the dossiers' and the outside-tracker's included, which the core does not read yet; `one` and `empty` name them in `not_ported`, as the app's brief does today; `done` holds what the app decided today, which only #477 will record. The `huge` brief and the `huge` document are built from it in `harness/main.ts`. In `harness/documents.json` the markdown is written by hand and the trees are the core's, as in `harness/corpus.json`, which comes from the core's corpus. After changing either, or the core's parser: `cargo test -p regia-app -- --ignored write_harness`. A test fails while they are stale.

A new view adds its routes to `harness/main.ts` and `prove/views.js`. The sections are the whole shell on one fixture (`data`, `lang`, `dir`, `density`, `save=fail`, `help=1`, `quota=<used>`, and for the brief proof's controls `sort=number` and `ported=all`): the shell's own moves keep it mounted, and only the current mount may say it has drawn.

## Where things are

| Path | What it is |
|---|---|
| `src/lib/bindings.ts` | The IPC types, generated by ts-rs from `app/src-tauri/src/ipc.rs`. After changing a type there: `cargo test -p regia-app -- --ignored write_bindings` |
| `src/lib/ipc.ts` | One hand-written wrapper per app command |
| `src/lib/i18n.ts` | Fluent, over the `.ftl` files of `crates/regia-i18n/locales/`, read at build time: the CSP admits no fetch |
| `src/lib/content/` | The renderer of the core's markdown tree |
| `src/lib/smoke.ts`, `src/lib/v07.ts` | Test only: what `app-smoke` checks on each engine (rendering, V07 on the DOM, CSP, navigation, ACL, CSS features) |
| `src/tokens.css`, `src/fonts/` | The design system's tokens, the dossier's `docs/app/design-system/tokens.css` at 38c87dc unchanged under a header, and the faces it names with their licences. To realign: copy the file again and update the hash in `prove/contrast.js` |
| `src/App.svelte` | The shell: the skip link, the sidebar (`src/shell/`), the section the address names (`src/lib/route.ts`), the keys and the help (`src/lib/glossary.ts`) |
| `src/screens/` | The screens this version has: the brief, the settings; the other sections say their screen comes later |
| `src/screens/Brief.svelte`, `src/screens/brief/`, `src/lib/brief.ts` | The brief: the queue's groups in the core's order (the view sorts nothing: `lib/brief.ts` only splits the tasks that need a session from the rest, as the CLI's text does), each block's state, the controller's warnings with their own causes. `data-brief` on its screen is `loading` until the core's answer is in the DOM, then `ready` or `failed`: the marker of the first brief on screen, for the dossier's T2 milestone. Each row carries `data-section`, `data-number` and `data-rank`, its list and place in the core's brief, which `app-smoke` reads back (`lib/smoke.ts`) |
| `src/lib/look.ts` | The direction and the density on `<html>`. The app serves `index.html` with the saved ones already there (`app/src-tauri/src/settings.rs`, the file `interface.toml` in the configuration folder), so the first paint is in them |
| `src/icons/` | The sidebar's icons and the wordmark, used as CSS masks: the page holds no SVG |
| `harness/`, `prove/` | The harness and the proofs, above |

Every language's catalog is in the bundle: about 25 kB of `.ftl` each, 8 kB gzipped. Importing only the active one would save that much, for an extra chunk to load before the first paint and another whenever the language changes. With a third language it becomes worth it.
