import { ImageCache, parseProps, type Look } from "./images.js";

/**
 * What the renderer needs from the layout engine. The wasm `Editor` from `opendox-wasm`
 * implements it; tests can supply a fake.
 */
export interface LayoutSource {
  /** Page size in points. */
  page_width(): number;
  page_height(): number;
  /** Glyphs on a page, flattened in groups of {@link GLYPH_STRIDE}. */
  page_glyphs(index: number): Float32Array;
  /** SVG path in font units, y down. `undefined` if the glyph is unknown. */
  glyph_path(font: number, face: number, glyph: number): string | undefined;
  units_per_em(font: number, face: number): number;
  /** Highlights, underlines and strikethroughs on a page, in groups of {@link DECORATION_STRIDE}. */
  page_decorations(index: number): Float32Array;
  /** Table borders on a page, in groups of {@link RULE_STRIDE}. */
  page_rules(index: number): Float32Array;
  /** Images on a page in drawing order, in groups of {@link OBJECT_STRIDE}. */
  page_objects(index: number): Float32Array;
  object_id(key: number): string | undefined;
  image_data(id: string): Uint8Array;
  image_mime(id: string): string;
  image_props(id: string): Float32Array | undefined;
}

/**
 * Floats per image: key, layer (0 behind, 1 text, 2 front), bounding box x, y, width, height,
 * drawn width, drawn height, rotation (degrees clockwise). Points.
 */
export const OBJECT_STRIDE = 9;

/**
 * Floats per glyph: font id, face index, glyph id, x, y, font size (positions in points),
 * colour (`0xRRGGBB`, or -1 for the default ink).
 */
export const GLYPH_STRIDE = 7;

/**
 * Floats per decoration: kind (0 highlight, 1 underline, 2 strikethrough), x, y, width,
 * height (a filled rectangle, points), colour as for glyphs. Highlights come first.
 */
export const DECORATION_STRIDE = 6;

/** Floats per border line: x0, y0, x1, y1 in points. */
export const RULE_STRIDE = 4;

/** Table border thickness in points (1px at 100% zoom). */
const RULE_WIDTH_PT = 0.75;

const INK = "#1d2129";

/** A layout colour (`0xRRGGBB`, or negative for the default ink) as CSS. */
function css(color: number): string {
  return color < 0 ? INK : `#${color.toString(16).padStart(6, "0")}`;
}

/** CSS pixels per point: CSS defines 1in = 96px = 72pt. */
const PX_PER_PT = 96 / 72;

export interface RenderStats {
  pages: number;
  /** Pages painted by this call (the ones on or near the screen); the rest paint when they
   * scroll into view. */
  drawn: number;
  glyphs: number;
  ms: number;
}

/** How far beyond the window (in window heights) pages are painted ahead of being seen, and
 * kept painted. Pages further away give their canvas memory back. */
const AHEAD = 1;

/**
 * Draws laid-out pages, one canvas per page.
 *
 * Glyphs are filled as vector paths at the exact positions the Rust layout computed, so the
 * browser's own text shaping never runs and every browser draws the same thing.
 *
 * Each page is a `.opendox-page` element holding the canvas and an empty `.opendox-overlay`
 * layer above it, where callers can place carets and selection boxes without redrawing text.
 *
 * Only pages on or near the screen are painted: every page keeps its size, the others are
 * marked stale and painted when they scroll near, so a keystroke in a long document costs a
 * few pages of drawing, not all of them. Pages far off screen release their canvas memory.
 */
export class PageRenderer {
  private readonly paths = new Map<string, Path2D | null>();
  private readonly emSizes = new Map<string, number>();
  private readonly pageEls: HTMLElement[] = [];
  private readonly images = new ImageCache(() => this.render(this.pageEls.length));
  /** Pages whose canvas doesn't show the current layout. */
  private readonly stale = new Set<number>();
  /** Pages on or near the screen, per the observer. */
  private readonly near = new Set<number>();
  private readonly observer: IntersectionObserver | null =
    typeof IntersectionObserver === "undefined"
      ? null
      : new IntersectionObserver((entries) => this.onVisibility(entries), {
          rootMargin: `${AHEAD * 100}% 0px`,
        });
  /** A look shown instead of an image's stored one, while a slider is being dragged. */
  lookOverride?: (id: string) => Partial<Look> | undefined;

  constructor(
    private readonly source: LayoutSource,
    private readonly container: HTMLElement,
    public zoom = 1,
  ) {}

  /** CSS pixels per point at the current zoom. */
  get cssPerPt(): number {
    return PX_PER_PT * this.zoom;
  }

  /** Page elements from the last render. */
  get pages(): readonly HTMLElement[] {
    return this.pageEls;
  }

  overlay(page: number): HTMLElement | undefined {
    return this.pageEls[page]?.querySelector<HTMLElement>(".opendox-overlay") ?? undefined;
  }

  /**
   * Lays out pages `0..pageCount` (reusing page elements from the previous call) and paints
   * the ones on or near the screen; the rest are painted when they come near.
   */
  render(pageCount: number): RenderStats {
    const start = performance.now();
    while (this.pageEls.length > pageCount) {
      const gone = this.pageEls.pop();
      if (gone) {
        this.observer?.unobserve(gone);
        gone.remove();
      }
      this.near.delete(this.pageEls.length);
      this.stale.delete(this.pageEls.length);
    }
    while (this.pageEls.length < pageCount) {
      const page = document.createElement("div");
      page.className = "opendox-page";
      page.dataset.page = String(this.pageEls.length);
      // White until painted, so a page scrolled to quickly never flashes the background.
      page.style.cssText = "position:relative;background:#fff;";
      const canvas = document.createElement("canvas");
      canvas.setAttribute("aria-hidden", "true");
      canvas.style.display = "block";
      const overlay = document.createElement("div");
      overlay.className = "opendox-overlay";
      overlay.style.cssText = "position:absolute;inset:0;pointer-events:none;";
      page.append(canvas, overlay);
      this.container.append(page);
      this.pageEls.push(page);
      this.observer?.observe(page);
    }
    // Every page keeps its size, painted or not, so scrolling and positions stay right.
    const width = `${this.source.page_width() * this.cssPerPt}px`;
    const height = `${this.source.page_height() * this.cssPerPt}px`;
    let glyphs = 0;
    let drawn = 0;
    this.pageEls.forEach((page, i) => {
      const canvas = page.querySelector("canvas")!;
      canvas.style.width = width;
      canvas.style.height = height;
      this.stale.add(i);
      if (this.onScreen(page)) {
        glyphs += this.drawPage(canvas, i);
        drawn++;
      }
    });
    return { pages: pageCount, drawn, glyphs, ms: performance.now() - start };
  }

  /** Paints every stale page now, wherever it is (before printing or a screenshot of all). */
  paintAll(): void {
    this.pageEls.forEach((page, i) => {
      if (this.stale.has(i)) this.drawPage(page.querySelector("canvas")!, i);
    });
  }

  /** Whether a page is within {@link AHEAD} window heights of the screen, measured now. */
  private onScreen(page: HTMLElement): boolean {
    if (this.near.has(Number(page.dataset.page))) return true;
    // The observer reports a frame late; measure, so a new page on screen paints right away.
    const r = page.getBoundingClientRect();
    const h = window.innerHeight || document.documentElement.clientHeight;
    return r.bottom >= -AHEAD * h && r.top <= h * (1 + AHEAD);
  }

  private onVisibility(entries: IntersectionObserverEntry[]): void {
    for (const e of entries) {
      const page = e.target as HTMLElement;
      const i = Number(page.dataset.page);
      if (e.isIntersecting) {
        this.near.add(i);
        if (this.stale.has(i)) this.drawPage(page.querySelector("canvas")!, i);
      } else {
        this.near.delete(i);
        // Far away: give the pixels back; it's painted again when it comes near.
        const canvas = page.querySelector("canvas")!;
        canvas.width = 0;
        canvas.height = 0;
        this.stale.add(i);
      }
    }
  }

  private drawPage(canvas: HTMLCanvasElement, index: number): number {
    this.stale.delete(index);
    const dpr = window.devicePixelRatio || 1;
    const widthPx = Math.round(this.source.page_width() * this.cssPerPt * dpr);
    const heightPx = Math.round(this.source.page_height() * this.cssPerPt * dpr);
    // Resizing clears and reallocates; only when the size really changed.
    if (canvas.width !== widthPx || canvas.height !== heightPx) {
      canvas.width = widthPx;
      canvas.height = heightPx;
    }

    const ctx = canvas.getContext("2d");
    if (!ctx) return 0;
    const devicePerPt = this.cssPerPt * dpr;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, widthPx, heightPx);
    const objects = this.source.page_objects(index);
    this.drawObjects(ctx, objects, devicePerPt, (layer) => layer === 0);
    this.drawRules(ctx, index, devicePerPt);
    this.drawObjects(ctx, objects, devicePerPt, (layer) => layer === 1);

    // Highlights go under the text, underlines and strikethroughs over it.
    const decorations = this.source.page_decorations(index);
    this.drawDecorations(ctx, decorations, devicePerPt, true);
    const data = this.source.page_glyphs(index);
    let color = NaN;
    for (let i = 0; i < data.length; i += GLYPH_STRIDE) {
      const font = data[i];
      const face = data[i + 1];
      const path = this.path(font, face, data[i + 2]);
      if (!path) continue;
      if (data[i + 6] !== color) {
        color = data[i + 6];
        ctx.fillStyle = css(color);
      }
      const k = (data[i + 5] / this.emSize(font, face)) * devicePerPt;
      ctx.setTransform(k, 0, 0, k, data[i + 3] * devicePerPt, data[i + 4] * devicePerPt);
      ctx.fill(path);
    }
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    this.drawDecorations(ctx, decorations, devicePerPt, false);
    this.drawObjects(ctx, objects, devicePerPt, (layer) => layer === 2);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    return data.length / GLYPH_STRIDE;
  }

  /** Images in the given layers: each rotated about the centre of its bounding box. */
  private drawObjects(
    ctx: CanvasRenderingContext2D,
    data: Float32Array,
    devicePerPt: number,
    layer: (l: number) => boolean,
  ): void {
    for (let i = 0; i < data.length; i += OBJECT_STRIDE) {
      if (!layer(data[i + 1])) continue;
      const id = this.source.object_id(data[i]);
      const raw = id ? this.source.image_props(id) : undefined;
      if (!id || !raw) continue;
      const p = { ...parseProps(raw), ...this.lookOverride?.(id) };
      const [x, y, bw, bh, w, h, rot] = [data[i + 2], data[i + 3], data[i + 4], data[i + 5], data[i + 6], data[i + 7], data[i + 8]];
      const k = devicePerPt;
      ctx.setTransform(k, 0, 0, k, (x + bw / 2) * k, (y + bh / 2) * k);
      ctx.rotate((rot * Math.PI) / 180);
      const picture = this.images.get(
        id,
        () => this.source.image_data(id),
        () => this.source.image_mime(id),
        p,
      );
      if (picture) {
        ctx.globalAlpha = Math.max(0, Math.min(1, 1 - p.transparency / 100));
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(picture, -w / 2, -h / 2, w, h);
        ctx.globalAlpha = 1;
      } else {
        // Still decoding: a light placeholder the size of the image.
        ctx.fillStyle = "#eef0f3";
        ctx.fillRect(-w / 2, -h / 2, w, h);
      }
    }
    ctx.setTransform(1, 0, 0, 1, 0, 0);
  }

  /** Highlights (`behind`) or the underlines and strikethroughs (otherwise). Lines are at
   * least one device pixel thick so they never vanish at small zoom. */
  private drawDecorations(
    ctx: CanvasRenderingContext2D,
    data: Float32Array,
    devicePerPt: number,
    behind: boolean,
  ): void {
    for (let i = 0; i < data.length; i += DECORATION_STRIDE) {
      if ((data[i] === 0) !== behind) continue;
      const k = devicePerPt;
      ctx.fillStyle = css(data[i + 5]);
      const h = behind ? data[i + 4] * k : Math.max(1, data[i + 4] * k);
      ctx.fillRect(data[i + 1] * k, data[i + 2] * k, data[i + 3] * k, h);
    }
  }

  /** Table borders, snapped to whole device pixels so thin lines stay crisp. */
  private drawRules(ctx: CanvasRenderingContext2D, index: number, devicePerPt: number): void {
    const rules = this.source.page_rules(index);
    if (rules.length === 0) return;
    const width = Math.max(1, Math.round(RULE_WIDTH_PT * devicePerPt));
    const snap = (v: number) => Math.round(v * devicePerPt) + (width % 2) / 2;
    ctx.strokeStyle = INK;
    ctx.lineWidth = width;
    ctx.beginPath();
    for (let i = 0; i < rules.length; i += RULE_STRIDE) {
      ctx.moveTo(snap(rules[i]), snap(rules[i + 1]));
      ctx.lineTo(snap(rules[i + 2]), snap(rules[i + 3]));
    }
    ctx.stroke();
  }

  private path(font: number, face: number, glyph: number): Path2D | null {
    const key = `${font}:${face}:${glyph}`;
    let path = this.paths.get(key);
    if (path === undefined) {
      const d = this.source.glyph_path(font, face, glyph);
      // Spaces have empty outlines; cache them as null so they're skipped cheaply.
      path = d ? new Path2D(d) : null;
      this.paths.set(key, path);
    }
    return path;
  }

  private emSize(font: number, face: number): number {
    const key = `${font}:${face}`;
    let size = this.emSizes.get(key);
    if (size === undefined) {
      size = this.source.units_per_em(font, face) || 1000;
      this.emSizes.set(key, size);
    }
    return size;
  }
}
