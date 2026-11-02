# Benchmark: windows-stress (windows x86_64)

- webview: 156.0.4314.58; Tauri 2.12.5; Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36 Edg/156.0.0.0
- display: 1500x938 @ dpr 2; refresh interval measured 17.70 ms (56 Hz); stage 1280x720
- WebGL2 context: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) (Google Inc. (Intel))
- WebGPU: intel / xe-lpg
- environment flags: {}
- run: 2026-11-02T04:44:28.873Z, 620 s; 3 repetitions x 2500 ms per phase; scales 10d, 300, 1000
- dataset: x300 = 46500 nodes, 44 lanes, 993 days; x1000 = 155000 nodes, 64 lanes, 2275 days; x10d = 12400 nodes, 16 lanes, 91 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d-lod | x10d | canvas2d | 17.7 / 34.4 | 17.6 / 18.2 | 17.7 / 18.8 | 17.7 / 18.2 | **17.7 / 18.3** | 36 | 0.50 | 2 % | 3 | 0.0 |
| canvas2d-lod | x300 | canvas2d | 17.6 / 18.1 | 17.6 / 18.1 | 17.6 / 18.2 | 17.6 / 18.1 | **17.6 / 18.1** | 19 | 0.30 | 0 % | 3 | 0.1 |
| canvas2d | x10d | canvas2d | 17.7 / 34.9 | 17.7 / 34.6 | 17.8 / 18.8 | 17.7 / 18.2 | **17.7 / 18.4** | 36 | 0.50 | 4 % | 3 | 0.0 |
| canvas2d | x300 | canvas2d | 17.7 / 18.1 | 17.8 / 53.4 | 17.6 / 18.1 | 17.7 / 18.1 | **17.7 / 18.3** | 71 | 0.30 | 4 % | 3 | 0.1 |
| pixi-webgl | x1000 | webgl: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.1 | 17.7 / 18.1 | 17.7 / 18.2 | 17.6 / 18.2 | **17.7 / 18.2** | 19 | 0.30 | 0 % | 3 | 0.1 |
| pixi-webgl | x10d | webgl: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.7 / 18.2 | 17.6 / 18.2 | 17.6 / 18.1 | 17.6 / 18.1 | **17.6 / 18.2** | 19 | 0.70 | 0 % | 3 | 0.1 |
| pixi-webgl | x300 | webgl: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.7 / 18.2 | 17.6 / 18.2 | 17.7 / 18.2 | 17.6 / 18.1 | **17.7 / 18.2** | 19 | 0.30 | 0 % | 3 | 0.1 |
| canvas2d-lod | x1000 | canvas2d | 17.6 / 18.1 | 17.6 / 18.2 | 17.6 / 18.1 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.40 | 0 % | 3 | 0.0 |
| canvas2d | x1000 | canvas2d | 17.6 / 18.2 | 17.8 / 70.8 | 17.6 / 18.1 | 17.6 / 18.1 | **17.7 / 18.3** | 213 | 0.30 | 3 % | 3 | 0.1 |
| pixi-webgpu | x10d | webgpu: intel / xe-lpg | 17.7 / 18.1 | 17.6 / 18.2 | 17.6 / 18.2 | 17.7 / 18.2 | **17.6 / 18.2** | 23 | 0.80 | 0 % | 3 | 0.1 |
| pixi-webgpu | x300 | webgpu: intel / xe-lpg | 17.6 / 18.2 | 17.7 / 18.1 | 17.6 / 18.2 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.40 | 0 % | 3 | 0.1 |
| pixi-webgpu | x1000 | webgpu: intel / xe-lpg | 17.6 / 18.2 | 17.6 / 18.1 | 17.6 / 18.1 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.40 | 0 % | 3 | 0.0 |
| svg | x10d | svg | 17.7 / 18.2 | 17.6 / 18.1 | 17.6 / 18.2 | 17.7 / 18.2 | **17.7 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.1 |
| svg | x300 | svg | 17.6 / 18.2 | 35.4 / 53.0 | 17.7 / 18.2 | 17.7 / 18.2 | **17.8 / 35.6** | 71 | 0.10 | 12 % | 3 | 0.1 |
| webgl2-raw | x10d | webgl2: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.7 / 18.1 | 17.6 / 18.2 | 17.6 / 18.1 | 17.7 / 18.2 | **17.6 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.0 |
| webgl2-raw | x300 | webgl2: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.1 | 17.7 / 18.1 | 17.6 / 18.2 | 17.7 / 18.2 | **17.7 / 18.2** | 20 | 0.20 | 0 % | 3 | 0.1 |
| svg | x1000 | svg | 36.0 / 54.2 | 126.0 / 211.5 | 36.0 / 54.4 | 36.5 / 70.7 | **51.7 / 125.6** | 214 | 0.10 | 100 % | 3 | 15.8 |
| webgl2-raw | x1000 | webgl2: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.2 | 17.6 / 18.2 | 17.6 / 18.2 | 17.7 / 18.1 | **17.7 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.1 |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x10d | native | 2160 | native: 14721 DOM elements |
| x10d | detail | 0 | 0.026 |
| x10d | label-zoom-wide | 200 | 0.228 |
| x300 | native | 8100 | native: 55202 DOM elements |
| x300 | detail | 1 | 0.056 |
| x300 | label-zoom-wide | 58 | 0.066 |
| x1000 | native | 27000 | native: 184040 DOM elements |
| x1000 | detail | 3 | 0.096 |
| x1000 | label-zoom-wide | 13 | 0.064 |

