# Benchmark: linux-wslg-headroom (linux x86_64)

- webview: 2.52.6; Tauri 2.12.5; Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15
- display: 1500x938 @ dpr 2; refresh interval measured 16.00 ms (63 Hz); stage 1280x720
- WebGL2 context: Apple GPU (Apple Inc.)
- WebGPU: not available
- environment flags: {"WEBKIT_DISABLE_DMABUF_RENDERER":"1"}
- run: 2026-11-02T05:08:25.930Z, 142 s; 3 repetitions x 2500 ms per phase; scales 100
- dataset: x100 = 15500 nodes, 32 lanes, 455 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d (x10 draws/frame) | x100 | canvas2d | 49.0 / 58.0 | n/a / n/a | 43.0 / 59.0 | 37.0 / 40.0 | **40.0 / 58.0** | 1338 | 24.00 | 100 % | 3 | 1.0 |
| canvas2d-lod (x10 draws/frame) | x100 | canvas2d | 49.0 / 58.0 | 57.0 / 115.0 | 44.0 / 62.0 | 37.0 / 40.0 | **40.0 / 83.0** | 122 | 25.00 | 100 % | 3 | 1.0 |
| pixi-webgl (x10 draws/frame) | x100 | webgl: Apple GPU | 105.0 / 116.0 | 112.0 / 197.0 | 115.0 / 132.0 | 91.0 / 95.0 | **105.0 / 142.0** | 252 | 52.00 | 100 % | 3 | 4.0 |
| webgl2-raw (x10 draws/frame) | x100 | webgl2: Apple GPU | 68.0 / 74.0 | 72.0 / 84.0 | 71.0 / 79.0 | 65.0 / 69.0 | **69.0 / 78.0** | 92 | 30.00 | 100 % | 3 | 1.0 |

## Configurations that did not run

| renderer | scale | rep | reason |
|---|---|---|---|
| pixi-webgpu | x100 | 1 | No available renderer for the current environment |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x100 | detail | 13 | 0.060 |
| x100 | label-zoom-wide | 154 | 0.120 |

