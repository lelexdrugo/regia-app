# Sanity check: ci-macos-sanity (macos aarch64)

- webview: 21624.5.1.11.3; Tauri 2.12.5; Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)
- display: 1024x768 @ dpr 1; refresh interval measured 17.00 ms (59 Hz); stage 1280x720
- WebGL2 context: Apple GPU (Apple Inc.)
- WebGPU: not available (no adapter)
- environment flags: {}
- run: 2026-11-02T05:08:06.334Z, 83 s; 3 repetitions x 2000 ms per phase; scales 10
- dataset: x10 = 1550 nodes, 16 lanes, 91 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d+busyv | x10 | canvas2d | 29.0 / 30.0 | 29.0 / 31.0 | 29.0 / 30.0 | 29.0 / 30.0 | **29.0 / 30.0** | 32 | 28.00 | 100 % | 3 | 0.0 |
| canvas2d+shadow | x10 | canvas2d | 17.0 / 20.0 | 17.0 / 20.0 | 17.0 / 20.0 | 17.0 / 19.0 | **17.0 / 20.0** | 26 | 0.00 | 0 % | 3 | 0.0 |
| canvas2d | x10 | canvas2d | 17.0 / 20.0 | 17.0 / 20.0 | 17.0 / 19.0 | 17.0 / 20.0 | **17.0 / 20.0** | 23 | 1.00 | 0 % | 3 | 0.0 |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x10 | detail | 20 | 0.240 |
| x10 | label-zoom-wide | 101 | 0.140 |

## Verdict

- canvas2d + 27.2 ms (1.6 refresh intervals) of busy work per frame: script median 1.00 -> 28.00 ms (PASS: expected at least +20.4 ms); frame median 17.0 -> 29.0 ms (PASS: expected at least x1.3).
- canvas2d + shadowBlur on every node (a real drawing cost, not script time): script median 1.00 -> 0.00 ms; frame median 17.0 -> 17.0 ms; frame p95 20.0 -> 20.0 ms (NOT SEEN: expected a visible rise in frame median x1.15 or p95 x1.3; when the engine rasterises on a GPU fast enough the cost hides under the refresh interval, and then the headroom runs, with repeated drawing, are the control for the GPU side).

