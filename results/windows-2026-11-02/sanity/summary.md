# Sanity check: windows-sanity (windows x86_64)

- webview: 156.0.4314.58; Tauri 2.12.5; Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36 Edg/156.0.0.0
- display: 1500x938 @ dpr 2; refresh interval measured 17.70 ms (56 Hz); stage 1280x720
- WebGL2 context: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) (Google Inc. (Intel))
- WebGPU: intel / xe-lpg
- environment flags: {}
- run: 2026-11-02T04:57:41.878Z, 103 s; 3 repetitions x 2500 ms per phase; scales 10
- dataset: x10 = 1550 nodes, 16 lanes, 91 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d+busyv | x10 | canvas2d | 35.0 / 36.1 | 34.9 / 36.1 | 35.0 / 36.0 | 34.9 / 36.0 | **35.0 / 36.0** | 37 | 28.60 | 63 % | 3 | 0.0 |
| canvas2d+shadow | x10 | canvas2d | 17.6 / 18.1 | 17.6 / 18.1 | 17.6 / 18.2 | 17.6 / 18.2 | **17.6 / 18.2** | 20 | 0.20 | 0 % | 3 | 0.0 |
| canvas2d | x10 | canvas2d | 17.7 / 18.2 | 17.6 / 18.2 | 17.6 / 18.2 | 17.6 / 18.1 | **17.6 / 18.1** | 19 | 0.20 | 0 % | 3 | 0.0 |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x10 | detail | 20 | 0.032 |
| x10 | label-zoom-wide | 101 | 0.080 |

## Verdict

- canvas2d + 28.3 ms (1.6 refresh intervals) of busy work per frame: script median 0.20 -> 28.60 ms (PASS: expected at least +21.2 ms); frame median 17.6 -> 35.0 ms (PASS: expected at least x1.3).
- canvas2d + shadowBlur on every node (a real drawing cost, not script time): script median 0.20 -> 0.20 ms; frame median 17.6 -> 17.6 ms; frame p95 18.1 -> 18.2 ms (NOT SEEN: expected a visible rise in frame median x1.15 or p95 x1.3; when the engine rasterises on a GPU fast enough the cost hides under the refresh interval, and then the headroom runs, with repeated drawing, are the control for the GPU side).

