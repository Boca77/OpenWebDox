/**
 * Pictures: preparing files for insertion, and decoding + colour-treating them for drawing.
 *
 * The colour treatment mirrors `opendox_export::adjust` exactly (recolor, then brightness and
 * contrast), so the page looks the same as the exported PDF. Change both together.
 */

/** Image properties as `Editor.image_props` returns them, by name. */
export interface ImageProps {
  /** Points, before rotation. */
  width: number;
  height: number;
  /** Degrees clockwise. */
  rotation: number;
  wrap: WrapMode;
  /** Offset of a positioned image's bounding box from the left margin / paragraph top. */
  x: number;
  y: number;
  recolor: Recolor;
  brightness: number;
  contrast: number;
  transparency: number;
  pxWidth: number;
  pxHeight: number;
  /** Images in table cells are always laid out in line. */
  inTable: boolean;
}

export type WrapMode = "inline" | "wrap" | "break" | "behind" | "front";
export const WRAP_MODES: WrapMode[] = ["inline", "wrap", "break", "behind", "front"];
export type Recolor = "none" | "grayscale" | "sepia" | "negative";
export const RECOLORS: Recolor[] = ["none", "grayscale", "sepia", "negative"];

export function parseProps(v: Float32Array): ImageProps {
  return {
    width: v[0],
    height: v[1],
    rotation: v[2],
    wrap: WRAP_MODES[v[3]] ?? "inline",
    x: v[4],
    y: v[5],
    recolor: RECOLORS[v[6]] ?? "none",
    brightness: v[7],
    contrast: v[8],
    transparency: v[9],
    pxWidth: v[10],
    pxHeight: v[11],
    inTable: v[12] === 1,
  };
}

/** A file ready for `Editor.insert_image`. */
export interface PreparedImage {
  bytes: Uint8Array;
  mime: "image/png" | "image/jpeg";
  pxWidth: number;
  pxHeight: number;
}

/** Longest side kept, in pixels: enough for print at page width, small enough to sync. */
const MAX_SIDE = 2000;

/**
 * Turns any image file the browser can decode into a PNG (if it has transparency) or JPEG,
 * scaled down to {@link MAX_SIDE} pixels. Images are stored in the document and sent to every
 * collaborator, so keeping them bounded matters.
 */
export async function prepareImage(file: Blob): Promise<PreparedImage> {
  const bitmap = await createImageBitmap(file);
  const k = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  const w = Math.max(1, Math.round(bitmap.width * k));
  const h = Math.max(1, Math.round(bitmap.height * k));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("no 2d canvas");
  ctx.drawImage(bitmap, 0, 0, w, h);
  bitmap.close();
  const data = ctx.getImageData(0, 0, w, h).data;
  let opaque = true;
  for (let i = 3; i < data.length; i += 4) {
    if (data[i] < 255) {
      opaque = false;
      break;
    }
  }
  const mime = opaque ? "image/jpeg" : "image/png";
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, mime, 0.9));
  if (!blob) throw new Error("could not encode image");
  return { bytes: new Uint8Array(await blob.arrayBuffer()), mime, pxWidth: w, pxHeight: h };
}

/** One pixel's treatment, as in `opendox_export::adjust`. Values 0-255. */
export function adjust(
  r: number,
  g: number,
  b: number,
  recolor: Recolor,
  brightness: number,
  contrast: number,
): [number, number, number] {
  let [x, y, z] = [r / 255, g / 255, b / 255];
  if (recolor === "grayscale") {
    const l = 0.299 * x + 0.587 * y + 0.114 * z;
    [x, y, z] = [l, l, l];
  } else if (recolor === "sepia") {
    [x, y, z] = [
      0.393 * x + 0.769 * y + 0.189 * z,
      0.349 * x + 0.686 * y + 0.168 * z,
      0.272 * x + 0.534 * y + 0.131 * z,
    ];
  } else if (recolor === "negative") {
    [x, y, z] = [1 - x, 1 - y, 1 - z];
  }
  const k = 1 + contrast / 100;
  const shift = brightness / 200;
  const f = (v: number) => Math.round(Math.min(1, Math.max(0, (v - 0.5) * k + 0.5 + shift)) * 255);
  return [f(x), f(y), f(z)];
}

/** What a picture looks like: the parts of its properties that change pixels. */
export interface Look {
  recolor: Recolor;
  brightness: number;
  contrast: number;
}

interface Entry {
  original?: ImageBitmap;
  loading?: Promise<void>;
  treated?: { key: string; source: CanvasImageSource };
}

/**
 * Decoded pictures by image id. An id's bytes never change, so each is decoded once; a
 * treated copy is kept for the current look.
 */
export class ImageCache {
  private readonly entries = new Map<string, Entry>();

  /** Called when a picture finishes decoding, so the page can be drawn again. */
  constructor(private readonly onLoad: () => void) {}

  /** The picture ready to draw, or `undefined` while it's still decoding. */
  get(id: string, bytes: () => Uint8Array, mime: () => string, look: Look): CanvasImageSource | undefined {
    let e = this.entries.get(id);
    if (!e) {
      e = {};
      this.entries.set(id, e);
      const entry = e;
      entry.loading = createImageBitmap(new Blob([bytes() as BlobPart], { type: mime() }))
        .then((bmp) => {
          entry.original = bmp;
          this.onLoad();
        })
        .catch(() => {
          // An unreadable file draws as an empty frame rather than breaking the page.
        });
    }
    if (!e.original) return undefined;
    if (look.recolor === "none" && look.brightness === 0 && look.contrast === 0) return e.original;
    const key = `${look.recolor}|${look.brightness}|${look.contrast}`;
    if (e.treated?.key !== key) e.treated = { key, source: treat(e.original, look) };
    return e.treated.source;
  }
}

function treat(src: ImageBitmap, look: Look): CanvasImageSource {
  const c = document.createElement("canvas");
  c.width = src.width;
  c.height = src.height;
  const ctx = c.getContext("2d", { willReadFrequently: true });
  if (!ctx) return src;
  ctx.drawImage(src, 0, 0);
  const img = ctx.getImageData(0, 0, c.width, c.height);
  const d = img.data;
  for (let i = 0; i < d.length; i += 4) {
    const [r, g, b] = adjust(d[i], d[i + 1], d[i + 2], look.recolor, look.brightness, look.contrast);
    d[i] = r;
    d[i + 1] = g;
    d[i + 2] = b;
  }
  ctx.putImageData(img, 0, 0);
  return c;
}
