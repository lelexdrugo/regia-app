# moviola bench

Dev only. It measures the moviola's time per frame, the T5 milestone of the dossier's
`docs/app/piano/traguardi.md`, with the product's renderer inside the product's webview: WebView2 on Windows,
WebKitGTK on Linux, WKWebView on macOS. The renderer is app/ui's Canvas 2D stage (`app/ui/src/lib/moviola`),
imported as it is. This is the renderer spike's bench (commit 84871ea of `spike/moviola-renderer`), moved into the
product. Method and criteria are in the dossier: `docs/app/moviola/metodo.md` and `criteri.md`.

## What you need

- Node 22 or later (24 is what CI uses), with npm.
- Rust from [rustup](https://rustup.rs), stable. The workspace needs at least 1.90.
- A clone of regia-app.
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
   - On Linux two kinds of run are software although nothing names their renderer, and `summary.md` says which:
     - a window in Xvfb, with no display or with `--xvfb`: Xvfb has no GPU;
     - a virtual machine (`systemd-detect-virt` names a hypervisor) where `glxinfo` read nothing, as on GitHub's
       Linux runners, which have no GPU.

     A Linux that is not virtual stays gpu when nothing is read: install `mesa-utils` there.
   - The display's refresh interval is calibrated before each block, on a second of frames that draw nothing. It is
     the floor of every frame time, and the thresholds and the late frames count on it.
   - Windows lowers a dynamic refresh rate while nothing moves, so the calibration can read 60 Hz on a 120 Hz display
     while the stage's frames run at 120. No display hands over a frame sooner than its interval, and jitter puts few
     frames there: about 2 % at most under three quarters of the interval on the macOS runner, the jitteriest measured.
   - So when at least a quarter of a block's frames are shorter than three quarters of the calibrated interval, the
     display ran faster. Its interval is then the median of those frames, and `summary.md` says so ("but 99 % of its
     frames ran at 8.30 ms (120 Hz)"). The interval only ever goes down: a slow renderer cannot raise it.
   - For a run that counts, set a fixed refresh rate (Windows: Settings, System, Display, Advanced display, a rate
     that is not "Dynamic"). A renderer that keeps the fast rate for less than a quarter of its frames is judged on
     the calibrated one.
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

## Recorded runs

Frame time in ms, median / p95 over all phases, 3 repetitions per phase (2500 ms with a warm-up of 1000 ms on the
desktops, 2000 ms and 800 ms on the CI runners). Late in pan is the share of frames longer than 1.5 refresh
intervals. Each row is one machine, not a reference. The engine is the class the bench gives today, which
`npm run analyze` on the run's `results.json` gives again.

| Run | Engine | x1 | x10 | x100 (zoom p95) | x10 dense (late in pan) | Sanity |
|---|---|---|---|---|---|---|
| **Windows**, Intel Core Ultra 9 285H, Intel Arc 140T (ANGLE on Direct3D 11), WebView2 157, 101 Hz, commit db60b74 | gpu | 9.8 / 10.2 | 9.9 / 19.1 | 9.8 / 12.8 (39.1) | 9.9 / 19.6 (19.5 %) | PASS |
| **WSL**, same processor, Ubuntu 24.04.3 on WSLg, Mesa `llvmpipe` (no GPU), WebKitGTK 2.52.6, 63 Hz, commit e27e942 | software | 21.0 / 23.0 | 23.0 / 29.0 | 25.0 / 34.0 (61.0) | 24.0 / 30.0 (88.2 %) | PASS |
| **Linux CI runner** (GitHub-hosted virtual machine, AMD EPYC 9V45, 2 threads, Ubuntu 24.04.5, no GPU of its own), WebKitGTK 2.52.6, 63 Hz, commit 90555a6, **not a reference** | software (classed gpu when it ran) | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 18.0 (33.0) | 16.0 / 17.0 (0.0 %) | PASS |
| **macOS CI runner** (GitHub-hosted virtual machine, Apple M2 Pro (Virtual), 5 threads, macOS 15.8.2), WKWebView, 59 Hz, commit 90555a6, **not a reference** | gpu | 17.0 / 20.0 | 17.0 / 20.0 | 17.0 / 26.0 (28.0) | 17.0 / 20.0 (0.3 %) | PASS |
| **Windows**, the first row's machine with its display at 120 Hz, WebView2 158, commit 3fa4f07, before #489 | gpu | 8.3 / 8.5 | 8.4 / 16.7 | 8.3 / 8.5 (33.4) | 8.3 / 16.7 (38.4 %) | PASS |
| **Windows**, the same, commit feb6666, #489's level of detail | gpu | 8.3 / 8.4 | 8.3 / 8.4 | 8.3 / 8.5 (16.7) | 8.3 / 8.4 (0.0 %) | PASS |
| **Windows**, the same, commit 667c9bd, as #489 delivered it, calibrated at 60 Hz and read at the 120 Hz its frames ran at | gpu | 8.3 / 8.4 | 8.3 / 8.4 | 8.3 / 8.4 (16.7) | 8.3 / 8.4 (0.0 %) | PASS |
| **Linux CI runner** (GitHub-hosted virtual machine, 4 threads, Ubuntu 24.04.5, no GPU), WebKitGTK 2.52.6, 63 Hz, commit feb6666, **not a reference** | software | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 20.0 (24.0) | 16.0 / 17.0 (0.0 %) | PASS |
| **Linux CI runner** (GitHub-hosted virtual machine, 4 threads, Ubuntu 24.04.5, no GPU), WebKitGTK 2.52.6, 63 Hz, commit 667c9bd, **not a reference** | software | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 (22.0) | 16.0 / 17.0 (0.0 %) | PASS |
| **macOS CI runner** (GitHub-hosted virtual machine, 3 threads, macOS 15.8.2), WKWebView, 59 Hz, commit feb6666, **not a reference** | gpu | 17.0 / 19.0 | 17.0 / 26.0 | 17.0 / 95.0 (87.0) | 17.0 / 61.0 (2.0 %) | PASS |
| **macOS CI runner** (GitHub-hosted virtual machine, 3 threads, macOS 15.8.2), WKWebView, 59 Hz, commit 667c9bd, **not a reference** | gpu | 17.0 / 18.0 | 17.0 / 19.0 | 19.0 / 33.0 (29.0) | 17.0 / 19.0 (1.7 %) | PASS |

- **WSL is not a native Linux with a GPU.** WSLg rasterises on the processor. Its numbers are recorded as
  information, and the one threshold that applies to this engine, x10 with median at most 33 ms and p95 at most
  50 ms, passes (23.0 / 29.0). The rows of T5 that name a GPU are not judged here.
- On Windows T5 holds at x1 and does not hold at x100 (zoom) nor at x10 dense. That is the level of detail's work.
  With it (#489, at 120 Hz, where the threshold is 9.1 ms), x10 dense holds T5, with no frame late in pan and p95
  8.4 ms; x1 still holds, and x100 still misses it in zoom (p95 16.7 ms).
- Frame times of different machines do not compare: different display rates, engines and commits.
- **The 667c9bd row** is the run whose calibration read 60 Hz while its frames ran at 120 (Windows' dynamic refresh
  rate). Its `summary.md` judged it on 18.4 ms and gave x100 a PASS. The bench now reads such a run at the rate its
  frames ran at ("Read the result" above): x100 fails in zoom, as #489 had worked out by hand, and the other
  verdicts stand.
- **The CI rows are "not a reference".** They come from runs of `moviola-benchmark.yml` (`-f os=linux-macos`):
  #488's on `main`, #489's two on its branch, on virtual machines with no GPU of their own. They prove that the bench
  builds and runs in the product's webview on both systems, and that the sanity check passes there. They do not
  judge T5: the per-scale verdicts in their `summary.md` count as information on a runner. For the record, in #488's
  run Linux passes x10 on the software row (classed gpu then, it had x1 PASS, x100 FAIL with zoom p95 33.0 ms
  against 17.6, x10 dense PASS), and macOS fails x1 (p95 20.0 against 18.7), x100 and x10 dense. In #489's two runs
  macOS fails x100 and x10 dense, and x1 in the first.
- The CI rows are paced at the refresh rate of a virtual display (63 Hz and 59 Hz), so most frames sit on one
  interval and the medians say little; read the p95 and the zoom column.
- **The Linux runner is classed software.** The probe reads no renderer there (`glxinfo` finds no display, and
  WebKit answers "Apple GPU" to any WebGL query), and a GitHub Linux runner has no GPU; the window runs in Xvfb,
  which has none either. The bench classed it gpu until #683. On the software row of T5, x10 with median at most
  33 ms and p95 at most 50 ms, the three Linux runs pass (16.0 / 17.0), as information.
- **Cost of the CI runs.** Job durations from the run, each rounded up to the minute as the billing does, at the
  list price of 0.008 USD per Linux minute and ten times that on macOS: for #488's run the Linux job 7 min
  (0.06 USD) and the macOS job 5 min (0.40 USD), 0.46 USD in all, against the 0.8 USD that was estimated and
  approved. On the Free plan this is quota and not money; the run's timing endpoint reports 0 billable
  milliseconds, which is why the figure is computed and not read. #489's two runs took 12 Linux minutes and 10 macOS
  minutes (0.90 USD at list price). The repository is public since 25 November 2026, and its standard runners are
  not billed.
- A native Linux with a GPU and a Mac with a physical display are not measured yet (the Mac is #496).

### Headroom

x100 with every frame drawn 10 times (`moviola-benchmark.yml -f repeat=10`, the suite's headroom step): the cost of
drawing that the refresh interval hides. Main block only, 3 repetitions of 5000 ms per phase, warm-up 800 ms.
Frame time in ms, median / p95 per phase, with the frames pooled over the repetitions in brackets.

| Run | Engine | pan | zoom | playhead | hover | all phases | script (median) |
|---|---|---|---|---|---|---|---|
| **Linux CI runner** (GitHub-hosted virtual machine, 4 threads, Ubuntu 24.04.5, Mesa `llvmpipe` in Xvfb, no GPU), WebKitGTK 2.52.6, 63 Hz, commit 0d7f63f, **not a reference** | software | 40.0 / 47.0 (381) | 46.0 / 148.0 (241) | 41.0 / 53.0 (361) | 30.0 / 31.0 (497) | 37.0 / 64.0 (1480) | 32.0 |

- Ten draws take 37 ms at the median: in software one draw of x100 costs about a quarter of the runner's 16 ms
  interval, and the widest views of the zoom phase about nine tenths of it (p95 148 ms for ten).
- The zoom phase has 241 frames, where the renderer spike's headroom run on llvmpipe had fewer than 30 (#398): its
  p95 is a measure, not a handful of frames.
- A runner's numbers are information. WSL's llvmpipe and a native Linux with a GPU are not measured with ten draws.

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
| `--help` | print the options and stop, building and measuring nothing (`npm run bench:suite -- --help` too) |

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
  hand. CI minutes are a budget. `-f repeat=10` runs the suite's headroom step instead: x100 with every frame drawn
  10 times, in phases of 5 s, so that a slow zoom still has 30 frames.
- `ci.yml` runs `npm test` and builds the page on every push to `main` that touches the build, in its Linux job: a
  change of app/ui's stage that breaks the bench shows there.
