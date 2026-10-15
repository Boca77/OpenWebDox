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
}

/** Floats per glyph: font id, face index, glyph id, x, y, font size (positions in points). */
export const GLYPH_STRIDE = 6;

/** CSS pixels per point: CSS defines 1in = 96px = 72pt. */
const PX_PER_PT = 96 / 72;

export interface RenderStats {
  pages: number;
  glyphs: number;
  ms: number;
}

/**
 * Draws laid-out pages onto one canvas per page.
 *
 * Glyphs are filled as vector paths at the exact positions the Rust layout computed, so the
 * browser's own text shaping never runs and every browser draws the same thing.
 */
export class PageRenderer {
  private readonly paths = new Map<string, Path2D | null>();
  private readonly emSizes = new Map<string, number>();
  private readonly canvases: HTMLCanvasElement[] = [];

  constructor(
    private readonly source: LayoutSource,
    private readonly container: HTMLElement,
    public zoom = 1,
  ) {}

  /** Draws pages `0..pageCount`, reusing canvases from the previous call. */
  render(pageCount: number): RenderStats {
    const start = performance.now();
    while (this.canvases.length > pageCount) {
      this.canvases.pop()?.remove();
    }
    while (this.canvases.length < pageCount) {
      const canvas = document.createElement("canvas");
      canvas.className = "opendox-page";
      canvas.setAttribute("aria-hidden", "true");
      this.container.append(canvas);
      this.canvases.push(canvas);
    }
    let glyphs = 0;
    this.canvases.forEach((canvas, i) => {
      glyphs += this.drawPage(canvas, i);
    });
    return { pages: pageCount, glyphs, ms: performance.now() - start };
  }

  private drawPage(canvas: HTMLCanvasElement, index: number): number {
    const dpr = window.devicePixelRatio || 1;
    const cssPerPt = PX_PER_PT * this.zoom;
    const widthPx = Math.round(this.source.page_width() * cssPerPt * dpr);
    const heightPx = Math.round(this.source.page_height() * cssPerPt * dpr);
    canvas.width = widthPx;
    canvas.height = heightPx;
    canvas.style.width = `${widthPx / dpr}px`;
    canvas.style.height = `${heightPx / dpr}px`;

    const ctx = canvas.getContext("2d");
    if (!ctx) return 0;
    const devicePerPt = cssPerPt * dpr;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, widthPx, heightPx);
    ctx.fillStyle = "#1d2129";

    const data = this.source.page_glyphs(index);
    for (let i = 0; i < data.length; i += GLYPH_STRIDE) {
      const font = data[i];
      const face = data[i + 1];
      const path = this.path(font, face, data[i + 2]);
      if (!path) continue;
      const k = (data[i + 5] / this.emSize(font, face)) * devicePerPt;
      ctx.setTransform(k, 0, 0, k, data[i + 3] * devicePerPt, data[i + 4] * devicePerPt);
      ctx.fill(path);
    }
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    return data.length / GLYPH_STRIDE;
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
