# Benchmark: ci-linux-main (linux x86_64)

- webview: 2.52.6; Tauri 2.12.5; Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15
- display: 1600x1000 @ dpr 1; refresh interval measured 16.00 ms (63 Hz); stage 1280x720
- WebGL2 context: Apple GPU (Apple Inc.)
- WebGPU: not available
- environment flags: {"WEBKIT_DISABLE_DMABUF_RENDERER":"1"}
- run: 2026-11-02T05:02:49.146Z, 419 s; 3 repetitions x 2000 ms per phase; scales 1, 10, 100
- dataset: x1 = 155 nodes, 8 lanes, 18 days; x10 = 1550 nodes, 16 lanes, 91 days; x100 = 15500 nodes, 32 lanes, 455 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d-lod | x1 | canvas2d | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 18 | 2.00 | 0 % | 3 | 0.0 |
| canvas2d | x1 | canvas2d | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 18 | 2.00 | 0 % | 3 | 0.0 |
| pixi-webgl | x1 | webgl: Apple GPU | 39.0 / 43.0 | 38.0 / 42.0 | 44.0 / 49.0 | 37.0 / 40.0 | **38.0 / 47.0** | 307 | 7.00 | 100 % | 3 | 2.0 |
| svg | x1 | svg | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 19 | 0.00 | 0 % | 3 | 0.0 |
| webgl2-raw | x1 | webgl2: Apple GPU | 33.0 / 36.0 | 33.0 / 35.0 | 33.0 / 36.0 | 34.0 / 39.0 | **33.0 / 36.0** | 53 | 2.00 | 100 % | 3 | 1.0 |
| canvas2d-lod | x10 | canvas2d | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 18 | 3.00 | 0 % | 3 | 0.0 |
| canvas2d | x10 | canvas2d | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 18 | 3.00 | 0 % | 3 | 0.0 |
| pixi-webgl | x10 | webgl: Apple GPU | 49.0 / 58.0 | 46.0 / 55.0 | 52.0 / 63.0 | 41.0 / 44.0 | **45.0 / 59.0** | 301 | 9.00 | 100 % | 3 | 1.0 |
| svg | x10 | svg | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 18 | 0.00 | 0 % | 3 | 0.0 |
| webgl2-raw | x10 | webgl2: Apple GPU | 37.0 / 40.0 | 36.0 / 40.0 | 36.0 / 39.0 | 36.0 / 39.0 | **36.0 / 40.0** | 43 | 3.00 | 100 % | 3 | 1.0 |
| canvas2d-lod | x100 | canvas2d | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 20 | 3.00 | 0 % | 3 | 0.0 |
| canvas2d | x100 | canvas2d | 16.0 / 17.0 | 16.0 / 45.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 17.0** | 206 | 2.00 | 1 % | 3 | 0.0 |
| pixi-webgl | x100 | webgl: Apple GPU | 55.0 / 59.0 | 61.0 / 112.0 | 58.0 / 68.0 | 46.0 / 51.0 | **55.0 / 80.0** | 305 | 17.00 | 100 % | 3 | 1.0 |
| svg | x100 | svg | 16.0 / 17.0 | 16.0 / 32.0 | 16.0 / 17.0 | 16.0 / 17.0 | **16.0 / 19.0** | 43 | 0.00 | 3 % | 3 | 0.0 |
| webgl2-raw | x100 | webgl2: Apple GPU | 40.0 / 42.0 | 43.0 / 53.0 | 39.0 / 44.0 | 39.0 / 41.0 | **39.0 / 48.0** | 64 | 6.00 | 100 % | 3 | 2.0 |

## Configurations that did not run

| renderer | scale | rep | reason |
|---|---|---|---|
| pixi-webgpu | x1 | 1 | No available renderer for the current environment |
| pixi-webgpu | x10 | 1 | No available renderer for the current environment |
| pixi-webgpu | x100 | 1 | No available renderer for the current environment |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x1 | native | 27 | native: 196 DOM elements |
| x1 | detail | 15 | 0.060 |
| x1 | label-zoom-wide | 27 | 0.060 |
| x10 | native | 270 | native: 1863 DOM elements |
| x10 | detail | 20 | 0.060 |
| x10 | label-zoom-wide | 101 | 0.200 |
| x100 | native | 2700 | native: 18433 DOM elements |
| x100 | detail | 13 | 0.140 |
| x100 | label-zoom-wide | 154 | 0.260 |

