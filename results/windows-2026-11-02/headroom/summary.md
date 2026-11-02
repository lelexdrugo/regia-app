# Benchmark: windows-headroom (windows x86_64)

- webview: 156.0.4314.58; Tauri 2.12.5; Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36 Edg/156.0.0.0
- display: 1500x938 @ dpr 2; refresh interval measured 17.60 ms (57 Hz); stage 1280x720
- WebGL2 context: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) (Google Inc. (Intel))
- WebGPU: intel / xe-lpg
- environment flags: {}
- run: 2026-11-02T04:54:49.183Z, 172 s; 3 repetitions x 2500 ms per phase; scales 100
- dataset: x100 = 15500 nodes, 32 lanes, 455 days

Frame time in ms: median / p95 of the pooled frames of all repetitions; `n/a` = fewer than 30 frames. Lower is better; the display refresh interval is the floor, and one missed refresh costs one whole interval. `script ms` is the time inside the draw function: it includes rasterisation on some engines (WebKit software) and not on others (Chromium with GPU), and for SVG it leaves out style, layout and paint, so it is not a cost to compare.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | max | script ms (median, not comparable across engines) | frames >1.5x refresh | repetitions | spread of rep medians |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d (x10 draws/frame) | x100 | canvas2d | 53.2 / 87.3 | 53.8 / 264.2 | 35.6 / 53.5 | 35.6 / 53.7 | **51.7 / 89.0** | 354 | 0.90 | 97 % | 3 | 15.8 |
| canvas2d-lod (x10 draws/frame) | x100 | canvas2d | 53.2 / 88.4 | 52.9 / 89.1 | 35.6 / 54.0 | 35.8 / 53.6 | **52.0 / 87.6** | 107 | 1.00 | 96 % | 3 | 16.0 |
| pixi-webgl (x10 draws/frame) | x100 | webgl: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.6 / 18.2 | 17.6 / 18.2 | 17.6 / 18.2 | 17.6 / 18.2 | **17.6 / 18.2** | 19 | 0.60 | 0 % | 3 | 0.0 |
| pixi-webgpu (x10 draws/frame) | x100 | webgpu: intel / xe-lpg | 17.7 / 18.1 | 17.6 / 18.1 | 17.6 / 18.1 | 17.6 / 18.1 | **17.6 / 18.1** | 19 | 0.70 | 0 % | 3 | 0.1 |
| webgl2-raw (x10 draws/frame) | x100 | webgl2: ANGLE (Intel, Intel(R) Arc(TM) 140T GPU (16GB) (0x00007D51) Direct3D11 vs_5_0 ps_5_0, D3D11) | 17.7 / 18.2 | 17.7 / 18.2 | 17.6 / 18.2 | 17.7 / 18.2 | **17.7 / 18.2** | 19 | 0.40 | 0 % | 3 | 0.1 |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x100 | detail | 13 | 0.042 |
| x100 | label-zoom-wide | 154 | 0.146 |

