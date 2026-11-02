# Sanity check: ci-linux-sanity (linux x86_64)

- webview: 2.52.6; Tauri 2.12.5; Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15
- display: 1600x1000 @ dpr 1; refresh interval measured 16.00 ms (63 Hz); stage 1280x720
- WebGL2 context: Apple GPU (Apple Inc.)
- WebGPU: not available
- environment flags: {"WEBKIT_DISABLE_DMABUF_RENDERER":"1"}
- run: 2026-11-02T05:10:19.351Z, 83 s; 3 repetitions x 2000 ms per phase; scales 10
- dataset: x10 = 1550 nodes, 16 lanes, 91 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d+busyv | x10 | canvas2d | 34.0 / 36.0 | 35.0 / 46.0 | 34.0 / 37.0 | 33.0 / 35.0 | **34.0 / 38.0** | 48 | 30.00 | 100 % | 3 | 0.0 |
| canvas2d+shadow | x10 | canvas2d | 20.0 / 27.0 | 21.0 / 39.0 | 18.0 / 32.0 | 31.0 / 46.0 | **23.0 / 37.0** | 48 | 18.00 | 41 % | 3 | 0.0 |
| canvas2d | x10 | canvas2d | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 19 | 3.00 | 0 % | 3 | 0.0 |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x10 | detail | 20 | 0.100 |
| x10 | label-zoom-wide | 101 | 0.220 |

## Verdict

- canvas2d + 25.6 ms (1.6 refresh intervals) of busy work per frame: script median 3.00 -> 30.00 ms (PASS: expected at least +19.2 ms); frame median 16.0 -> 34.0 ms (PASS: expected at least x1.3).
- canvas2d + shadowBlur on every node (a real drawing cost, not script time): script median 3.00 -> 18.00 ms; frame median 16.0 -> 23.0 ms; frame p95 17.0 -> 37.0 ms (PASS: expected a visible rise in frame median x1.15 or p95 x1.3; when the engine rasterises on a GPU fast enough the cost hides under the refresh interval, and then the headroom runs, with repeated drawing, are the control for the GPU side).

