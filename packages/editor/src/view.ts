import { OBJECT_STRIDE, PageRenderer, type LayoutSource, type RenderStats } from "./renderer.js";
import {
  parseProps,
  prepareImage,
  RECOLORS,
  WRAP_MODES,
  type ImageProps,
  type Look,
  type PreparedImage,
  type Recolor,
  type WrapMode,
} from "./images.js";

/**
 * Everything the view needs from the engine. The wasm `Editor` implements it. Positions are
 * flat positions: one number per caret position, body text and table cells together in
 * reading order.
 */
export interface EditorEngine extends LayoutSource {
  layout(): number;
  len(): number;
  slice(start: number, end: number): string;
  /** Returns the caret position after the inserted text. */
  insert(pos: number, text: string): number;
  /** Deletes a range; returns the caret position. */
  delete(start: number, end: number): number;
  delete_backward(pos: number): number;
  delete_forward(pos: number): number;
  move_left(pos: number): number;
  move_right(pos: number): number;
  /** `[pos, upstream]` */
  hit_test(page: number, x: number, y: number): Uint32Array | undefined;
  /** `[page, x, top, bottom]` in points */
  caret_rect(pos: number, upstream: boolean): Float32Array | undefined;
  move_vertical(pos: number, upstream: boolean, goalX: number, down: boolean): Uint32Array | undefined;
  line_start(pos: number, upstream: boolean): Uint32Array | undefined;
  line_end(pos: number, upstream: boolean): Uint32Array | undefined;
  /** Groups of `page, x, y, width, height` in points */
  selection_rects(a: number, b: number): Float32Array;
  insert_table(pos: number, rows: number, cols: number): number;
  /** `[row, col, rows, cols]` */
  table_info(pos: number): Uint32Array | undefined;
  next_cell(pos: number): number | undefined;
  prev_cell(pos: number): number | undefined;
  table_insert_row(pos: number, below: boolean): number;
  table_insert_column(pos: number, right: boolean): number;
  table_delete_row(pos: number): number;
  table_delete_column(pos: number): number;
  table_delete(pos: number): number;
  set_bold(anchor: number, focus: number, on: boolean): void;
  set_italic(anchor: number, focus: number, on: boolean): void;
  /** Points; 0 = default size. */
  set_font_size(anchor: number, focus: number, size: number): void;
  /** Family name; "" = default font. */
  set_font(anchor: number, focus: number, family: string): void;
  /** `[bold, italic, size]`, size 0 = default */
  char_format(anchor: number, focus: number): Float32Array;
  char_font(anchor: number, focus: number): string;
  /** 0 left, 1 centre, 2 right, 3 justify */
  set_align(anchor: number, focus: number, align: number): void;
  set_line_spacing(anchor: number, focus: number, spacing: number): void;
  set_space_before(anchor: number, focus: number, pt: number): void;
  set_space_after(anchor: number, focus: number, pt: number): void;
  /** `[align, line spacing, space before, space after]` */
  paragraph_format(pos: number): Float32Array;
  /** The selection to restore as `[anchor, focus]`, or undefined if there was nothing. */
  undo(anchor: number, focus: number): Uint32Array | undefined;
  redo(anchor: number, focus: number): Uint32Array | undefined;
  can_undo(): boolean;
  can_redo(): boolean;
  clear_history(): void;
  /** Returns the new image's id. */
  insert_image(
    pos: number,
    data: Uint8Array,
    mime: string,
    pxWidth: number,
    pxHeight: number,
    width: number,
    height: number,
  ): string;
  image_alt(id: string): string;
  image_position(id: string): number | undefined;
  set_image_size(id: string, width: number, height: number): void;
  set_image_rotation(id: string, degrees: number): void;
  /** 0 in line, 1 wrap, 2 break, 3 behind, 4 in front. */
  set_image_wrap(id: string, wrap: number): void;
  set_image_offset(id: string, x: number, y: number): void;
  set_image_geometry(id: string, width: number, height: number, rotation: number, x: number, y: number): void;
  /** 0 none, 1 grayscale, 2 sepia, 3 negative. */
  set_image_recolor(id: string, recolor: number): void;
  set_image_adjustments(id: string, brightness: number, contrast: number, transparency: number): void;
  set_image_alt(id: string, alt: string): void;
  place_image(id: string, page: number, x: number, y: number): void;
  /** Returns where the caret goes. */
  delete_image(id: string): number;
  is_over_text(page: number, x: number, y: number): boolean;
}

/** Where an image is drawn: page and bounding box in points, its drawn size and rotation. */
interface PlacedImage {
  page: number;
  x: number;
  y: number;
  bw: number;
  bh: number;
  w: number;
  h: number;
  rotation: number;
  layer: number;
}

/** A handle on the selected image: the side or corner it moves (-1, 0, 1 per axis). */
type Handle = { dx: number; dy: number } | "rotate";

export type Alignment = "left" | "center" | "right" | "justify";
const ALIGNMENTS: Alignment[] = ["left", "center", "right", "justify"];

/** Formatting at the caret or selection, for toolbars. */
export interface FormatState {
  bold: boolean;
  italic: boolean;
  /** Points; `null` = the document default. */
  size: number | null;
  /** Family name; `null` = the document default. */
  font: string | null;
  align: Alignment;
  lineSpacing: number;
  spaceBefore: number;
  spaceAfter: number;
  canUndo: boolean;
  canRedo: boolean;
}

export interface EditStats {
  layoutMs: number;
  render: RenderStats;
}

/** Where the caret is in a table. */
export interface TableInfo {
  row: number;
  col: number;
  rows: number;
  cols: number;
}

/** Another person's selection, as positions per the last layout. */
export interface RemoteSelection {
  name: string;
  /** CSS colour; dark enough for white text, since it is also the name flag's background. */
  color: string;
  anchor: number;
  focus: number;
}

export type TableCommand =
  | "insertRowAbove"
  | "insertRowBelow"
  | "insertColumnLeft"
  | "insertColumnRight"
  | "deleteRow"
  | "deleteColumn"
  | "deleteTable";

const RECT_STRIDE = 5;

const STYLE_ID = "opendox-editor-style";
const STYLES = `
.opendox-editor { position: relative; cursor: text; }
.opendox-caret {
  position: absolute; width: 1.5px; background: #1d2129;
  animation: opendox-blink 1.06s steps(1) infinite;
}
@keyframes opendox-blink { 50% { opacity: 0; } }
@media (prefers-reduced-motion: reduce) { .opendox-caret { animation: none; } }
.opendox-editor:not(.opendox-focused) .opendox-caret { display: none; }
.opendox-selection { position: absolute; background: rgb(47 91 211 / 0.28); }
.opendox-editor:not(.opendox-focused) .opendox-selection { background: rgb(74 81 92 / 0.18); }
.opendox-remote-caret { position: absolute; width: 2px; margin-left: -1px; }
.opendox-remote-name {
  position: absolute; bottom: 100%; left: 0; padding: 0 4px; border-radius: 2px 2px 2px 0;
  color: #fff; font: 600 11px/16px system-ui, sans-serif; white-space: nowrap;
}
.opendox-remote-selection { position: absolute; }
.opendox-input {
  position: absolute; z-index: 2; margin: 0; padding: 0; border: 0; outline: none;
  resize: none; overflow: hidden; white-space: pre; opacity: 0; width: 2px;
  color: #1d2129; background: transparent; font-family: inherit; line-height: 1;
}
.opendox-image-frame {
  position: absolute; box-sizing: border-box; border: 1.5px solid #2f5bd3;
  pointer-events: auto; cursor: move; touch-action: none;
}
.opendox-image-frame.opendox-inline { cursor: default; }
.opendox-handle {
  position: absolute; width: 10px; height: 10px; margin: -5px 0 0 -5px; box-sizing: border-box;
  background: #fff; border: 1.5px solid #2f5bd3; pointer-events: auto; touch-action: none;
}
.opendox-handle::before { content: ""; position: absolute; inset: -17px; }
.opendox-handle.opendox-rotate { border-radius: 50%; cursor: grab; }
.opendox-stem { position: absolute; left: 50%; width: 1.5px; height: 20px; top: -20px; margin-left: -0.75px; background: #2f5bd3; }
.opendox-editor.opendox-image-selected .opendox-caret { display: none; }
.opendox-input.opendox-composing {
  opacity: 1; width: auto; min-width: 2em; field-sizing: content; background: #fff;
  box-shadow: 0 0 0 1px #c9cdd4; text-decoration: underline;
}
`;

/**
 * An editable, paginated document view.
 *
 * Text is drawn by {@link PageRenderer}. Keyboard input goes through a hidden textarea placed
 * at the caret, which is what lets IMEs (Chinese, Japanese, Korean input) and mobile keyboards
 * work and puts their candidate windows next to the caret. While an IME is composing, the
 * textarea becomes visible over the caret to show the not-yet-committed text.
 */
export class EditorView {
  readonly renderer: PageRenderer;
  /** Called after every edit with timings. */
  onUpdate?: (stats: EditStats) => void;
  /** Called whenever the caret or selection moves. */
  onSelectionChange?: () => void;
  /** Other people's selections, drawn with the local one. Set by `Collaboration`. */
  remoteSelections?: () => RemoteSelection[];

  private readonly editListeners: (() => void)[] = [];
  private readonly selectionListeners: (() => void)[] = [];
  private readonly input: HTMLTextAreaElement;
  private anchor = 0;
  private focus = 0;
  private upstream = false;
  /** Column (points) that Up/Down aim for, kept across consecutive vertical moves. */
  private goalX: number | null = null;
  private composing = false;
  private dragging = false;
  /** Id of the selected image, if one is selected instead of text. */
  private image: string | null = null;
  /** An image move, resize or turn in progress, shown as a frame until it's dropped. */
  private gesture: {
    kind: "move" | Handle;
    id: string;
    start: PlacedImage;
    pointerX: number;
    pointerY: number;
    moved: boolean;
    preview: { cx: number; cy: number; w: number; h: number; rotation: number; page: number };
    shift: boolean;
  } | null = null;

  constructor(
    private readonly engine: EditorEngine,
    private readonly container: HTMLElement,
  ) {
    if (!document.getElementById(STYLE_ID)) {
      const style = document.createElement("style");
      style.id = STYLE_ID;
      style.textContent = STYLES;
      document.head.append(style);
    }
    container.classList.add("opendox-editor");
    this.renderer = new PageRenderer(engine, container);

    this.input = document.createElement("textarea");
    this.input.className = "opendox-input";
    this.input.setAttribute("aria-label", "Document text");
    this.input.setAttribute("autocapitalize", "off");
    this.input.setAttribute("autocomplete", "off");
    this.input.spellcheck = false;
    container.append(this.input);

    container.addEventListener("pointerdown", (e) => this.onPointerDown(e));
    container.addEventListener("pointermove", (e) => this.onPointerMove(e));
    container.addEventListener("pointerup", (e) => this.onPointerUp(e));
    container.addEventListener("pointercancel", () => {
      this.dragging = false;
      this.gesture = null;
      this.drawSelection();
    });
    container.addEventListener("dragover", (e) => {
      if (e.dataTransfer?.types.includes("Files")) e.preventDefault();
    });
    container.addEventListener("drop", (e) => this.onDrop(e));
    container.addEventListener("contextmenu", (e) => this.onContextMenu(e));
    this.input.addEventListener("keydown", (e) => this.onKeyDown(e));
    this.input.addEventListener("input", () => this.onInput());
    this.input.addEventListener("compositionstart", () => this.onCompositionStart());
    this.input.addEventListener("compositionend", () => this.onCompositionEnd());
    this.input.addEventListener("paste", (e) => this.onPaste(e));
    this.input.addEventListener("copy", (e) => this.onCopy(e, false));
    this.input.addEventListener("cut", (e) => this.onCopy(e, true));
    this.input.addEventListener("focus", () => container.classList.add("opendox-focused"));
    this.input.addEventListener("blur", () => container.classList.remove("opendox-focused"));

    this.relayout();
  }

  /** Current selection; `anchor === focus` when it's just a caret. */
  get selection(): { anchor: number; focus: number } {
    return { anchor: this.anchor, focus: this.focus };
  }

  /** Listens for local edits (`"edit"`) or caret/selection moves (`"selection"`). */
  on(event: "edit" | "selection", listener: () => void): void {
    (event === "edit" ? this.editListeners : this.selectionListeners).push(listener);
  }

  /**
   * Applies a change made elsewhere (a remote edit): moves the selection to where the engine
   * remapped it and redraws. Pass the current selection to only redraw remote carets.
   */
  applyRemote(anchor: number, focus: number, docChanged: boolean): void {
    this.anchor = anchor;
    this.focus = focus;
    if (docChanged) this.relayout();
    else this.drawSelection();
  }

  focusEditor(): void {
    this.input.focus({ preventScroll: true });
  }

  setZoom(zoom: number): void {
    this.renderer.zoom = zoom;
    this.relayout();
  }

  /** Inserts text at the caret, replacing the selection. */
  insertText(text: string): void {
    const clean = text.replace(/\r\n?/g, "\n");
    if (!clean) return;
    const at = this.deleteSelectionRaw();
    const after = this.engine.insert(at, clean);
    this.collapseTo(after, false);
    this.edited();
  }

  /** Inserts a table at the caret and puts the caret in its first cell. */
  insertTable(rows: number, cols: number): void {
    const at = this.deleteSelectionRaw();
    const cell = this.engine.insert_table(at, rows, cols);
    this.collapseTo(cell, false);
    this.edited();
  }

  /** Where the caret is in a table, or `undefined` outside tables. */
  tableInfo(): TableInfo | undefined {
    const t = this.engine.table_info(this.focus);
    return t ? { row: t[0], col: t[1], rows: t[2], cols: t[3] } : undefined;
  }

  /** Runs a table command for the table holding the caret. */
  tableCommand(command: TableCommand): void {
    if (!this.tableInfo()) return;
    const e = this.engine;
    const at = this.focus;
    const to = {
      insertRowAbove: () => e.table_insert_row(at, false),
      insertRowBelow: () => e.table_insert_row(at, true),
      insertColumnLeft: () => e.table_insert_column(at, false),
      insertColumnRight: () => e.table_insert_column(at, true),
      deleteRow: () => e.table_delete_row(at),
      deleteColumn: () => e.table_delete_column(at),
      deleteTable: () => e.table_delete(at),
    }[command]();
    this.collapseTo(to, false);
    this.edited();
  }

  // --- images ----------------------------------------------------------------------------

  /** The selected image's id, or `null` when text is selected. */
  get selectedImage(): string | null {
    return this.image;
  }

  /** An image's properties, or `undefined` if it isn't in the document. */
  imageProps(id = this.image): ImageProps | undefined {
    const raw = id ? this.engine.image_props(id) : undefined;
    return raw ? parseProps(raw) : undefined;
  }

  imageAlt(id = this.image): string {
    return id ? this.engine.image_alt(id) : "";
  }

  /** Inserts a picture at the caret (replacing the selection) and selects it. */
  insertImage(img: PreparedImage): void {
    const at = this.deleteSelectionRaw();
    // Pixels at 96 dpi, kept within the text width.
    const maxWidth = this.engine.page_width() - 144;
    let w = (img.pxWidth * 72) / 96;
    let h = (img.pxHeight * 72) / 96;
    if (w > maxWidth) [w, h] = [maxWidth, (h * maxWidth) / w];
    const id = this.engine.insert_image(at, img.bytes, img.mime, img.pxWidth, img.pxHeight, w, h);
    this.collapseTo(at + 1, false);
    this.image = id;
    this.edited();
  }

  /** Reads an image file (any type the browser can decode) and inserts it at the caret. */
  async insertImageFile(file: Blob): Promise<void> {
    this.insertImage(await prepareImage(file));
  }

  /** Selects an image (or, with `null`, goes back to the text caret). */
  selectImage(id: string | null): void {
    this.image = id;
    if (id) {
      const at = this.engine.image_position(id);
      if (at !== undefined) this.collapseTo(at + 1, false);
    }
    this.drawSelection();
  }

  setImageWrap(mode: WrapMode): void {
    this.imageEdit((id) => this.engine.set_image_wrap(id, WRAP_MODES.indexOf(mode)));
  }

  /** Points, before rotation. */
  setImageSize(width: number, height: number): void {
    this.imageEdit((id) => this.engine.set_image_size(id, Math.max(1, width), Math.max(1, height)));
  }

  /** Degrees clockwise. */
  setImageRotation(degrees: number): void {
    this.imageEdit((id) => this.engine.set_image_rotation(id, degrees));
  }

  /** Offset of a positioned image from the left margin and its paragraph's top, in points. */
  setImageOffset(x: number, y: number): void {
    this.imageEdit((id) => this.engine.set_image_offset(id, x, y));
  }

  setImageRecolor(recolor: Recolor): void {
    this.imageEdit((id) => this.engine.set_image_recolor(id, RECOLORS.indexOf(recolor)));
  }

  setImageAdjustments(brightness: number, contrast: number, transparency: number): void {
    this.imageEdit((id) => this.engine.set_image_adjustments(id, brightness, contrast, transparency));
  }

  setImageAlt(alt: string): void {
    this.imageEdit((id) => this.engine.set_image_alt(id, alt));
  }

  /** Shows the selected image with a different look without changing the document (while a
   * slider is dragged); `null` ends the preview. */
  previewImageLook(look: Partial<Look> & { transparency?: number } | null): void {
    const id = this.image;
    this.renderer.lookOverride = look && id ? (i) => (i === id ? look : undefined) : undefined;
    // Pages only: the selection didn't change, and telling listeners would make toolbars
    // re-read the stored look over the one being previewed.
    this.renderer.render(this.renderer.pages.length);
  }

  deleteImage(): void {
    if (!this.image) return;
    const at = this.engine.delete_image(this.image);
    this.image = null;
    this.collapseTo(at, false);
    this.edited();
  }

  /** The selected image's bounding box in client (viewport) pixels, for placing toolbars. */
  imageClientRect(): DOMRect | undefined {
    const p = this.image ? this.placed(this.image) : undefined;
    const pageEl = p ? this.renderer.pages[p.page] : undefined;
    if (!p || !pageEl) return undefined;
    const r = pageEl.getBoundingClientRect();
    const k = this.renderer.cssPerPt;
    return new DOMRect(r.left + p.x * k, r.top + p.y * k, p.bw * k, p.bh * k);
  }

  private imageEdit(apply: (id: string) => void): void {
    if (!this.image) return;
    this.renderer.lookOverride = undefined;
    apply(this.image);
    this.edited();
  }

  /** Where the last layout drew image `id`. */
  private placed(id: string): PlacedImage | undefined {
    for (let page = 0; page < this.renderer.pages.length; page++) {
      const d = this.engine.page_objects(page);
      for (let i = 0; i < d.length; i += OBJECT_STRIDE) {
        if (this.engine.object_id(d[i]) !== id) continue;
        return {
          page,
          layer: d[i + 1],
          x: d[i + 2],
          y: d[i + 3],
          bw: d[i + 4],
          bh: d[i + 5],
          w: d[i + 6],
          h: d[i + 7],
          rotation: d[i + 8],
        };
      }
    }
    return undefined;
  }

  /** The image under page point `(x, y)`: in front first, then among the text, then behind
   * it (only where there's no text over it). */
  private imageAt(page: number, x: number, y: number): string | undefined {
    const d = this.engine.page_objects(page);
    let found: { id: string; layer: number } | undefined;
    for (let i = 0; i < d.length; i += OBJECT_STRIDE) {
      const [bx, by, bw, bh, w, h, rot] = [d[i + 2], d[i + 3], d[i + 4], d[i + 5], d[i + 6], d[i + 7], d[i + 8]];
      // Into the image's own (unrotated) frame.
      const t = (-rot * Math.PI) / 180;
      const [px, py] = [x - (bx + bw / 2), y - (by + bh / 2)];
      const lx = px * Math.cos(t) - py * Math.sin(t);
      const ly = px * Math.sin(t) + py * Math.cos(t);
      if (Math.abs(lx) > w / 2 || Math.abs(ly) > h / 2) continue;
      const id = this.engine.object_id(d[i]);
      // Later entries are drawn on top.
      if (id) found = { id, layer: d[i + 1] };
    }
    if (found?.layer === 0 && this.engine.is_over_text(page, x, y)) return undefined;
    return found?.id;
  }

  // --- formatting and undo ----------------------------------------------------------------

  /** Formatting at the caret or across the selection. */
  formatState(): FormatState {
    const e = this.engine;
    const [bold, italic, size] = e.char_format(this.anchor, this.focus);
    const font = e.char_font(this.anchor, this.focus);
    const [align, lineSpacing, spaceBefore, spaceAfter] = e.paragraph_format(this.focus);
    return {
      bold: bold === 1,
      italic: italic === 1,
      size: size > 0 ? size : null,
      font: font || null,
      align: ALIGNMENTS[align] ?? "left",
      lineSpacing: Math.round(lineSpacing * 100) / 100,
      spaceBefore,
      spaceAfter,
      canUndo: e.can_undo(),
      canRedo: e.can_redo(),
    };
  }

  /** Toggles bold on the selection; at a caret, for the next text typed there. */
  toggleBold(): void {
    this.format((a, f) => this.engine.set_bold(a, f, !this.formatState().bold));
  }

  toggleItalic(): void {
    this.format((a, f) => this.engine.set_italic(a, f, !this.formatState().italic));
  }

  /** Font size in points; `null` for the default. */
  setFontSize(size: number | null): void {
    this.format((a, f) => this.engine.set_font_size(a, f, size ?? 0));
  }

  /** Font family; `null` for the default. */
  setFont(family: string | null): void {
    this.format((a, f) => this.engine.set_font(a, f, family ?? ""));
  }

  setAlign(align: Alignment): void {
    this.engine.set_align(this.anchor, this.focus, ALIGNMENTS.indexOf(align));
    this.edited();
  }

  /** Multiple of single spacing: 1, 1.15, 1.5, 2 … */
  setLineSpacing(spacing: number): void {
    this.engine.set_line_spacing(this.anchor, this.focus, spacing);
    this.edited();
  }

  /** Space before and after the selected paragraphs, in points. */
  setParagraphSpacing(before: number, after: number): void {
    this.engine.set_space_before(this.anchor, this.focus, before);
    this.engine.set_space_after(this.anchor, this.focus, after);
    this.edited();
  }

  /** Undoes this user's last change; other people's edits are never undone. */
  undo(): void {
    this.history(this.engine.undo(this.anchor, this.focus));
  }

  redo(): void {
    this.history(this.engine.redo(this.anchor, this.focus));
  }

  /**
   * A character format command. With a selection it's an edit; at a caret it only changes
   * what typing will produce, so just the toolbar state needs refreshing.
   */
  private format(apply: (anchor: number, focus: number) => void): void {
    apply(this.anchor, this.focus);
    if (this.anchor !== this.focus) this.edited();
    else this.drawSelection();
  }

  private history(sel: Uint32Array | undefined): void {
    if (!sel) return;
    this.anchor = sel[0];
    this.focus = sel[1];
    this.upstream = false;
    this.goalX = null;
    this.edited();
  }

  /** Lays out, redraws pages and the caret. Call after changing the document from outside. */
  relayout(): void {
    const t0 = performance.now();
    const pages = this.engine.layout();
    const layoutMs = performance.now() - t0;
    const render = this.renderer.render(pages);
    const len = this.engine.len();
    this.anchor = Math.min(this.anchor, len);
    this.focus = Math.min(this.focus, len);
    this.drawSelection();
    this.onUpdate?.({ layoutMs, render });
  }

  /** Relayout after a local edit, and tell edit listeners. */
  private edited(): void {
    this.relayout();
    for (const l of this.editListeners) l();
  }

  // --- pointer ---------------------------------------------------------------------------

  private onPointerDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    const handle = (e.target as HTMLElement).closest<HTMLElement>(".opendox-handle");
    const pt = this.pagePoint(e.clientX, e.clientY);
    if (pt && (handle || (e.target as HTMLElement).closest(".opendox-image-frame"))) {
      this.startGesture(e, handle, pt);
      return;
    }
    const over = pt ? this.imageAt(pt.page, pt.x, pt.y) : undefined;
    if (over) {
      this.selectImage(over);
      this.focusEditor();
      this.startGesture(e, null, pt!);
      return;
    }
    if (this.image) this.selectImage(null);
    const hit = this.hitAt(e.clientX, e.clientY);
    if (!hit) return;
    // Keep focus in the textarea and stop the browser starting its own text selection.
    e.preventDefault();
    this.focusEditor();
    this.setCaret(hit[0], hit[1] === 1, e.shiftKey);
    this.goalX = null;
    this.dragging = true;
    this.container.setPointerCapture(e.pointerId);
  }

  private onPointerMove(e: PointerEvent): void {
    if (this.gesture) {
      this.moveGesture(e);
      return;
    }
    if (!this.dragging) return;
    const hit = this.hitAt(e.clientX, e.clientY);
    if (hit) this.setCaret(hit[0], hit[1] === 1, true);
  }

  private onPointerUp(e: PointerEvent): void {
    this.dragging = false;
    const g = this.gesture;
    if (!g) return;
    this.gesture = null;
    if (!g.moved) {
      this.drawSelection();
      return;
    }
    const p = g.preview;
    const props = this.imageProps(g.id);
    if (!props) return;
    const positioned = props.wrap !== "inline" && !props.inTable;
    if (g.kind === "move") {
      if (positioned) {
        const [fw, fh] = footprint(p.w, p.h, p.rotation);
        this.engine.place_image(g.id, p.page, p.cx - fw / 2, p.cy - fh / 2);
      } else {
        const pt = this.pagePoint(e.clientX, e.clientY);
        if (pt) this.engine.place_image(g.id, pt.page, pt.x, pt.y);
      }
    } else {
      // The bounding box's top-left moved by this much; positioned images follow it.
      const [fw, fh] = footprint(p.w, p.h, p.rotation);
      const dx = p.cx - fw / 2 - g.start.x;
      const dy = p.cy - fh / 2 - g.start.y;
      this.engine.set_image_geometry(
        g.id,
        p.w,
        p.h,
        p.rotation,
        props.x + (positioned || props.wrap === "break" ? dx : 0),
        props.y + (positioned ? dy : 0),
      );
    }
    const at = this.engine.image_position(g.id);
    this.edited();
    const after = this.engine.image_position(g.id) ?? at;
    if (after !== undefined) this.collapseTo(after + 1, false);
    this.drawSelection();
  }

  /** Page point (points) under a client point, on the nearest page. */
  private pagePoint(clientX: number, clientY: number): { page: number; x: number; y: number } | undefined {
    const pages = this.renderer.pages;
    let best = -1;
    let bestDist = Infinity;
    pages.forEach((el, i) => {
      const r = el.getBoundingClientRect();
      const dist = clientY < r.top ? r.top - clientY : clientY > r.bottom ? clientY - r.bottom : 0;
      if (dist < bestDist) {
        bestDist = dist;
        best = i;
      }
    });
    if (best < 0) return undefined;
    const r = pages[best].getBoundingClientRect();
    const k = this.renderer.cssPerPt;
    return { page: best, x: (clientX - r.left) / k, y: (clientY - r.top) / k };
  }

  private startGesture(e: PointerEvent, handle: HTMLElement | null, pt: { page: number; x: number; y: number }): void {
    if (!this.image) return;
    const start = this.placed(this.image);
    if (!start) return;
    e.preventDefault();
    e.stopPropagation();
    this.focusEditor();
    let kind: "move" | Handle = "move";
    if (handle?.dataset.handle === "rotate") kind = "rotate";
    else if (handle) {
      const [dx, dy] = handle.dataset.handle!.split(",").map(Number);
      kind = { dx, dy };
    }
    this.gesture = {
      kind,
      id: this.image,
      start,
      pointerX: pt.x,
      pointerY: pt.y,
      moved: false,
      shift: e.shiftKey,
      preview: {
        cx: start.x + start.bw / 2,
        cy: start.y + start.bh / 2,
        w: start.w,
        h: start.h,
        rotation: start.rotation,
        page: start.page,
      },
    };
    this.container.setPointerCapture(e.pointerId);
  }

  private moveGesture(e: PointerEvent): void {
    const g = this.gesture!;
    const pt = this.pagePoint(e.clientX, e.clientY);
    if (!pt) return;
    const s = g.start;
    // Points on the start page, even when the pointer is over another page.
    const startEl = this.renderer.pages[s.page].getBoundingClientRect();
    const k = this.renderer.cssPerPt;
    const px = (e.clientX - startEl.left) / k;
    const py = (e.clientY - startEl.top) / k;
    if (!g.moved && Math.hypot(px - g.pointerX, py - g.pointerY) < 3) return;
    g.moved = true;
    const cx = s.x + s.bw / 2;
    const cy = s.y + s.bh / 2;
    const p = g.preview;
    if (g.kind === "move") {
      p.page = pt.page;
      p.cx = pt.page === s.page ? cx + px - g.pointerX : pt.x - (g.pointerX - cx);
      p.cy = pt.page === s.page ? cy + py - g.pointerY : pt.y - (g.pointerY - cy);
    } else if (g.kind === "rotate") {
      let deg = (Math.atan2(py - cy, px - cx) * 180) / Math.PI + 90;
      deg = ((deg % 360) + 360) % 360;
      // Shift snaps to 15°; otherwise right angles are sticky.
      const step = e.shiftKey ? 15 : 90;
      const snapped = Math.round(deg / step) * step;
      if (e.shiftKey || Math.abs(snapped - deg) < 4) deg = snapped % 360;
      p.rotation = Math.round(deg * 10) / 10;
    } else {
      const { dx, dy } = g.kind;
      const t = (s.rotation * Math.PI) / 180;
      const [cos, sin] = [Math.cos(t), Math.sin(t)];
      // Pointer in the image's own frame, measured from its centre.
      const [vx, vy] = [px - cx, py - cy];
      const lx = vx * cos + vy * sin;
      const ly = -vx * sin + vy * cos;
      // The opposite edge or corner stays put.
      const ax = (-dx * s.w) / 2;
      const ay = (-dy * s.h) / 2;
      let w = dx ? Math.max(8, dx * (lx - ax)) : s.w;
      let h = dy ? Math.max(8, dy * (ly - ay)) : s.h;
      // Corners keep the proportions unless Shift is held; sides stretch one way.
      if (dx && dy && !e.shiftKey) {
        const f = Math.max(w / s.w, h / s.h);
        [w, h] = [s.w * f, s.h * f];
      }
      const ncx = dx ? ax + (dx * w) / 2 : 0;
      const ncy = dy ? ay + (dy * h) / 2 : 0;
      p.w = w;
      p.h = h;
      p.cx = cx + ncx * cos - ncy * sin;
      p.cy = cy + ncx * sin + ncy * cos;
    }
    this.drawSelection();
  }

  private async onDrop(e: DragEvent): Promise<void> {
    const files = [...(e.dataTransfer?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (files.length === 0) return;
    e.preventDefault();
    const hit = this.hitAt(e.clientX, e.clientY);
    if (hit) this.collapseTo(hit[0], hit[1] === 1);
    this.focusEditor();
    for (const f of files) await this.insertImageFile(f);
  }

  /** A right-click outside the selection moves the caret there first, like a left-click. */
  private onContextMenu(e: MouseEvent): void {
    const hit = this.hitAt(e.clientX, e.clientY);
    if (!hit) return;
    const [lo, hi] = this.range();
    if (lo === hi || hit[0] < lo || hit[0] > hi) {
      this.setCaret(hit[0], hit[1] === 1, false);
    }
    this.focusEditor();
  }

  /** Hit-tests the page nearest to a client point (pages stack vertically). */
  private hitAt(clientX: number, clientY: number): Uint32Array | undefined {
    const pages = this.renderer.pages;
    let best = -1;
    let bestDist = Infinity;
    pages.forEach((el, i) => {
      const r = el.getBoundingClientRect();
      const dist = clientY < r.top ? r.top - clientY : clientY > r.bottom ? clientY - r.bottom : 0;
      if (dist < bestDist) {
        bestDist = dist;
        best = i;
      }
    });
    if (best < 0) return undefined;
    const r = pages[best].getBoundingClientRect();
    const k = this.renderer.cssPerPt;
    return this.engine.hit_test(best, (clientX - r.left) / k, (clientY - r.top) / k);
  }

  // --- keyboard --------------------------------------------------------------------------

  private onKeyDown(e: KeyboardEvent): void {
    if (this.composing || e.isComposing) return;
    if (this.image && this.onImageKey(e)) {
      e.preventDefault();
      return;
    }
    const mod = e.ctrlKey || e.metaKey;
    const extend = e.shiftKey;
    if (mod && !e.altKey && this.onShortcut(e.key.toLowerCase(), e.shiftKey)) {
      e.preventDefault();
      return;
    }
    switch (e.key) {
      case "ArrowLeft":
        this.moveHorizontal(false, extend);
        break;
      case "ArrowRight":
        this.moveHorizontal(true, extend);
        break;
      case "ArrowUp":
        this.moveVertical(false, extend);
        break;
      case "ArrowDown":
        this.moveVertical(true, extend);
        break;
      case "Home":
        this.moveToLineEdge(false, mod, extend);
        break;
      case "End":
        this.moveToLineEdge(true, mod, extend);
        break;
      case "Backspace":
        this.deleteBackward();
        break;
      case "Delete":
        this.deleteForward();
        break;
      case "Enter":
        this.insertText("\n");
        break;
      case "Tab":
        // Outside tables Tab keeps its default job of moving focus out of the editor.
        if (mod || !this.tableInfo()) return;
        this.tabToCell(!e.shiftKey);
        break;
      case "a":
      case "A":
        if (!mod) return;
        this.anchor = 0;
        this.focus = this.engine.len();
        this.upstream = false;
        this.drawSelection();
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  /** Keys while an image is selected. Returns whether the key was handled here; others go
   * on to the text, at a caret just after the image. */
  private onImageKey(e: KeyboardEvent): boolean {
    const id = this.image!;
    const props = this.imageProps(id);
    if (e.key === "Delete" || e.key === "Backspace") {
      this.deleteImage();
      return true;
    }
    if (e.key === "Escape") {
      this.selectImage(null);
      return true;
    }
    const nudge: Record<string, [number, number]> = {
      ArrowLeft: [-1, 0],
      ArrowRight: [1, 0],
      ArrowUp: [0, -1],
      ArrowDown: [0, 1],
    };
    if (e.key in nudge && props && props.wrap !== "inline" && !props.inTable) {
      const [dx, dy] = nudge[e.key];
      const k = e.shiftKey ? 10 : 1;
      this.setImageOffset(props.x + dx * k, props.wrap === "break" ? props.y : props.y + dy * k);
      return true;
    }
    if (!(e.ctrlKey || e.metaKey) || !["z", "y"].includes(e.key.toLowerCase())) {
      this.image = null;
      this.drawSelection();
    }
    return false;
  }

  /** Ctrl/Cmd shortcuts, as in Google Docs. Returns whether `key` was one. */
  private onShortcut(key: string, shift: boolean): boolean {
    const aligns: Record<string, Alignment> = { l: "left", e: "center", r: "right", j: "justify" };
    if (shift && key in aligns) {
      this.setAlign(aligns[key]);
    } else if (key === "z") {
      if (shift) this.redo();
      else this.undo();
    } else if (key === "y" && !shift) {
      this.redo();
    } else if (key === "b" && !shift) {
      this.toggleBold();
    } else if (key === "i" && !shift) {
      this.toggleItalic();
    } else {
      return false;
    }
    return true;
  }

  private moveHorizontal(forward: boolean, extend: boolean): void {
    this.goalX = null;
    if (!extend && this.anchor !== this.focus) {
      const [lo, hi] = this.range();
      this.collapseTo(forward ? hi : lo, false);
      this.drawSelection();
      return;
    }
    const to = forward ? this.engine.move_right(this.focus) : this.engine.move_left(this.focus);
    this.setCaret(to, false, extend);
  }

  private moveVertical(down: boolean, extend: boolean): void {
    const rect = this.engine.caret_rect(this.focus, this.upstream);
    if (!rect) return;
    const goal = this.goalX ?? rect[1];
    const to = this.engine.move_vertical(this.focus, this.upstream, goal, down);
    if (to) {
      this.setCaret(to[0], to[1] === 1, extend);
    } else {
      this.setCaret(down ? this.engine.len() : 0, false, extend);
    }
    this.goalX = goal;
  }

  private moveToLineEdge(end: boolean, wholeDocument: boolean, extend: boolean): void {
    this.goalX = null;
    if (wholeDocument) {
      this.setCaret(end ? this.engine.len() : 0, false, extend);
      return;
    }
    const to = end
      ? this.engine.line_end(this.focus, this.upstream)
      : this.engine.line_start(this.focus, this.upstream);
    if (to) this.setCaret(to[0], to[1] === 1, extend);
  }

  /** Tab / Shift+Tab between cells; Tab in the last cell adds a row, as in Google Docs. */
  private tabToCell(forward: boolean): void {
    this.goalX = null;
    let to = forward ? this.engine.next_cell(this.focus) : this.engine.prev_cell(this.focus);
    if (to === undefined && forward) {
      this.engine.table_insert_row(this.focus, true);
      this.engine.layout();
      to = this.engine.next_cell(this.focus);
    }
    if (to !== undefined) this.collapseTo(to, false);
    this.edited();
  }

  private deleteBackward(): void {
    const pos =
      this.anchor !== this.focus
        ? this.deleteSelectionRaw()
        : this.engine.delete_backward(this.focus);
    this.collapseTo(pos, false);
    this.edited();
  }

  private deleteForward(): void {
    const pos =
      this.anchor !== this.focus
        ? this.deleteSelectionRaw()
        : this.engine.delete_forward(this.focus);
    this.collapseTo(pos, false);
    this.edited();
  }

  // --- text input and IME ----------------------------------------------------------------

  private onInput(): void {
    if (this.composing) return;
    this.flushInput();
  }

  private onCompositionStart(): void {
    this.composing = true;
    // Typing replaces the selection; remove it now so the preedit sits where text will go.
    if (this.anchor !== this.focus) {
      this.deleteSelectionRaw();
      this.edited();
    }
    this.input.classList.add("opendox-composing");
  }

  private onCompositionEnd(): void {
    this.composing = false;
    this.input.classList.remove("opendox-composing");
    // Read the committed text from the textarea rather than the event: Chrome and Firefox fire
    // `input` and `compositionend` in different orders, and this handles both.
    this.flushInput();
  }

  private flushInput(): void {
    const text = this.input.value;
    this.input.value = "";
    if (text) this.insertText(text);
  }

  private onPaste(e: ClipboardEvent): void {
    e.preventDefault();
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (files.length > 0) {
      void (async () => {
        for (const f of files) await this.insertImageFile(f);
      })();
      return;
    }
    const text = e.clipboardData?.getData("text/plain");
    if (text) this.insertText(text);
  }

  private onCopy(e: ClipboardEvent, cut: boolean): void {
    if (this.anchor === this.focus) return;
    e.preventDefault();
    const [lo, hi] = this.range();
    e.clipboardData?.setData("text/plain", this.engine.slice(lo, hi));
    if (cut) {
      this.deleteSelectionRaw();
      this.edited();
    }
  }

  // --- selection state -------------------------------------------------------------------

  private range(): [number, number] {
    return this.anchor <= this.focus ? [this.anchor, this.focus] : [this.focus, this.anchor];
  }

  /** Deletes the selection without relayout; returns where the caret ends up. */
  private deleteSelectionRaw(): number {
    const [lo, hi] = this.range();
    const at = lo === hi ? lo : this.engine.delete(lo, hi);
    this.collapseTo(at, false);
    return at;
  }

  private collapseTo(pos: number, upstream: boolean): void {
    this.anchor = this.focus = pos;
    this.upstream = upstream;
  }

  private setCaret(pos: number, upstream: boolean, extend: boolean): void {
    this.focus = pos;
    if (!extend) this.anchor = pos;
    this.upstream = upstream;
    this.drawSelection();
  }

  /** Redraws selection boxes and the caret, and moves the textarea to the caret. */
  private drawSelection(): void {
    const k = this.renderer.cssPerPt;
    for (const el of this.container.querySelectorAll(
      ".opendox-caret, .opendox-selection, .opendox-remote-caret, .opendox-remote-selection, .opendox-image-frame",
    )) {
      el.remove();
    }
    if (this.image && !this.engine.image_props(this.image)) this.image = null;
    this.container.classList.toggle("opendox-image-selected", this.image !== null);
    if (this.image) this.drawImageFrame(this.image);
    for (const r of this.remoteSelections?.() ?? []) this.drawRemote(r);
    if (this.anchor !== this.focus) {
      const rects = this.engine.selection_rects(this.anchor, this.focus);
      for (let i = 0; i < rects.length; i += RECT_STRIDE) {
        const box = document.createElement("div");
        box.className = "opendox-selection";
        placeBox(box, rects[i + 1] * k, rects[i + 2] * k, rects[i + 3] * k, rects[i + 4] * k);
        this.renderer.overlay(rects[i])?.append(box);
      }
    }
    this.onSelectionChange?.();
    for (const l of this.selectionListeners) l();

    const caret = this.engine.caret_rect(this.focus, this.upstream);
    if (!caret) return;
    const [page, x, top, bottom] = caret;
    const pageEl = this.renderer.pages[page];
    if (!pageEl) return;
    const height = (bottom - top) * k;

    if (this.anchor === this.focus) {
      const el = document.createElement("div");
      el.className = "opendox-caret";
      placeBox(el, x * k, top * k, 0, height);
      el.style.width = "";
      this.renderer.overlay(page)?.append(el);
    }

    // The textarea follows the caret so IME candidate windows open next to it.
    this.input.style.left = `${pageEl.offsetLeft + x * k}px`;
    this.input.style.top = `${pageEl.offsetTop + top * k}px`;
    this.input.style.height = `${height}px`;
    this.input.style.fontSize = `${height * 0.7}px`;

    const focusEl = this.anchor === this.focus ? this.renderer.overlay(page)?.lastElementChild : null;
    (focusEl as HTMLElement | null)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  /** The selected image's frame: eight size handles and a rotate handle, turned with the
   * image. During a gesture it shows the preview instead. */
  private drawImageFrame(id: string): void {
    const k = this.renderer.cssPerPt;
    const g = this.gesture?.id === id && this.gesture.moved ? this.gesture.preview : undefined;
    const placed = this.placed(id);
    if (!placed) return;
    const f = g ?? {
      cx: placed.x + placed.bw / 2,
      cy: placed.y + placed.bh / 2,
      w: placed.w,
      h: placed.h,
      rotation: placed.rotation,
      page: placed.page,
    };
    const frame = document.createElement("div");
    frame.className = "opendox-image-frame";
    const props = this.imageProps(id);
    if (props && (props.wrap === "inline" || props.inTable)) frame.classList.add("opendox-inline");
    frame.style.left = `${(f.cx - f.w / 2) * k}px`;
    frame.style.top = `${(f.cy - f.h / 2) * k}px`;
    frame.style.width = `${f.w * k}px`;
    frame.style.height = `${f.h * k}px`;
    frame.style.transform = `rotate(${f.rotation}deg)`;
    const cursors = ["nwse", "ns", "nesw", "ew"];
    for (const [dx, dy] of [[-1, -1], [0, -1], [1, -1], [1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0]]) {
      const h = document.createElement("div");
      h.className = "opendox-handle";
      h.dataset.handle = `${dx},${dy}`;
      h.style.left = `${((dx + 1) / 2) * 100}%`;
      h.style.top = `${((dy + 1) / 2) * 100}%`;
      // The cursor follows the handle's direction once the image is turned.
      const angle = (Math.atan2(dy, dx) * 180) / Math.PI + f.rotation;
      const octant = ((Math.round(angle / 45) % 8) + 8) % 8;
      h.style.cursor = `${cursors[[3, 0, 1, 2, 3, 0, 1, 2][octant]]}-resize`;
      frame.append(h);
    }
    const stem = document.createElement("div");
    stem.className = "opendox-stem";
    const rot = document.createElement("div");
    rot.className = "opendox-handle opendox-rotate";
    rot.dataset.handle = "rotate";
    rot.style.left = "50%";
    rot.style.top = "-20px";
    rot.title = "Rotate (Shift: 15° steps)";
    frame.append(stem, rot);
    this.renderer.overlay(f.page)?.append(frame);
  }

  /** Another person's selection wash and caret, with their name above the caret. */
  private drawRemote(r: RemoteSelection): void {
    const k = this.renderer.cssPerPt;
    const len = this.engine.len();
    const [a, f] = [Math.min(r.anchor, len), Math.min(r.focus, len)];
    if (a !== f) {
      const rects = this.engine.selection_rects(a, f);
      for (let i = 0; i < rects.length; i += RECT_STRIDE) {
        const box = document.createElement("div");
        box.className = "opendox-remote-selection";
        box.style.background = `color-mix(in srgb, ${r.color} 22%, transparent)`;
        placeBox(box, rects[i + 1] * k, rects[i + 2] * k, rects[i + 3] * k, rects[i + 4] * k);
        this.renderer.overlay(rects[i])?.append(box);
      }
    }
    const caret = this.engine.caret_rect(f, false);
    if (!caret) return;
    const [page, x, top, bottom] = caret;
    const el = document.createElement("div");
    el.className = "opendox-remote-caret";
    el.style.background = r.color;
    placeBox(el, x * k, top * k, 0, (bottom - top) * k);
    el.style.width = "";
    const name = document.createElement("span");
    name.className = "opendox-remote-name";
    name.style.background = r.color;
    name.textContent = r.name;
    el.append(name);
    this.renderer.overlay(page)?.append(el);
  }
}

/** Width and height of a `w` x `h` box turned by `deg` degrees: its bounding box. */
function footprint(w: number, h: number, deg: number): [number, number] {
  const t = (deg * Math.PI) / 180;
  const [s, c] = [Math.abs(Math.sin(t)), Math.abs(Math.cos(t))];
  return [w * c + h * s, w * s + h * c];
}

function placeBox(el: HTMLElement, left: number, top: number, width: number, height: number) {
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
  el.style.width = `${width}px`;
  el.style.height = `${height}px`;
}
