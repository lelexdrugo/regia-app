// The world in tiles (#489): the stage's lanes, edges and marks drawn once for a zoom, in squares of `CELL` CSS px
// on whole device pixels, and blitted on every frame the camera moves without zooming, a pan, the playhead followed,
// the pointer over the stage. On the machine measured for #489, blitting a square cost a fraction of drawing what is
// on it, so a frame stayed under the display's interval whatever the stage held (the bench of #488: x10 dense lost
// 38 % of its frames in pan as the renderer drew it before, and none with the tiles). A new zoom draws its first frame directly (canvas2d.ts's `drawWorldDirect`) and the next frames at that
// zoom draw a few tiles each, until all those in view are there: while one is missing the frame draws directly.
// Anything that changes the marks, a direction, a filter, the stage's size or density, drops them all (`invalidate`).
import { CELL, drawWorld, type Frame, type WorldSource, worldMargin } from "./canvas2d";

/** New tiles a frame draws at most: a few of the 24 in view, so a frame never draws much more than the stage. */
const BUDGET = 4;
/** Tiles kept, the last used first: one and a half stages. */
const KEEP = 36;

type Tile = { canvas: HTMLCanvasElement; used: number };

export class WorldTiles implements WorldSource {
  /** Whether the last frame's world came from tiles: the stage tells the proofs (`data-world`). */
  blitted = false;
  private readonly tiles = new Map<string, Tile>();
  private readonly spare: HTMLCanvasElement[] = [];
  private key = "";
  private clock = 0;
  private generation = 0;

  /** The marks changed: no tile is good any more. */
  invalidate(): void {
    this.generation += 1;
  }

  /** The stage is gone: the tiles let go of their canvases now, not when the collector comes. */
  clear(): void {
    this.drop();
    this.spare.length = 0;
    this.key = "";
  }

  draw(ctx: CanvasRenderingContext2D, f: Frame): boolean {
    this.blitted = false;
    const { cam, view, dpr } = f;
    const s = dpr * cam.z;
    const key = `${this.generation} ${dpr} ${cam.z}`;
    if (key !== this.key) {
      // Another zoom, or other marks: this frame draws directly, and tiles start at the next one with this key.
      this.key = key;
      this.drop();
      return false;
    }
    const step = Math.max(1, Math.round(CELL * dpr));
    // Where the world's origin lands on the stage, in device pixels: whole, so tiles meet without a seam.
    const ox = Math.round((view.w / 2) * dpr - cam.cx * s);
    const oy = Math.round((view.h / 2) * dpr - cam.cy * s);
    const ix0 = Math.floor(-ox / step);
    const ix1 = Math.floor((Math.round(view.w * dpr) - 1 - ox) / step);
    const iy0 = Math.floor(-oy / step);
    const iy1 = Math.floor((Math.round(view.h * dpr) - 1 - oy) / step);
    this.clock += 1;
    let budget = BUDGET;
    let complete = true;
    for (let iy = iy0; iy <= iy1; iy++) {
      for (let ix = ix0; ix <= ix1; ix++) {
        const id = `${ix} ${iy}`;
        let tile = this.tiles.get(id);
        if (!tile && budget > 0) {
          tile = { canvas: this.paint(f, ix, iy, step, s), used: 0 };
          this.tiles.set(id, tile);
          budget -= 1;
        }
        if (tile) tile.used = this.clock;
        else complete = false;
      }
    }
    this.evict();
    if (!complete) return false;
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    for (let iy = iy0; iy <= iy1; iy++) {
      for (let ix = ix0; ix <= ix1; ix++) {
        const tile = this.tiles.get(`${ix} ${iy}`) as Tile;
        ctx.drawImage(tile.canvas, ox + ix * step, oy + iy * step);
      }
    }
    ctx.restore();
    this.blitted = true;
    return true;
  }

  /** One tile: the world from `ix * step` to `(ix + 1) * step` device pixels of the world at this zoom, and down. */
  private paint(f: Frame, ix: number, iy: number, step: number, s: number): HTMLCanvasElement {
    const canvas = this.spare.pop() ?? document.createElement("canvas");
    if (canvas.width !== step || canvas.height !== step) {
      canvas.width = step;
      canvas.height = step;
    }
    const ctx = canvas.getContext("2d") as CanvasRenderingContext2D;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, step, step);
    ctx.save();
    ctx.setTransform(s, 0, 0, s, -ix * step, -iy * step);
    const [mx, my] = worldMargin(f);
    drawWorld(ctx, f, (ix * step) / s - mx, ((ix + 1) * step) / s + mx, (iy * step) / s - my, ((iy + 1) * step) / s + my);
    ctx.restore();
    return canvas;
  }

  private evict(): void {
    if (this.tiles.size <= KEEP) return;
    const old = [...this.tiles.entries()].sort((a, b) => a[1].used - b[1].used).slice(0, this.tiles.size - KEEP);
    for (const [id, tile] of old) {
      this.tiles.delete(id);
      if (this.spare.length < BUDGET) this.spare.push(tile.canvas);
    }
  }

  private drop(): void {
    for (const tile of this.tiles.values()) if (this.spare.length < BUDGET) this.spare.push(tile.canvas);
    this.tiles.clear();
  }
}
