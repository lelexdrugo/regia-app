# moviola bench

Dev only. It measures the moviola's time per frame, the T5 milestone of the dossier's
`docs/app/piano/traguardi.md`, with the product's renderer inside the product's webview: WebView2 on Windows,
WebKitGTK on Linux, WKWebView on macOS. The renderer is app/ui's Canvas 2D stage (`app/ui/src/lib/moviola`),
imported as it is. This is the renderer spike's bench (commit 84871ea of `spike/moviola-renderer`), moved into the
product. Method and criteria are in the dossier: `docs/app/moviola/metodo.md` and `criteri.md`.

## What you need

- Node 22 or later (24 is what CI uses), with npm.
- Rust from [rustup](https://rustup.rs), stable. The workspace needs at least 1.90.
- A clone of regia-app. It is a private repository, so ask its owner for access.
- **Linux, WSL2 included** (Ubuntu 24.04):

  ```sh
  sudo apt-get update
  sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev \
    libayatana-appindicator3-dev librsvg2-dev mesa-utils xvfb
  ```

- **macOS**: the Xcode command line tools (`xcode-select --install`).
- **Windows**: the WebView2 runtime, which Windows 11 already has, and the Visual Studio C++ build tools that
  rustup asks for.

## Run it

From the root of the clone:

```sh
cd crates/regia-moviola-bench
npm ci
npm run bench
```

The first run compiles the window. That is the workspace's Tauri, so it takes a few minutes; later runs take
seconds. Then a 1280×800 window opens on top of the others and runs for about five minutes. It measures x1, x10,
x100 and x10 dense, three repetitions each, and then the sanity check. Leave the window in view and the machine
idle until the window closes by itself. A hidden or covered window gets no animation frames. The run then stops
with "requestAnimationFrame stalled", and you have to run it again.

The command exits with 0 when the run is complete and the sanity check passes. It exits with 1 when the run
failed or the sanity check failed, 2 on a bad option, and 3 when the run took longer than 40 minutes.

### In WSL

Clone into the Linux home, not under `/mnt/c`. The Windows file system is slow from WSL, and `node_modules` holds
native files for one system only.

```sh
git clone <regia-app's URL> ~/regia-app
cd ~/regia-app && git checkout <the branch you were given>
cd crates/regia-moviola-bench && npm ci && npm run bench
```

With WSLg the window opens on the Windows desktop. The folder and the file are labelled `linux-wsl`. These are
WSL's numbers, not those of a native Linux with a GPU: WSLg rasterises on Mesa's `llvmpipe` unless told otherwise.
To try the GPU through Mesa's d3d12 driver, add `-- --env GALLIUM_DRIVER=d3d12`. Without a display, or to keep
the window off the desktop, add `-- --xvfb` for a virtual display. That is always a software run.

## What to send back

The run's folder is `results/<label>-<time>/`, and the last line of the run names it. The label is the system:
`windows`, `linux`, `linux-wsl` or `macos`, with `-software` added for `--software`. Send back **`results.json`**.
It holds everything:

- the system, the processor and the memory;
- the GPU, as the system names it and as the page's WebGL does;
- WSL and virtual machine flags;
- the webview's and Tauri's versions;
- the commit measured;
- every frame of every phase, and the sanity check.

It holds no user name, host name or path. The timelines are synthetic, with no real title. `summary.md` is the
same run in readable form, and `npm run analyze -- <folder>` rebuilds it from `results.json`. The screenshots in
`shots/` show the text the canvas drew.

## Read the result

`summary.md` has three parts:

1. **The machine:** the system, WSL, virtual machine, the GPU twice, and the engine class.
   - The class is **gpu** or **software**. On Linux the class comes from `glxinfo`, because WebKit answers every
     page "Apple GPU". `llvmpipe` makes the run software.
   - The display's refresh interval is measured before each block. It is the floor of every frame time.
2. **T5:** one row per scale, with median / p95 in ms per phase (pan, zoom, playhead, hover) and over all phases.
   - Also: the frames later than 1.5 refresh intervals, and the script time inside the draw.
   - A row has a threshold only on the engine that T5 names for it:
     - with a GPU: x1, and x100 in each phase, with p95 at most 1.1 intervals; x10 dense, with at most 1 % of
       frames late in pan;
     - software: x10, with median at most 33 ms and p95 at most 50 ms.
   - On a runner, a virtual machine or WSL the verdict is information only. traguardi.md records those numbers
     without a verdict.
3. **The sanity check, at x10:** the same stage with 1.6 refresh intervals of busy work per frame must come out
   slower, in the script time (+75 % of that work) and in the frame time (median ×1.3).
   - **Verdict: PASS** means the numbers measure what they say.
   - **FAIL** means they do not count, whatever they show.
   - The blurred shadow on every fill and stroke is information only. It shows where the canvas rasterises on the
     processor, and a fast GPU hides it.

x10 dense is the lanes and days of x10 with eight times the milestones and aggregates. The product lays out one
column per milestone, so x10 dense makes a longer stage, not a more crowded view at the same zoom. Its 12,400
marks are in view together only near the zoom phase's widest view.

## Options

`npm run bench -- <options>`:

| Option | What it does |
|---|---|
| `--software` | GPU off: `--disable-gpu` for WebView2, `LIBGL_ALWAYS_SOFTWARE=1` on Linux. No effect on macOS |
| `--only main` or `--only sanity` | one block only. The default is both |
| `--scales 1,10,100,10d` | the scales of the main block. `Nd` is N's lanes and days with 8 times the nodes |
| `--reps 3`, `--phase-ms 2500`, `--warmup-ms 1000` | repetitions, length of each of the four phases, warm-up |
| `--repeat 10` | draw each frame N times: the headroom under the refresh interval |
| `--no-shots` | no screenshots (taken at x1 and x100) |
| `--env KEY=VALUE` | more environment for the window, repeatable |
| `--xvfb` | Linux: a virtual display even where there is one |
| `--label name`, `--out folder` | the run's name and folder |
| `--skip-build` | reuse the page and the window already built |
| `--timeout-s 2400` | stop a run that does not end |

On Linux the window gets `WEBKIT_DISABLE_DMABUF_RENDERER=1`, as the spike's runs did, unless the variable is
already set.

`npm run bench:suite` runs the whole set into `results/<system>-<date>/`:

- `bench`, as above;
- `software`, the same with the GPU off;
- `stress`, x300 and x1000;
- `headroom`, x100 drawn 10 times per frame.

`npm test` runs the bench's own tests: the timelines, the verdicts, the server's rules and the machine's
description.

## How it works

- `src/main.rs`, the window. A Tauri binary of the workspace, so it is the app's own Tauri, wry and webview. It
  opens the page that the runner serves on the loopback and ends the run when the page asks. It writes nothing,
  embeds nothing, and has no command and no capability.
- `src/`, the page. `harness.js` and `scenario.js` are the spike's harness and scenario, and drive app/ui's
  `Stage`. `timeline.js` is the spike's seeded generator, in the core's timeline shape. Every phase starts from
  the stage at rest: the playhead on the last milestone, nothing under the pointer.
- `scripts/`, the runner.
  - `run-bench.mjs` builds the page with Vite and the window with cargo. It serves the page under the app's own CSP
    (`serve.mjs`) and receives the results, then adds what the machine is (`machine.mjs`) and writes the summary
    (`analyze.mjs`).
- `.github/workflows/moviola-benchmark.yml` runs the same command on the runners. It runs only when started by
  hand. CI minutes are a budget.
