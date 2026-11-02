# Benchmark: ci-windows-main (windows x86_64)

- webview: 156.0.4314.50; Tauri 2.12.5; Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36 Edg/156.0.0.0
- display: 1024x768 @ dpr 1; refresh interval measured 15.60 ms (64 Hz); stage 1280x720
- WebGL2 context: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) (Google Inc. (Microsoft))
- WebGPU: not available (no adapter)
- environment flags: {}
- run: 2026-11-02T05:03:43.303Z, 415 s; 3 repetitions x 2000 ms per phase; scales 1, 10, 100
- dataset: x1 = 155 nodes, 8 lanes, 18 days; x10 = 1550 nodes, 16 lanes, 91 days; x100 = 15500 nodes, 32 lanes, 455 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d-lod | x1 | canvas2d | 15.6 / 15.7 | 15.6 / 31.3 | 15.6 / 31.3 | 15.7 / 31.3 | **15.6 / 31.3** | 47 | 0.20 | 15 % | 3 | 0.0 |
| canvas2d | x1 | canvas2d | 15.6 / 31.2 | 15.7 / 31.3 | 15.6 / 31.3 | 15.7 / 31.3 | **15.6 / 31.3** | 47 | 0.20 | 22 % | 3 | 0.0 |
| pixi-webgl | x1 | webgl: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 47 | 0.70 | 20 % | 3 | 0.0 |
| svg | x1 | svg | 15.6 / 15.7 | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 31 | 0.10 | 13 % | 3 | 0.0 |
| webgl2-raw | x1 | webgl2: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) | 15.7 / 31.3 | 15.7 / 31.3 | 15.7 / 31.3 | 15.6 / 31.3 | **15.7 / 31.3** | 31 | 0.30 | 29 % | 3 | 0.0 |
| canvas2d-lod | x10 | canvas2d | 15.6 / 31.2 | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 63 | 0.30 | 19 % | 3 | 0.0 |
| canvas2d | x10 | canvas2d | 15.6 / 31.2 | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 47 | 0.40 | 20 % | 3 | 0.0 |
| pixi-webgl | x10 | webgl: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) | 15.7 / 31.3 | 15.6 / 31.3 | 15.7 / 31.3 | 15.6 / 31.2 | **15.6 / 31.3** | 31 | 1.00 | 27 % | 3 | 0.1 |
| svg | x10 | svg | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 31 | 0.20 | 18 % | 3 | 0.0 |
| webgl2-raw | x10 | webgl2: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) | 15.6 / 31.3 | 15.7 / 31.3 | 15.6 / 31.3 | 15.7 / 31.3 | **15.7 / 31.3** | 31 | 0.40 | 25 % | 3 | 0.1 |
| canvas2d-lod | x100 | canvas2d | 15.6 / 31.2 | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 63 | 0.40 | 19 % | 3 | 0.0 |
| canvas2d | x100 | canvas2d | 15.6 / 31.2 | 15.6 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 47 | 0.40 | 14 % | 3 | 0.0 |
| pixi-webgl | x100 | webgl: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) | 31.3 / 46.9 | 46.8 / 62.4 | 31.3 / 46.9 | 31.2 / 31.3 | **31.3 / 46.9** | 63 | 2.20 | 97 % | 3 | 0.0 |
| svg | x100 | svg | 15.6 / 31.3 | 15.7 / 31.3 | 15.6 / 31.3 | 15.6 / 31.3 | **15.6 / 31.3** | 47 | 0.20 | 19 % | 3 | 0.0 |
| webgl2-raw | x100 | webgl2: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) | 31.2 / 31.3 | 31.2 / 31.4 | 31.2 / 31.3 | 31.2 / 31.3 | **31.2 / 31.3** | 47 | 0.40 | 76 % | 3 | 0.0 |

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
| x1 | detail | 15 | 0.030 |
| x1 | label-zoom-wide | 27 | 0.046 |
| x10 | native | 270 | native: 1863 DOM elements |
| x10 | detail | 20 | 0.042 |
| x10 | label-zoom-wide | 101 | 0.088 |
| x100 | native | 2700 | native: 18433 DOM elements |
| x100 | detail | 13 | 0.046 |
| x100 | label-zoom-wide | 154 | 0.254 |

