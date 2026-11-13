// The moviola's stage: one Canvas the size of its host, at the screen's pixel density, drawing one project's
// timeline (canvas2d.ts) on demand, never in a loop. It holds the camera (a hand drags it, the wheel zooms it, a
// double click and 0 bring it home), the selection (a click finds the node with the grid's hitTest; the arrows go
// from moment to moment), the playhead (dragged on the stage or on the tape, moved by a selection, as the UX
// prototype does) and the colours, read again from the tokens when the direction on <html> changes: a new theme is
// a redraw, not a reload. The canvas carries what the proofs wait on: `data-frames`, the frames drawn, `data-dir`,
// the direction of the last one, `data-camera`, its camera, and `data-view`, the stage above the tape.
import { clampCamera, type Extent, homeCamera, panBy, reveal, zoomAround } from "./camera";
import { AXIS, bucketByKind, type CanvasText, cut, drawFrame, type Frame, TAPE, type Tape } from "./canvas2d";
import { dayStarts, HOUR, laneEnvelope, type Layout, layout, type Model, timeScale } from "./model";
import { readPalette, type Palette } from "./palette";
import { buildScene, type Camera, hitTest, newLabels, pickLabels, type Scene, toScreenX, toScreenY, toWorldX, toWorldY, type View } from "./scene";

export type StageOptions = {
  text: CanvasText;
  /** The days of this machine's calendar: the axis labels the first milestone of each. */
  dayKey: (t: number) => string;
  /** Called when the playhead or the selection moves. */
  onchange?: (stage: Stage) => void;
  /** Off only for the harness's negative control: a stage that does not hear a change of direction. */
  observeTheme?: boolean;
};

type Drag =
  | { kind: "pan"; x: number; y: number; moved: boolean; node: number }
  | { kind: "playhead" }
  | { kind: "tape" };

const ZOOM_STEP = 1.25;

export class Stage {
  readonly model: Model;
  /** The playhead's instant. */
  t: number;
  /** The selected node, a model index, or -1. */
  selected = -1;
  camera: Camera = { cx: 0, cy: 0, z: 1 };
  /** Frames drawn so far. */
  frames = 0;
  /** Resolves once the first frame is drawn with the faces loaded. */
  readonly ready: Promise<void>;

  private readonly host: HTMLElement;
  private readonly canvas: HTMLCanvasElement;
  private readonly ctx: CanvasRenderingContext2D;
  private readonly options: StageOptions;
  private palette: Palette;
  private text: CanvasText;
  private lay: Layout | null = null;
  private scene: Scene | null = null;
  private frame: Frame | null = null;
  /** The scene index of each model node. */
  private sceneOf = new Int32Array(0);
  private view: View = { w: 0, h: 0 };
  private width = 0;
  private height = 0;
  private dpr = 1;
  private hover = -1;
  private drag: Drag | null = null;
  private pending = 0;
  private fonts = false;
  private settle: () => void = () => {};
  private readonly labels = newLabels();
  private readonly cleanup: (() => void)[] = [];

  constructor(host: HTMLElement, canvas: HTMLCanvasElement, model: Model, options: StageOptions) {
    this.host = host;
    this.canvas = canvas;
    this.model = model;
    this.options = options;
    this.text = options.text;
    this.t = model.t1;
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) {
      throw new Error("no 2D context");
    }
    this.ctx = ctx;
    this.palette = readPalette();
    this.ready = new Promise((resolve) => (this.settle = resolve));

    const resized = new ResizeObserver(() => this.resize());
    resized.observe(host);
    this.cleanup.push(() => resized.disconnect());
    this.watchDensity();
    if (options.observeTheme !== false) {
      const themed = new MutationObserver(() => this.retheme());
      themed.observe(document.documentElement, { attributes: true, attributeFilter: ["data-dir", "data-density"] });
      this.cleanup.push(() => themed.disconnect());
    }
    const fonts = document.fonts;
    const loaded = () => this.render();
    fonts.addEventListener("loadingdone", loaded);
    this.cleanup.push(() => fonts.removeEventListener("loadingdone", loaded));
    void fonts.ready.then(() => {
      this.fonts = true;
      this.render();
    });

    const on = <K extends keyof HTMLElementEventMap>(type: K, listener: (event: HTMLElementEventMap[K]) => void, options?: AddEventListenerOptions) => {
      canvas.addEventListener(type, listener, options);
      this.cleanup.push(() => canvas.removeEventListener(type, listener, options));
    };
    on("pointerdown", (event) => this.pointerdown(event));
    on("pointermove", (event) => this.pointermove(event));
    on("pointerup", (event) => this.pointerup(event));
    on("pointercancel", (event) => this.pointerup(event));
    on("pointerleave", () => this.setHover(-1));
    on("wheel", (event) => this.wheel(event), { passive: false });
    on("dblclick", () => this.home());
    on("keydown", (event) => this.keydown(event));
    this.resize();
  }

  destroy(): void {
    if (this.pending) cancelAnimationFrame(this.pending);
    for (const undo of this.cleanup.splice(0)) undo();
  }

  /** The words of the catalog in use: a new language is a redraw. */
  setText(text: CanvasText): void {
    this.text = text;
    if (this.frame) this.frame.text = text;
    this.render();
  }

  /** The stage's extent, the stage view and the tape's scale, at the host's size. */
  get extent(): Extent {
    return { w: this.lay?.width ?? 0, h: this.lay?.height ?? 0 };
  }

  get stageView(): View {
    return this.view;
  }

  get currentScene(): Scene | null {
    return this.scene;
  }

  get currentLayout(): Layout | null {
    return this.lay;
  }

  /** The node under a screen point, for this camera, becomes the one under the pointer: the bench's hover. */
  hoverAt(x: number, y: number, camera: Camera): void {
    this.camera = camera;
    this.hover = this.nodeAt(x, y);
  }

  /** The tape's scale and its two ends. */
  get tape(): Tape | null {
    return this.frame?.tape ?? null;
  }

  /** The playhead's place on the range, from 0 at the first milestone to 1000 at the last, in time. Not the tape's
   * scale, as the prototype's range was: there a joint is as wide as a day, and the hour of margin at each end of a
   * sparse story takes most of the range. */
  rangeValue(): number {
    const { t0, t1 } = this.model;
    return t1 > t0 ? Math.round(Math.min(1, Math.max(0, (this.t - t0) / (t1 - t0))) * 1000) : 1000;
  }

  setRange(value: number): void {
    const { t0, t1 } = this.model;
    this.setPlayhead(t0 + ((t1 - t0) * value) / 1000, true);
  }

  /** `impostaT`: the playhead within six hours of the first and the last milestone. With `follow`, the camera
   * keeps it in view. */
  setPlayhead(t: number, follow = false): void {
    const m = this.model;
    this.t = Math.min(m.t1 + 6 * HOUR, Math.max(m.t0 - 6 * HOUR, t));
    if (follow && this.lay) {
      this.camera = reveal(this.camera, this.lay.xAt(this.t), this.camera.cy, 60, this.extent, this.view);
    }
    this.changed();
  }

  /** Selects a model node (or none, with -1); the playhead goes to its instant and the camera shows it. */
  select(index: number): void {
    const node = this.model.nodes[index];
    this.selected = node ? index : -1;
    if (node && this.lay) {
      this.t = node.t;
      this.camera = reveal(this.camera, this.lay.x[index] as number, this.lay.y[index] as number, 48, this.extent, this.view);
    }
    this.changed();
  }

  /** `saltaFotogramma`: the next or the previous milestone from the selection, or from the playhead. */
  step(direction: 1 | -1): void {
    const nodes = this.model.nodes;
    if (nodes.length === 0) return;
    if (this.selected >= 0) {
      this.select(Math.min(nodes.length - 1, Math.max(0, this.selected + direction)));
      return;
    }
    const next =
      direction > 0 ? nodes.find((n) => n.t > this.t + 1000) ?? nodes[nodes.length - 1] : [...nodes].reverse().find((n) => n.t < this.t - 1000) ?? nodes[0];
    if (next) this.select(next.index);
  }

  /** Zoom by `factor` around the selection, or the centre of the stage. */
  zoom(factor: number): void {
    const scene = this.scene;
    const k = this.selected >= 0 ? (this.sceneOf[this.selected] as number) : -1;
    const sx = scene && k >= 0 ? toScreenX(this.camera, this.view, scene.x[k] as number) : this.view.w / 2;
    const sy = scene && k >= 0 ? toScreenY(this.camera, this.view, scene.y[k] as number) : this.view.h / 2;
    this.camera = zoomAround(this.camera, sx, sy, factor, this.extent, this.view);
    this.render();
  }

  home(): void {
    this.camera = homeCamera(this.extent, this.view);
    this.render();
  }

  /** Draws now, with this camera: for the bench, which moves it at every frame. */
  drawNow(camera: Camera = this.camera): void {
    if (!this.frame) return;
    this.camera = camera;
    this.frame.cam = camera;
    this.frame.t = this.t;
    this.frame.selected = this.selected >= 0 ? (this.sceneOf[this.selected] as number) : -1;
    this.frame.hover = this.hover;
    pickLabels(this.frame.scene, camera, this.view, (k) => this.labelWidth(k), this.labels);
    drawFrame(this.ctx, this.frame);
    this.frames += 1;
  }

  /** Asks for one frame, at the next animation frame; asking twice draws once. */
  render(): void {
    if (this.pending) return;
    this.pending = requestAnimationFrame(() => {
      this.pending = 0;
      // A new density that no event announced (Chromium 156 does not, for one emulated) is caught by the frame.
      if ((devicePixelRatio || 1) !== this.dpr) this.resize();
      if (!this.frame) return;
      this.drawNow();
      const { cx, cy, z } = this.camera;
      this.canvas.dataset["frames"] = String(this.frames);
      this.canvas.dataset["dir"] = this.palette.direction;
      this.canvas.dataset["camera"] = `${cx.toFixed(1)} ${cy.toFixed(1)} ${z.toFixed(3)}`;
      this.canvas.dataset["view"] = `${this.view.w} ${this.view.h}`;
      if (this.fonts) this.settle();
    });
  }

  private changed(): void {
    this.options.onchange?.(this);
    this.render();
  }

  private retheme(): void {
    this.palette = readPalette();
    this.widths.clear();
    if (this.frame) this.frame.palette = this.palette;
    this.render();
  }

  /** A screen of another density moves the pixels under the canvas: the backing store follows, at once where the
   * engine says so, otherwise at the next frame. */
  private watchDensity(): void {
    const query = matchMedia(`(resolution: ${devicePixelRatio || 1}dppx)`);
    const changed = () => {
      query.removeEventListener("change", changed);
      this.resize();
      this.watchDensity();
    };
    query.addEventListener("change", changed);
    this.cleanup.push(() => query.removeEventListener("change", changed));
  }

  private readonly widths = new Map<number, number>();

  private labelWidth(k: number): number {
    let w = this.widths.get(k);
    if (w === undefined) {
      this.ctx.font = `12px ${this.palette.font}`;
      w = this.ctx.measureText(this.labelOf(k)).width;
      this.widths.set(k, w);
    }
    return w;
  }

  private labelOf(k: number): string {
    const scene = this.scene;
    const node = scene ? this.model.nodes[scene.node[k] as number] : undefined;
    return node ? cut(node.milestone.title) : "";
  }

  /** The host's size, the backing store at the screen's density, and the stage laid out again at the new size,
   * the camera on the same part of it. */
  private resize(): void {
    const w = this.host.clientWidth;
    const h = this.host.clientHeight;
    if (w <= 0 || h <= TAPE + AXIS) return;
    const dpr = devicePixelRatio || 1;
    if (w === this.width && h === this.height && dpr === this.dpr && this.frame) return;
    const before = this.lay ? { cam: this.camera, extent: this.extent } : null;
    this.width = w;
    this.height = h;
    this.dpr = dpr;
    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
    this.view = { w, h: h - TAPE };
    const lay = layout(this.model, this.view.w, this.view.h);
    const scene = buildScene(this.model, lay);
    this.lay = lay;
    this.scene = scene;
    this.sceneOf = new Int32Array(this.model.nodes.length);
    scene.node.forEach((index, k) => (this.sceneOf[index] = k));
    this.widths.clear();
    const extent = this.extent;
    if (before) {
      const sx = extent.w / Math.max(1, before.extent.w);
      const sy = extent.h / Math.max(1, before.extent.h);
      this.camera = clampCamera({ cx: before.cam.cx * sx, cy: before.cam.cy * sy, z: before.cam.z }, extent, this.view);
    } else {
      this.camera = homeCamera(extent, this.view);
    }
    const times = this.model.nodes.map((n) => n.t);
    const tape: Tape = { scale: timeScale(times, 10, w - 10, 12 * HOUR, 26), x0: 10, x1: w - 10 };
    this.frame = {
      model: this.model,
      layout: lay,
      scene,
      buckets: bucketByKind(scene),
      envelopes: this.model.lanes.flatMap((lane) => laneEnvelope(this.model, lay, lane) ?? []),
      days: dayStarts(this.model, this.options.dayKey),
      cam: this.camera,
      view: this.view,
      width: w,
      height: h,
      dpr,
      palette: this.palette,
      t: this.t,
      selected: -1,
      hover: -1,
      labels: this.labels,
      labelText: (k) => this.labelOf(k),
      text: this.text,
      tape,
    };
    this.render();
  }

  private local(event: MouseEvent): { x: number; y: number } {
    const box = this.canvas.getBoundingClientRect();
    return { x: event.clientX - box.left, y: event.clientY - box.top };
  }

  /** The node under a screen point, as a scene index, or -1: within twelve pixels at any zoom. */
  private nodeAt(x: number, y: number): number {
    const scene = this.scene;
    if (!scene || y < AXIS || y > this.view.h) return -1;
    const radius = 12 / this.camera.z;
    return hitTest(scene, toWorldX(this.camera, this.view, x), toWorldY(this.camera, this.view, y), radius);
  }

  private playheadX(): number {
    return this.lay ? toScreenX(this.camera, this.view, this.lay.xAt(this.t)) : -1e9;
  }

  /** `setPointerCapture` fails with synthetic pointers (`cattura` of the prototype): the drag works without it. */
  private capture(event: PointerEvent, on: boolean): void {
    try {
      if (on) this.canvas.setPointerCapture(event.pointerId);
      else this.canvas.releasePointerCapture(event.pointerId);
    } catch {
      // A pointer that is not active.
    }
  }

  private pointerdown(event: PointerEvent): void {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    const { x, y } = this.local(event);
    this.capture(event, true);
    if (y > this.view.h) {
      this.drag = { kind: "tape" };
      this.tapeTo(x);
    } else if (Math.abs(x - this.playheadX()) <= 6 && y >= AXIS) {
      this.drag = { kind: "playhead" };
    } else {
      this.drag = { kind: "pan", x, y, moved: false, node: this.nodeAt(x, y) };
    }
  }

  private pointermove(event: PointerEvent): void {
    const { x, y } = this.local(event);
    const drag = this.drag;
    if (!drag) {
      const k = this.nodeAt(x, y);
      this.setHover(k);
      this.canvas.classList.toggle("on-node", k >= 0);
      this.canvas.classList.toggle("on-playhead", k < 0 && Math.abs(x - this.playheadX()) <= 6 && y >= AXIS && y <= this.view.h);
      return;
    }
    if (drag.kind === "tape") {
      this.tapeTo(x);
    } else if (drag.kind === "playhead" && this.lay) {
      this.setPlayhead(this.lay.tAt(toWorldX(this.camera, this.view, x)));
    } else if (drag.kind === "pan") {
      if (!drag.moved && Math.hypot(x - drag.x, y - drag.y) < 4) return;
      this.camera = panBy(this.camera, x - drag.x, y - drag.y, this.extent, this.view);
      drag.x = x;
      drag.y = y;
      drag.moved = true;
      this.canvas.classList.add("panning");
      this.render();
    }
  }

  private pointerup(event: PointerEvent): void {
    const drag = this.drag;
    this.drag = null;
    this.capture(event, false);
    this.canvas.classList.remove("panning");
    if (drag?.kind === "pan" && !drag.moved && event.type === "pointerup") {
      this.select(drag.node >= 0 && this.scene ? (this.scene.node[drag.node] as number) : -1);
    }
  }

  private tapeTo(x: number): void {
    const tape = this.tape;
    if (tape) this.setPlayhead(tape.scale.t(x), true);
  }

  private setHover(k: number): void {
    if (k === this.hover) return;
    this.hover = k;
    this.render();
  }

  private wheel(event: WheelEvent): void {
    event.preventDefault();
    const { x, y } = this.local(event);
    const across = event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY);
    if (across) {
      this.camera = panBy(this.camera, -(event.deltaX || event.deltaY), 0, this.extent, this.view);
    } else {
      // A pinch on a touchpad arrives as a wheel with Ctrl: it zooms faster.
      const factor = Math.exp(-event.deltaY * (event.ctrlKey ? 0.01 : 0.0015));
      this.camera = zoomAround(this.camera, x, Math.min(y, this.view.h), factor, this.extent, this.view);
    }
    this.render();
  }

  /** The arrows go from moment to moment, Up and Down to the nearest in the next lane with one; Home and End to the
   * first and the last; + and - zoom, 0 brings the camera home; Esc lets the selection go. */
  private keydown(event: KeyboardEvent): void {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const nodes = this.model.nodes;
    const done = () => event.preventDefault();
    switch (event.key) {
      case "ArrowRight":
        this.step(1);
        return done();
      case "ArrowLeft":
        this.step(-1);
        return done();
      case "ArrowUp":
      case "ArrowDown":
        this.acrossLanes(event.key === "ArrowUp" ? -1 : 1);
        return done();
      case "Home":
        if (nodes[0]) this.select(0);
        return done();
      case "End":
        if (nodes.length) this.select(nodes.length - 1);
        return done();
      case "+":
      case "=":
        this.zoom(ZOOM_STEP);
        return done();
      case "-":
        this.zoom(1 / ZOOM_STEP);
        return done();
      case "0":
        this.home();
        return done();
      case "Escape":
        if (this.selected >= 0) {
          this.select(-1);
          done();
        }
        return;
    }
  }

  /** From the selection (or the playhead) to the nearest node in time of the next lane, up or down, that has one. */
  private acrossLanes(direction: 1 | -1): void {
    const model = this.model;
    const from = model.nodes[this.selected];
    const t = from ? from.t : this.t;
    for (let lane = (from ? from.lane : direction > 0 ? -1 : model.lanes.length) + direction; lane >= 0 && lane < model.lanes.length; lane += direction) {
      const candidates = (model.lanes[lane]?.nodes ?? []).map((i) => model.nodes[i]).filter((n) => n !== undefined);
      if (candidates.length === 0) continue;
      const nearest = candidates.reduce((best, n) => (Math.abs(n.t - t) < Math.abs(best.t - t) ? n : best));
      this.select(nearest.index);
      return;
    }
  }
}
