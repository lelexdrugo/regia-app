# Benchmark: t (linux x86_64)

- webview: 2.52.6; Tauri 2.12.5; Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/60.5 Safari/605.1.15
- display: 1500x938 @ dpr 2; refresh interval measured 16.00 ms (63 Hz); stage 1280x720
- WebGL2 context: Apple GPU (Apple Inc.)
- WebGPU: not available
- environment flags: {"GALLIUM_DRIVER":"d3d12","WEBKIT_DISABLE_DMABUF_RENDERER":"1"}
- run: 2026-11-02T02:51:49.395Z, 6 s; 1 repetitions x 500 ms per phase; scales 10
- dataset: x10 = 1550 nodes, 16 lanes, 91 days

Frame time in ms: median / p95 of the pooled frames of all repetitions (`js` is the median script time per frame, label picking included). Lower is better; the display refresh interval is the floor.

| renderer | scale | backend / what rendered | pan | zoom | playhead | hover | all phases | js med | frames >1.5x refresh | rep spread of median |
|---|---|---|---|---|---|---|---|---|---|---|
| canvas2d | x10 | canvas2d | 18.0 / 21.0 | 19.0 / 19.0 | 19.0 / 22.0 | 18.0 / 19.0 | **19.0 / 21.0** | 4.00 | 1 % | 0.0 |
| webgl2-raw | x10 | webgl2: Apple GPU | 35.0 / 36.0 | 34.0 / 35.0 | 35.0 / 37.0 | 33.0 / 35.0 | **34.0 / 36.0** | 2.00 | 100 % | 0.0 |

## Screen-reader path (cost of the hidden DOM list for canvas/GL; SVG is native)

| scale | camera | items | ms per rebuild |
|---|---|---|---|
| x10 | detail | 19 | 0.040 |
| x10 | label-zoom-wide | 113 | 0.080 |

