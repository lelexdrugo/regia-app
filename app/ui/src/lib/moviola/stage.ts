// The moviola's stage: one Canvas the size of its host, at the screen's pixel density, drawing one project's
// timeline (canvas2d.ts) on demand, never in a loop. It holds the camera (a hand drags it, the wheel zooms it, a
// double click and 0 bring it home), the selection (a click finds the node with the grid's hitTest; the arrows go
// from moment to moment), the playhead (dragged on the stage or on the tape, moved by a selection, as the UX
// prototype does) and the colours, read again from the tokens when the direction on <html> changes: a new theme is
// a redraw, not a reload. The canvas carries what the proofs wait on: `data-frames`, the frames drawn, `data-dir`,
// the direction of the last one, `data-camera`, its camera, `data-view`, the stage above the tape, `data-groups`,
// at the zoom of groups (#489) the groups in view and the moments they hold, empty at the zoom of detail, and
// `data-world`, whether the last frame's world came from tiles or was drawn directly.
//
// The level of detail (#489): under lod.ts's zoom of groups each lane's milestones are drawn in groups, which the
// pointer finds as it finds a node, a click on one opening it at the zoom of detail; the world is blitted from tiles
// drawn once for the zoom (tiles.ts), dropped whenever the marks change; the labels go by rank and keep off the
// lanes' names.
//
// The list of moments (#490): the canvas is a listbox, and a window of the list (moments.ts, listbox.ts) is its own
// children, rebuilt when the moment chosen, the filters or the level of detail change. The arrows walk the list's
// order: time's at the zoom of detail, the groups' at the zoom of groups; PageUp and PageDown go ten moments, or a
// group, at a time.
import { clampCamera, type Extent, homeCamera, panBy, reveal, zoomAround } from "./camera";
import { AXIS, bucketByKind, type CanvasText, cut, drawFrame, drawStrip, type Frame, reservedPlaces, STRIP, type StripFrame, TAPE, type Tape } from "./canvas2d";
import { GROUP_ZOOM, groupAt, groupEnvelopes, type Groups, groupsAt, groupZoom, inView, size } from "./lod";
import { chooseWindow, dayStarts, type Envelope, halfThickness, HOUR, laneEnvelope, type Layout, layout, type Model, tapeScale, type TimeWindow } from "./model";
import { type ListMode, type ListText, MomentList } from "./listbox";
import { listOrder, nextEntry, type Order, PAGE } from "./moments";
import { readPalette, type Palette } from "./palette";
import { buildScene, type Camera, FILTERED, hitTest, labelRank, newLabels, pickLabels, type Scene, toScreenX, toScreenY, toWorldX, toWorldY, type View } from "./scene";
import { WorldTiles } from "./tiles";

export type StageOptions = {
  text: CanvasText;
  /** The days of this machine's calendar: the axis labels the first milestone of each. */
  dayKey: (t: number) => string;
  /** Called when the playhead or the selection moves. */
  onchange?: (stage: Stage) => void;
  /** The strip «time window» (#598), a canvas of its own above the stage: the whole story, the part in view, and the
   * gestures that choose a window. */
  strip?: HTMLCanvasElement;
  /** Called when the part of the story in view changes. */
  onview?: (view: ViewRange) => void;
  /** Called when the stage starts or stops drawing groups (#489), or the groups in view change: what the screen says
   * of them, `null` at the zoom of detail. */
  ongroups?: (shown: GroupsShown | null) => void;
  /** Off only for the harness's negative control: a stage that does not hear a change of direction. */
  observeTheme?: boolean;
  /** The words of the list of moments (#490), which the canvas holds as a listbox; none: no list. */
  list?: ListText;
  /** The list as the product has it, or one of the harness's negative controls (listbox.ts). */
  listMode?: ListMode;
};

/** The part of the story the stage shows: from one instant to another, or all of it. */
export type ViewRange = { from: number; to: number; whole: boolean };

/** At the zoom of groups, the groups in view and the moments they hold (#489). */
export type GroupsShown = { groups: number; moments: number };

type Drag =
  | { kind: "pan"; x: number; y: number; moved: boolean; node: number; group: number }
  | { kind: "playhead" }
  | { kind: "tape" };

/** What is under a point of the stage: a node, as a scene index, and at the zoom of groups the group; -1 for none. */
type Hit = { node: number; group: number };

const sameCamera = (a: Camera, b: Camera): boolean => Math.abs(a.cx - b.cx) < 1e-6 && Math.abs(a.cy - b.cy) < 1e-6 && Math.abs(a.z - b.z) < 1e-9;

/** A hand on the strip: a new window, one end of the window, or the window moved whole (`disegnaStriscia`). */
type StripDrag = { kind: "new" | "from" | "to" | "move"; x: number; a: number; b: number; moved: boolean };

const ZOOM_STEP = 1.25;

/**
 * The engines that draw the world part by part and from tiles, and a group of one from a sprite (#489): Chromium's,
 * where the bench measured them pay (WebView2 on Skia Ganesh: x10 dense from 38 % of frames late in pan to none).
 * WebKit's measured the other way on the CI's macOS runner, a frame that drew parts or tiles costing several times one
 * drawn in one pass (the dossier's report of #489), so there the world is drawn in one pass, as the bench of #488
 * measured it, and only the groups are new.
 */
const PARTS = typeof navigator !== "undefined" && /Chrome\/\d/.test(navigator.userAgent);

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
  /** The nodes the filters keep, by model index, or `null` for all; the lane chosen in the overview, or -1. */
  private kept: readonly boolean[] | null = null;
  private chosen = -1;
  private readonly strip: HTMLCanvasElement | null;
  private readonly stripCtx: CanvasRenderingContext2D | null;
  private stripFrame: StripFrame | null = null;
  private stripDrag: StripDrag | null = null;
  private lastView: ViewRange | null = null;
  /** The world in tiles for the zoom in use (#489), the groups of the last zoom under the zoom of groups with the
   * lanes' envelopes through them, the group under the pointer, and what the screen was last told of the groups. */
  private readonly tiles = new WorldTiles();
  private groups: Groups | null = null;
  private groupEnv: Envelope[] | null = null;
  private hoverGroup = -1;
  private lastGroups: string | null = null;
  /** The list of moments in the page (#490), and its order for the filters, the groups and the scene it was made for. */
  private readonly list: MomentList | null;
  private order: Order | null = null;
  private orderFor: [readonly boolean[] | null, Groups | null, Scene | null] = [null, null, null];

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
    this.strip = options.strip ?? null;
    this.stripCtx = this.strip ? this.strip.getContext("2d", { alpha: false }) : null;
    this.palette = readPalette();
    this.ready = new Promise((resolve) => (this.settle = resolve));
    this.list = options.list && options.listMode !== "none" ? new MomentList(canvas, model, options.list, options.listMode ?? "window") : null;

    const resized = new ResizeObserver(() => {
      this.resize();
      this.resizeStrip();
    });
    resized.observe(host);
    if (this.strip) resized.observe(this.strip);
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
    const strip = this.strip;
    if (strip) {
      const onStrip = <K extends keyof HTMLElementEventMap>(type: K, listener: (event: HTMLElementEventMap[K]) => void) => {
        strip.addEventListener(type, listener);
        this.cleanup.push(() => strip.removeEventListener(type, listener));
      };
      onStrip("pointerdown", (event) => this.stripDown(event));
      onStrip("pointermove", (event) => this.stripMove(event));
      onStrip("pointerup", (event) => this.stripUp(event));
      onStrip("pointercancel", (event) => this.stripUp(event));
      onStrip("dblclick", () => this.home());
    }
    this.resize();
    this.resizeStrip();
  }

  destroy(): void {
    if (this.pending) cancelAnimationFrame(this.pending);
    for (const undo of this.cleanup.splice(0)) undo();
    this.tiles.clear();
    if (this.list) {
      this.canvas.replaceChildren();
      this.canvas.removeAttribute("aria-activedescendant");
    }
  }

  /** The list's words in another language: the window is written again. */
  setListText(text: ListText): void {
    this.list?.setText(text);
    this.syncList();
  }

  /** The list's order now (moments.ts): the moments the filters keep, by time, or by group at the zoom of groups. */
  get listOrder(): Order {
    const groups = this.scene ? this.groupsFor(this.camera.z) : null;
    const [kept, cut, scene] = this.orderFor;
    if (this.order && kept === this.kept && cut === groups && scene === this.scene) return this.order;
    this.order = listOrder(this.model, this.scene ?? { node: new Uint32Array(0), kind: new Uint8Array(0) }, this.kept, groups);
    this.orderFor = [this.kept, groups, this.scene];
    return this.order;
  }

  /** The entry at the playhead: where the window stands while no moment is chosen, and where a page key starts. */
  private anchor(order: Order): number {
    let best = 0;
    let gap = Infinity;
    for (let e = 0; e < order.index.length; e++) {
      const d = Math.abs((this.model.nodes[order.index[e] as number]?.t ?? 0) - this.t);
      if (d < gap) {
        gap = d;
        best = e;
      }
    }
    return best;
  }

  /** The window of the list around the moment chosen, and the canvas naming its option. */
  private syncList(): void {
    if (!this.list) return;
    const order = this.listOrder;
    const active = this.list.render(order, this.selected, this.selected >= 0 ? -1 : this.anchor(order));
    if (active) this.canvas.setAttribute("aria-activedescendant", active);
    else this.canvas.removeAttribute("aria-activedescendant");
  }

  /** The words of the catalog in use: a new language is a redraw, the lanes' names measured again and the strip's
   * days written again. */
  setText(text: CanvasText): void {
    this.text = text;
    if (this.frame) {
      this.frame.text = text;
      this.frame.nameWidths = null;
    }
    if (this.stripFrame) {
      this.stripFrame.text = text;
      this.stripFrame.layer = null;
    }
    this.render();
  }

  /** The filters and the overview (#598): the nodes the filters keep (`null`: all), almost gone otherwise and out of
   * reach of the pointer, the labels and the keys; the lane chosen (-1: none), the others dimmed. */
  setEmphasis(kept: readonly boolean[] | null, lane: number): void {
    this.kept = kept;
    this.chosen = lane;
    this.applyEmphasis();
    this.render();
  }

  /** A node the filters keep. */
  shown(index: number): boolean {
    return this.kept === null || this.kept[index] === true;
  }

  private applyEmphasis(): void {
    const frame = this.frame;
    const scene = this.scene;
    if (!frame || !scene) return;
    // Another emphasis draws other marks: the tiles are drawn again.
    this.tiles.invalidate();
    frame.chosen = this.chosen;
    if (this.kept === null && this.chosen < 0) {
      frame.fade = null;
      return;
    }
    const fade = new Uint8Array(scene.n);
    for (let k = 0; k < scene.n; k++) {
      const index = scene.node[k] as number;
      const node = this.model.nodes[index];
      fade[k] = !this.shown(index) ? FILTERED : this.chosen >= 0 && node?.lane !== this.chosen ? 1 : 0;
    }
    frame.fade = fade;
  }

  /** The part of the story in view, as instants of the layout's time. */
  viewRange(): ViewRange | null {
    const lay = this.lay;
    if (!lay) return null;
    const from = lay.tAt(toWorldX(this.camera, this.view, 0));
    const to = lay.tAt(toWorldX(this.camera, this.view, this.view.w));
    return { from, to, whole: from <= this.model.t0 && to >= this.model.t1 };
  }

  /** A window of time on the stage (#598): its milestones fill the view, as the prototype's window fills the stage of
   * variant A, within the camera's zoom; none is the whole story, at rest. */
  showWindow(window: TimeWindow | null): void {
    const lay = this.lay;
    if (!window || !lay) {
      this.home();
      return;
    }
    const a = lay.xAt(window.from);
    const b = lay.xAt(window.to);
    const z = this.view.w / Math.max(1, b - a);
    this.camera = clampCamera({ cx: (a + b) / 2, cy: this.camera.cy, z }, this.extent, this.view);
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

  /** `saltaFotogramma`: the next or the previous milestone from the selection, or from the playhead, among those the
   * filters keep. */
  step(direction: 1 | -1): void {
    const order = this.listOrder;
    const at = this.selected >= 0 ? (order.place[this.selected] as number) : -1;
    if (at >= 0) {
      this.select(order.index[nextEntry(order, at, direction)] as number);
      return;
    }
    const nodes = this.model.nodes.filter((n) => this.shown(n.index));
    if (nodes.length === 0) return;
    const first = nodes[0] as Model["nodes"][number];
    const last = nodes[nodes.length - 1] as Model["nodes"][number];
    if (this.selected >= 0) {
      const from = this.selected;
      const next = direction > 0 ? nodes.find((n) => n.index > from) ?? last : [...nodes].reverse().find((n) => n.index < from) ?? first;
      this.select(next.index);
      return;
    }
    const next = direction > 0 ? nodes.find((n) => n.t > this.t + 1000) ?? last : [...nodes].reverse().find((n) => n.t < this.t - 1000) ?? first;
    this.select(next.index);
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
    const frame = this.frame;
    if (!frame) return;
    this.camera = camera;
    frame.cam = camera;
    frame.t = this.t;
    frame.selected = this.selected >= 0 ? (this.sceneOf[this.selected] as number) : -1;
    const groups = this.groupsFor(camera.z);
    frame.groups = groups;
    frame.groupEnvelopes = groups ? this.groupEnv : null;
    frame.hover = groups ? -1 : this.hover;
    frame.hoverGroup = groups ? this.hoverGroup : -1;
    pickLabels(frame.scene, camera, this.view, (k) => this.labelWidth(k), this.labels, frame.fade, (k) => this.rank(k), reservedPlaces(frame, this.ctx));
    drawFrame(this.ctx, frame, PARTS ? this.tiles : null);
    const strip = this.stripFrame;
    if (strip && this.stripCtx) {
      const range = this.viewRange();
      strip.band = range && !range.whole ? [strip.scale.x(range.from), strip.scale.x(range.to)] : null;
      drawStrip(this.stripCtx, strip);
    }
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
      this.canvas.dataset["world"] = this.tiles.blitted ? "tiles" : "direct";
      const range = this.viewRange();
      const last = this.lastView;
      if (range && (!last || last.from !== range.from || last.to !== range.to || last.whole !== range.whole)) {
        this.lastView = range;
        this.options.onview?.(range);
      }
      // What the screen says of the groups (#489): nothing is drawn in a group without its words.
      const groups = this.frame.groups;
      const shown = groups ? inView(groups, this.camera, this.view) : null;
      const said = shown ? `${shown.groups} ${shown.moments}` : "";
      this.canvas.dataset["groups"] = said;
      if (said !== this.lastGroups) {
        this.lastGroups = said;
        this.options.ongroups?.(shown);
      }
      this.syncList();
      if (this.fonts) this.settle();
    });
  }

  /** The groups at zoom `z` (lod.ts), with the lanes' envelopes through them; `null` at the zoom of detail. They are
   * cut at `groupZoom(z)`, so a zoom keeps them for a quarter of an octave instead of cutting them at every frame. */
  private groupsFor(z: number): Groups | null {
    const scene = this.scene;
    if (z >= GROUP_ZOOM || !scene) return null;
    const cut = groupZoom(z);
    if (this.groups?.z !== cut) {
      this.groups = groupsAt(scene, this.model, cut);
      this.groupEnv = groupEnvelopes(this.frame?.envelopes ?? [], this.groups, halfThickness);
      // The group under the pointer was one of the old ones.
      this.hoverGroup = -1;
    }
    return this.groups;
  }

  /** A label's rank: first a milestone at risk or waiting for the owner, then by kind (scene.ts's `labelRank`). */
  private rank(k: number): number {
    const scene = this.scene as Scene;
    const health = this.model.nodes[scene.node[k] as number]?.milestone.health;
    return labelRank(scene, k, health === "at_risk" || health === "waiting_on_owner");
  }

  private changed(): void {
    this.syncList();
    this.options.onchange?.(this);
    this.render();
  }

  private retheme(): void {
    this.palette = readPalette();
    this.widths.clear();
    // Every mark drawn once is in the old tokens: the tiles, the tape's layers, the strip's axis, the names' widths.
    this.tiles.invalidate();
    if (this.frame) {
      this.frame.palette = this.palette;
      this.frame.tapeLayers = null;
      this.frame.nameWidths = null;
      this.frame.sprites = null;
    }
    if (this.stripFrame) {
      this.stripFrame.palette = this.palette;
      this.stripFrame.layer = null;
    }
    this.render();
  }

  /** The strip at its own width and the screen's density, on the tape's scale with the prototype's narrower joints. */
  private resizeStrip(): void {
    const strip = this.strip;
    if (!strip) return;
    const w = strip.clientWidth;
    const dpr = devicePixelRatio || 1;
    if (w <= 16 || (this.stripFrame && this.stripFrame.width === w && this.stripFrame.dpr === dpr)) return;
    strip.width = Math.round(w * dpr);
    strip.height = Math.round(STRIP * dpr);
    const times = this.model.nodes.map((n) => n.t);
    this.stripFrame = {
      model: this.model,
      scale: tapeScale(times, 8, w - 8, 12 * HOUR, 18),
      x0: 8,
      x1: w - 8,
      width: w,
      dpr,
      palette: this.palette,
      text: this.text,
      days: dayStarts(this.model, this.options.dayKey),
      band: null,
      preview: null,
      layer: null,
    };
    this.render();
  }

  private stripX(event: MouseEvent): number {
    const s = this.stripFrame as StripFrame;
    const box = (this.strip as HTMLCanvasElement).getBoundingClientRect();
    return Math.min(s.x1, Math.max(s.x0, event.clientX - box.left));
  }

  private stripDown(event: PointerEvent): void {
    const s = this.stripFrame;
    if (!s || (event.pointerType === "mouse" && event.button !== 0)) return;
    const x = this.stripX(event);
    const band = s.band;
    const [a, b] = band ?? [s.x0, s.x1];
    const kind = band && Math.abs(x - a) <= 7 ? "from" : band && Math.abs(x - b) <= 7 ? "to" : band && x > a && x < b ? "move" : "new";
    this.stripDrag = { kind, x, a, b, moved: false };
    try {
      this.strip?.setPointerCapture(event.pointerId);
    } catch {
      // A pointer that is not active.
    }
  }

  private stripMove(event: PointerEvent): void {
    const g = this.stripDrag;
    const s = this.stripFrame;
    if (!g || !s) return;
    const x = this.stripX(event);
    let a = g.a;
    let b = g.b;
    if (g.kind === "from") a = x;
    else if (g.kind === "to") b = x;
    else if (g.kind === "move") {
      const d = Math.max(s.x0 - g.a, Math.min(s.x1 - g.b, x - g.x));
      a += d;
      b += d;
    } else {
      if (Math.abs(x - g.x) < 4) return;
      a = Math.min(x, g.x);
      b = Math.max(x, g.x);
    }
    g.moved = true;
    s.preview = [Math.min(a, b), Math.max(a, b)];
    this.render();
  }

  /** A window drawn on the strip becomes the stage's (`impostaFinestra`): the camera shows it. */
  private stripUp(event: PointerEvent): void {
    const g = this.stripDrag;
    this.stripDrag = null;
    try {
      this.strip?.releasePointerCapture(event.pointerId);
    } catch {
      // A pointer that is not active.
    }
    const s = this.stripFrame;
    const drawn = s?.preview ?? null;
    if (s) s.preview = null;
    if (!g || !g.moved || !s || !drawn) {
      this.render();
      return;
    }
    this.showWindow(chooseWindow(this.model, s.scale.t(drawn[0]), s.scale.t(drawn[1])));
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
    // A camera at rest stays at rest: the stage's top and left edges on the view's, as the screen settles.
    const resting = this.lay !== null && sameCamera(this.camera, homeCamera(this.extent, this.view));
    const before = this.lay && !resting ? { cam: this.camera, extent: this.extent } : null;
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
    // A new layout: no tile, no group and no group under the pointer holds.
    this.tiles.invalidate();
    this.groups = null;
    this.groupEnv = null;
    this.hoverGroup = -1;
    const extent = this.extent;
    if (before) {
      const sx = extent.w / Math.max(1, before.extent.w);
      const sy = extent.h / Math.max(1, before.extent.h);
      this.camera = clampCamera({ cx: before.cam.cx * sx, cy: before.cam.cy * sy, z: before.cam.z }, extent, this.view);
    } else {
      this.camera = homeCamera(extent, this.view);
    }
    const times = this.model.nodes.map((n) => n.t);
    const tape: Tape = { scale: tapeScale(times, 10, w - 10), x0: 10, x1: w - 10 };
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
      fade: null,
      chosen: -1,
      groups: null,
      groupEnvelopes: null,
      hoverGroup: -1,
      tapeLayers: null,
      nameWidths: null,
      sprites: null,
      parts: PARTS,
    };
    this.applyEmphasis();
    if (this.stripFrame && this.stripFrame.dpr !== dpr) {
      this.stripFrame = null;
      this.resizeStrip();
    }
    this.render();
  }

  private local(event: MouseEvent): { x: number; y: number } {
    const box = this.canvas.getBoundingClientRect();
    return { x: event.clientX - box.left, y: event.clientY - box.top };
  }

  /** The node under a screen point, as a scene index, or -1: within twelve pixels at any zoom. */
  private nodeAt(x: number, y: number): number {
    return this.pick(x, y).node;
  }

  /** What is under a screen point, within twelve pixels at any zoom: a node; at the zoom of groups (#489) the group,
   * and its one node when it has one. */
  private pick(x: number, y: number): Hit {
    const scene = this.scene;
    if (!scene || y < AXIS || y > this.view.h) return { node: -1, group: -1 };
    const radius = 12 / this.camera.z;
    const wx = toWorldX(this.camera, this.view, x);
    const wy = toWorldY(this.camera, this.view, y);
    const fade = this.frame?.fade ?? null;
    const groups = this.groupsFor(this.camera.z);
    if (!groups) return { node: hitTest(scene, wx, wy, radius, fade), group: -1 };
    const g = groupAt(groups, wx, wy, radius, fade);
    const alone = g >= 0 && size(groups, g) === 1 ? (groups.members[groups.start[g] as number] as number) : -1;
    return { node: alone, group: g };
  }

  /** A group of several moments clicked (#489): the camera goes to the zoom of detail, on the group. */
  private openGroup(g: number): void {
    const groups = this.groups;
    if (!groups || g < 0 || g >= groups.n) return;
    const cx = ((groups.x0[g] as number) + (groups.x1[g] as number)) / 2;
    this.camera = clampCamera({ cx, cy: groups.y[g] as number, z: GROUP_ZOOM * 1.25 }, this.extent, this.view);
    this.hoverGroup = -1;
    this.render();
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
      const hit = this.pick(x, y);
      this.drag = { kind: "pan", x, y, moved: false, node: hit.node, group: hit.group };
    }
  }

  private pointermove(event: PointerEvent): void {
    const { x, y } = this.local(event);
    const drag = this.drag;
    if (!drag) {
      const hit = this.pick(x, y);
      this.setHover(hit.node, hit.group);
      const on = hit.node >= 0 || hit.group >= 0;
      this.canvas.classList.toggle("on-node", on);
      this.canvas.classList.toggle("on-playhead", !on && Math.abs(x - this.playheadX()) <= 6 && y >= AXIS && y <= this.view.h);
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
      if (drag.node < 0 && drag.group >= 0) this.openGroup(drag.group);
      else this.select(drag.node >= 0 && this.scene ? (this.scene.node[drag.node] as number) : -1);
    }
  }

  private tapeTo(x: number): void {
    const tape = this.tape;
    if (tape) this.setPlayhead(tape.scale.t(x), true);
  }

  private setHover(k: number, group = -1): void {
    if (k === this.hover && group === this.hoverGroup) return;
    this.hover = k;
    this.hoverGroup = group;
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

  /** The arrows go from moment to moment in the list's order, Up and Down to the nearest in the next lane with one;
   * Home and End to the first and the last; PageUp and PageDown ten moments, or a group, back or on; + and - zoom, 0
   * brings the camera home; Esc lets the selection go. */
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
      case "Home": {
        const first = nodes.find((n) => this.shown(n.index));
        if (first) this.select(first.index);
        return done();
      }
      case "End": {
        const last = [...nodes].reverse().find((n) => this.shown(n.index));
        if (last) this.select(last.index);
        return done();
      }
      case "PageDown":
      case "PageUp": {
        const order = this.listOrder;
        const at = this.selected >= 0 ? (order.place[this.selected] as number) : -1;
        const e = nextEntry(order, at, event.key === "PageDown" ? PAGE : -PAGE, order.groups !== null, this.anchor(order));
        if (e >= 0) this.select(order.index[e] as number);
        return done();
      }
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
      const candidates = (model.lanes[lane]?.nodes ?? []).map((i) => model.nodes[i]).filter((n): n is Model["nodes"][number] => n !== undefined && this.shown(n.index));
      if (candidates.length === 0) continue;
      const nearest = candidates.reduce((best, n) => (Math.abs(n.t - t) < Math.abs(best.t - t) ? n : best));
      this.select(nearest.index);
      return;
    }
  }
}
