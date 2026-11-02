# Sanity check: ci-windows-sanity (windows x86_64)

- webview: 156.0.4314.50; Tauri 2.12.5; Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36 Edg/156.0.0.0
- display: 1024x768 @ dpr 1; refresh interval measured 15.60 ms (64 Hz); stage 1280x720
- WebGL2 context: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) (Google Inc. (Microsoft))
- WebGPU: not available (no adapter)
- environment flags: {}
- run: 2026-11-02T05:10:40.546Z, 83 s; 3 repetitions x 2000 ms per phase; scales 10
- dataset: x10 = 1550 nodes, 16 lanes, 91 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d+busyv | x10 | canvas2d | 31.2 / 31.3 | 31.2 / 31.3 | 31.2 / 31.3 | 31.2 / 31.3 | **31.2 / 31.3** | 47 | 25.50 | 79 % | 3 | 0.0 |
| canvas2d+shadow | x10 | canvas2d | 15.6 / 31.3 | 15.6 / 31.3 | 15.7 / 31.3 | 31.2 / 46.8 | **15.7 / 31.3** | 47 | 0.50 | 37 % | 3 | 0.0 |
| canvas2d | x10 | canvas2d | 15.6 / 31.3 | 15.7 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 63 | 0.40 | 19 % | 3 | 0.0 |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x10 | detail | 20 | 0.056 |
| x10 | label-zoom-wide | 101 | 0.110 |

## Verdict

- canvas2d + 25.0 ms (1.6 refresh intervals) of busy work per frame: script median 0.40 -> 25.50 ms (PASS: expected at least +18.7 ms); frame median 15.6 -> 31.2 ms (PASS: expected at least x1.3).
- canvas2d + shadowBlur on every node (a real drawing cost, not script time): script median 0.40 -> 0.50 ms; frame median 15.6 -> 15.7 ms; frame p95 31.3 -> 31.3 ms (NOT SEEN: expected a visible rise in frame median x1.15 or p95 x1.3; when the engine rasterises on a GPU fast enough the cost hides under the refresh interval, and then the headroom runs, with repeated drawing, are the control for the GPU side).

