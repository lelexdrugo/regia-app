# Benchmark: windows-main (windows x86_64)

- webview: 156.0.4314.58; Tauri 2.12.5; Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36 Edg/156.0.0.0
- display: 1500x938 @ dpr 2; refresh interval measured 17.70 ms (56 Hz); stage 1280x720
- WebGL2 context: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) (Google Inc. (Intel))
- WebGPU: intel / xe-lpg
- environment flags: {}
- run: 2026-11-02T04:25:36.455Z, 616 s; 3 repetitions x 2500 ms per phase; scales 1, 10, 100
- dataset: x1 = 155 nodes, 8 lanes, 18 days; x10 = 1550 nodes, 16 lanes, 91 days; x100 = 15500 nodes, 32 lanes, 455 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d-lod | x1 | canvas2d | 17.7 / 18.2 | 17.6 / 18.1 | 17.7 / 18.2 | 17.6 / 18.1 | **17.6 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.1 |
| canvas2d | x1 | canvas2d | 17.6 / 18.1 | 17.6 / 18.1 | 17.7 / 18.2 | 17.7 / 18.1 | **17.6 / 18.1** | 19 | 0.20 | 0 % | 3 | 0.1 |
| pixi-webgl | x1 | webgl: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.2 | 17.6 / 18.1 | 17.6 / 18.2 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.30 | 0 % | 3 | 0.1 |
| pixi-webgpu | x1 | webgpu: intel / xe-lpg | 17.6 / 18.1 | 17.6 / 18.2 | 17.6 / 18.1 | 17.6 / 18.1 | **17.6 / 18.1** | 19 | 0.50 | 0 % | 3 | 0.0 |
| svg | x1 | svg | 17.6 / 18.2 | 17.6 / 18.1 | 17.7 / 18.1 | 17.7 / 18.2 | **17.6 / 18.1** | 19 | 0.10 | 0 % | 3 | 0.1 |
| webgl2-raw | x1 | webgl2: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.2 | 17.6 / 18.2 | 17.7 / 18.2 | 17.6 / 18.1 | **17.6 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.1 |
| canvas2d-lod | x10 | canvas2d | 17.6 / 18.2 | 17.6 / 18.1 | 17.7 / 18.2 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.1 |
| canvas2d | x10 | canvas2d | 17.7 / 18.2 | 17.7 / 18.2 | 17.6 / 18.1 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.1 |
| pixi-webgl | x10 | webgl: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.2 | 17.6 / 18.2 | 17.6 / 18.2 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.50 | 0 % | 3 | 0.0 |
| pixi-webgpu | x10 | webgpu: intel / xe-lpg | 17.6 / 18.2 | 17.7 / 18.2 | 17.7 / 18.2 | 17.6 / 18.1 | **17.6 / 18.2** | 19 | 0.70 | 0 % | 3 | 0.1 |
| svg | x10 | svg | 17.7 / 18.2 | 17.6 / 18.1 | 17.6 / 18.1 | 17.7 / 18.2 | **17.6 / 18.2** | 20 | 0.20 | 0 % | 3 | 0.1 |
| webgl2-raw | x10 | webgl2: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.7 / 18.2 | 17.7 / 18.2 | 17.6 / 18.2 | 17.6 / 18.1 | **17.7 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.1 |
| canvas2d-lod | x100 | canvas2d | 17.6 / 18.1 | 17.7 / 18.2 | 17.7 / 18.2 | 17.7 / 18.2 | **17.7 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.0 |
| canvas2d | x100 | canvas2d | 17.6 / 18.2 | 17.7 / 18.4 | 17.7 / 18.2 | 17.6 / 18.2 | **17.7 / 18.2** | 36 | 0.30 | 1 % | 3 | 0.1 |
| pixi-webgl | x100 | webgl: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.2 | 17.7 / 18.2 | 17.6 / 18.1 | 17.7 / 18.2 | **17.6 / 18.2** | 19 | 0.50 | 0 % | 3 | 0.1 |
| pixi-webgpu | x100 | webgpu: intel / xe-lpg | 17.6 / 18.1 | 17.6 / 18.2 | 17.6 / 18.2 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.50 | 0 % | 3 | 0.0 |
| svg | x100 | svg | 17.7 / 18.2 | 17.6 / 18.1 | 17.6 / 18.2 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.10 | 0 % | 3 | 0.0 |
| webgl2-raw | x100 | webgl2: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.2 | 17.6 / 18.2 | 17.6 / 18.2 | 17.7 / 18.2 | **17.7 / 18.2** | 19 | 0.20 | 0 % | 3 | 0.1 |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x1 | native | 27 | native: 196 DOM elements |
| x1 | detail | 15 | 0.018 |
| x1 | label-zoom-wide | 27 | 0.044 |
| x10 | native | 270 | native: 1863 DOM elements |
| x10 | detail | 20 | 0.020 |
| x10 | label-zoom-wide | 101 | 0.086 |
| x100 | native | 2700 | native: 18433 DOM elements |
| x100 | detail | 13 | 0.040 |
| x100 | label-zoom-wide | 154 | 0.124 |

