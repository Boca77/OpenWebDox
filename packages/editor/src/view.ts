import { htmlToClip } from "./clipboard.js";
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
  /** The range as clipboard HTML (with opendox's exact content in it). */
  copy_html(start: number, end: number): string;
  /** Pastes clipboard JSON at `pos`; returns the caret after it. */
  paste(pos: number, json: string): number;
  /** Comments the range; returns the new comment's id. `created`: ms since the epoch. */
  add_comment(anchor: number, focus: number, author: string, text: string, created: number): string;
  reply_comment(id: string, author: string, text: string, created: number): void;
  resolve_comment(id: string, resolved: boolean): void;
  edit_comment(id: string, text: string): void;
  delete_comment(id: string): void;
  /** JSON array of comments (see `Comment`). */
  comments_json(): string;
  /** The open comment covering `pos`, if any. */
  comment_at(pos: number): string | undefined;
  /** Highlights one comment's text more strongly ("" for none); lay out after. */
  set_active_comment(id: string): void;
  /** Refuses (or allows) local edits; commands then throw. */
  set_read_only(on: boolean): void;
  is_read_only(): boolean;
  /** Largest position in the body. */
  len(): number;
  /** Largest position anywhere (the header and footer follow the body). */
  end(): number;
  slice(start: number, end: number): string;
  /** Returns the caret position after the inserted text. */
  insert(pos: number, text: string): number;
  /** Deletes a range; returns the caret position. */
  delete(start: number, end: number): number;
  delete_backward(pos: number): number;
  delete_forward(pos: number): number;
  move_left(pos: number): number;
  move_right(pos: number): number;
  /** To the next word's start (forward) or the start of the word before. */
  move_word(pos: number, forward: boolean): number;
  /** `[start, end]` of the word or run of spaces at `pos`. */
  word_range(pos: number): Uint32Array;
  /** Ctrl+Backspace / Ctrl+Delete; returns the caret. */
  delete_word(pos: number, forward: boolean): number;
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
  set_underline(anchor: number, focus: number, on: boolean): void;
  set_strikethrough(anchor: number, focus: number, on: boolean): void;
  /** `#rrggbb`; "" = default. */
  set_text_color(anchor: number, focus: number, color: string): void;
  /** `#rrggbb`; "" = none. */
  set_highlight(anchor: number, focus: number, color: string): void;
  /** 0 normal, 1 superscript, 2 subscript. */
  set_script(anchor: number, focus: number, script: number): void;
  /** Links the selection (or the link at a caret, or inserts the URL); returns `[start, end]`. */
  set_link(anchor: number, focus: number, url: string): Uint32Array;
  link_at(pos: number): string | undefined;
  /** `[start, end]` of the link at the caret. */
  link_range(pos: number): Uint32Array | undefined;
  /** `[bold, italic, size, underline, strikethrough, script]`, size 0 = default */
  char_format(anchor: number, focus: number): Float32Array;
  char_font(anchor: number, focus: number): string;
  char_color(anchor: number, focus: number): string;
  char_highlight(anchor: number, focus: number): string;
  /** 0 left, 1 centre, 2 right, 3 justify */
  set_align(anchor: number, focus: number, align: number): void;
  set_line_spacing(anchor: number, focus: number, spacing: number): void;
  set_space_before(anchor: number, focus: number, pt: number): void;
  set_space_after(anchor: number, focus: number, pt: number): void;
  /**
   * `[align, line spacing, space before, space after, indent start, indent end, first line
   * indent, list, list level, checked, style, page break before]`; list 0 none, 1 bullets,
   * 2 numbers, 3 checklist; style as in `set_paragraph_style`.
   */
  paragraph_format(pos: number): Float32Array;
  /** 0 normal, 1 title, 2 subtitle, 3–8 heading 1–6. */
  set_paragraph_style(anchor: number, focus: number, style: number): void;
  /** `[flat start, level]` pairs: level 0 title, 1–6 headings. */
  headings(): Uint32Array;
  set_header(on: boolean): void;
  set_footer(on: boolean): void;
  has_header(): boolean;
  has_footer(): boolean;
  /** 0 page number, 1 page count; returns the caret after it. */
  insert_field(pos: number, kind: number): number;
  /** 0 body, 1 header, 2 footer. */
  region(pos: number): number;
  /** `[first, last]` of `pos`'s region. */
  region_bounds(pos: number): Uint32Array;
  /** Start of the header (1) or footer (2). */
  region_start(region: number): number | undefined;
  /** Splits at `pos` and starts the rest on a new page; returns the caret after it. */
  insert_page_break(pos: number): number;
  set_page_break(pos: number, on: boolean): void;
  /** `[width, height, top, right, bottom, left]` in points. */
  page_setup(): Float32Array;
  set_page_setup(width: number, height: number, top: number, right: number, bottom: number, left: number): void;
  /** 0 none, 1 bullets, 2 numbers, 3 checklist. */
  set_list(anchor: number, focus: number, kind: number): void;
  indent(anchor: number, focus: number, deeper: boolean): void;
  set_checked(pos: number, checked: boolean): void;
  /** Points: from the left margin, from the right margin, first line relative to the rest. */
  set_indents(anchor: number, focus: number, start: number, end: number, first: number): void;
  /** Start of the list item whose marker is at the point, if any. */
  marker_at(page: number, x: number, y: number): number | undefined;
  /** `[start, end]` of the paragraph holding `pos`. */
  paragraph_range(pos: number): Uint32Array;
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

export type Script = "normal" | "super" | "sub";
const SCRIPTS: Script[] = ["normal", "super", "sub"];

export type ParagraphStyleName = "normal" | "title" | "subtitle" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6";
const PARAGRAPH_STYLES: ParagraphStyleName[] = ["normal", "title", "subtitle", "h1", "h2", "h3", "h4", "h5", "h6"];

/** A title or heading, for an outline. */
export interface Heading {
  /** Where the heading starts. */
  pos: number;
  /** 0 title, 1–6 heading level. */
  level: number;
  text: string;
}

/** A reply to a comment. */
export interface CommentReply {
  author: string;
  text: string;
  /** Milliseconds since the Unix epoch. */
  created: number;
}

/** A comment on a range of text. `start`/`end` are positions; equal once its text is deleted. */
export interface Comment {
  id: string;
  start: number;
  end: number;
  author: string;
  text: string;
  created: number;
  resolved: boolean;
  replies: CommentReply[];
}

/** Where the caret is: the body (with its tables), the page header or the page footer. */
export type DocRegion = "body" | "header" | "footer";
const REGIONS: DocRegion[] = ["body", "header", "footer"];

export type ListType = "none" | "bullet" | "number" | "check";
const LISTS: ListType[] = ["none", "bullet", "number", "check"];

/** What typing at the start of a paragraph, then a space, turns it into (as in Google Docs). */
const AUTO_LISTS: Record<string, ListType> = { "*": "bullet", "-": "bullet", "1.": "number", "1)": "number", "[]": "check" };

/** Formatting at the caret or selection, for toolbars. */
export interface FormatState {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strikethrough: boolean;
  script: Script;
  /** `#rrggbb`; `null` = the default colour. */
  color: string | null;
  /** `#rrggbb`; `null` = none. */
  highlight: string | null;
  /** The URL of the link at the caret, if any. */
  link: string | null;
  /** Points; `null` = the document default. */
  size: number | null;
  /** Family name; `null` = the document default. */
  font: string | null;
  align: Alignment;
  lineSpacing: number;
  spaceBefore: number;
  spaceAfter: number;
  /** Points; the paragraph's own indents, not counting its list level. */
  indentStart: number;
  indentEnd: number;
  indentFirst: number;
  list: ListType;
  /** 0 to 8. */
  listLevel: number;
  /** The caret's paragraph style. */
  style: ParagraphStyleName;
  /** The caret's paragraph starts a new page. */
  pageBreakBefore: boolean;
  /** Where the caret is. */
  region: DocRegion;
  hasHeader: boolean;
  hasFooter: boolean;
  checked: boolean;
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

/** Whether a key press would change the document (blocked when read-only): deleting, Enter,
 * Tab, and shortcuts other than select-all and copy. */
function editingKey(e: KeyboardEvent): boolean {
  if (["Backspace", "Delete", "Enter", "Tab"].includes(e.key)) return true;
  const mod = e.ctrlKey || e.metaKey;
  if (mod && !e.altKey && !e.shiftKey && ["a", "c"].includes(e.key.toLowerCase())) return false;
  return mod || (e.altKey && e.shiftKey);
}

/** Whether a key press moves or deletes by words: Ctrl, or Alt on macOS. */
function wordKey(e: KeyboardEvent): boolean {
  return /Mac|iPhone|iPad/.test(navigator.platform) ? e.altKey && !e.metaKey : e.ctrlKey;
}

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
  /** Called for Ctrl+K: the app shows its link editor, then calls {@link setLink}. */
  onLinkRequest?: () => void;
  /** Called for Ctrl+Alt+M: the app shows its new-comment box, then calls {@link addComment}. */
  onCommentRequest?: () => void;
  /** Other people's selections, drawn with the local one. Set by `Collaboration`. */
  remoteSelections?: () => RemoteSelection[];

  private readonly editListeners: (() => void)[] = [];
  private readonly selectionListeners: (() => void)[] = [];
  private readonly remoteListeners: (() => void)[] = [];
  private readonly input: HTMLTextAreaElement;
  private anchor = 0;
  private focus = 0;
  private upstream = false;
  /** Column (points) that Up/Down aim for, kept across consecutive vertical moves. */
  private goalX: number | null = null;
  private composing = false;
  private dragging = false;
  /** The last press, to count double and triple clicks. */
  private lastClick = { x: 0, y: 0, time: 0, count: 0 };
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

  /** Whether local edits are refused (a view-only collaborator, or the app's choice). */
  get readOnly(): boolean {
    return this.engine.is_read_only();
  }

  /**
   * Makes the document view-only (or editable again): typing, deleting, pasting and dropping
   * do nothing; moving, selecting and copying still work. Commands called anyway throw.
   */
  setReadOnly(on: boolean): void {
    this.engine.set_read_only(on);
    this.container.classList.toggle("opendox-readonly", on);
    this.drawSelection();
  }

  /** Current selection; `anchor === focus` when it's just a caret. */
  get selection(): { anchor: number; focus: number } {
    return { anchor: this.anchor, focus: this.focus };
  }

  /**
   * Listens for local edits (`"edit"`), changes made elsewhere and applied with
   * {@link applyRemote} (`"remote"`), or caret/selection moves (`"selection"`).
   */
  on(event: "edit" | "remote" | "selection", listener: () => void): void {
    const lists = { edit: this.editListeners, remote: this.remoteListeners, selection: this.selectionListeners };
    lists[event].push(listener);
  }

  /**
   * Applies a change made elsewhere (a remote edit): moves the selection to where the engine
   * remapped it and redraws. Pass the current selection to only redraw remote carets.
   */
  applyRemote(anchor: number, focus: number, docChanged: boolean): void {
    this.anchor = anchor;
    this.focus = focus;
    if (docChanged) {
      this.relayout();
      for (const l of this.remoteListeners) l();
    } else {
      this.drawSelection();
    }
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
    if (clean === " " && this.autoList()) return;
    const at = this.deleteSelectionRaw();
    const after = this.engine.insert(at, clean);
    this.collapseTo(after, false);
    this.edited();
  }

  /**
   * A space typed after "*", "-", "1." or "[]" at the start of a plain paragraph makes it a
   * list item instead. Returns whether it did.
   */
  private autoList(): boolean {
    if (this.anchor !== this.focus) return false;
    const [start] = this.engine.paragraph_range(this.focus);
    const type = AUTO_LISTS[this.engine.slice(start, this.focus)];
    if (!type || this.formatState().list !== "none" || this.tableInfo()) return false;
    const at = this.engine.delete(start, this.focus);
    this.engine.layout();
    this.engine.set_list(at, at, LISTS.indexOf(type));
    this.collapseTo(at, false);
    this.edited();
    return true;
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
    const [bold, italic, size, underline, strikethrough, script] = e.char_format(this.anchor, this.focus);
    const font = e.char_font(this.anchor, this.focus);
    const color = e.char_color(this.anchor, this.focus);
    const highlight = e.char_highlight(this.anchor, this.focus);
    const [align, lineSpacing, spaceBefore, spaceAfter, indentStart, indentEnd, indentFirst, list, level, checked, style, pageBreak] =
      e.paragraph_format(this.focus);
    return {
      bold: bold === 1,
      italic: italic === 1,
      underline: underline === 1,
      strikethrough: strikethrough === 1,
      script: SCRIPTS[script] ?? "normal",
      color: color || null,
      highlight: highlight || null,
      link: e.link_at(this.focus) ?? null,
      size: size > 0 ? size : null,
      font: font || null,
      align: ALIGNMENTS[align] ?? "left",
      lineSpacing: Math.round(lineSpacing * 100) / 100,
      spaceBefore,
      spaceAfter,
      indentStart,
      indentEnd,
      indentFirst,
      list: LISTS[list] ?? "none",
      listLevel: level,
      checked: checked === 1,
      style: PARAGRAPH_STYLES[style] ?? "normal",
      pageBreakBefore: pageBreak === 1,
      region: REGIONS[e.region(this.focus)] ?? "body",
      hasHeader: e.has_header(),
      hasFooter: e.has_footer(),
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

  toggleUnderline(): void {
    this.format((a, f) => this.engine.set_underline(a, f, !this.formatState().underline));
  }

  toggleStrikethrough(): void {
    this.format((a, f) => this.engine.set_strikethrough(a, f, !this.formatState().strikethrough));
  }

  /** Superscript or subscript on, or back to normal if it already is. */
  toggleScript(script: Exclude<Script, "normal">): void {
    const next = this.formatState().script === script ? "normal" : script;
    this.format((a, f) => this.engine.set_script(a, f, SCRIPTS.indexOf(next)));
  }

  /** Text colour as `#rrggbb`; `null` for the default. */
  setTextColor(color: string | null): void {
    this.format((a, f) => this.engine.set_text_color(a, f, color ?? ""));
  }

  /** Highlight colour as `#rrggbb`; `null` removes it. */
  setHighlight(color: string | null): void {
    this.format((a, f) => this.engine.set_highlight(a, f, color ?? ""));
  }

  /**
   * Links the selection to `url`, or changes the link at the caret; with nothing selected
   * outside a link, inserts the URL as linked text. An empty `url` removes the link.
   */
  setLink(url: string): void {
    const [start, end] = this.engine.set_link(this.anchor, this.focus, url.trim());
    this.anchor = start;
    this.focus = end;
    this.upstream = false;
    this.edited();
  }

  /** Selects the whole link at the caret; returns whether there was one. */
  selectLink(): boolean {
    const r = this.engine.link_range(this.focus);
    if (!r) return false;
    this.anchor = r[0];
    this.focus = r[1];
    this.drawSelection();
    return true;
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

  /**
   * Makes the selected paragraphs list items of `type`, or plain paragraphs if they already
   * are (the toolbar buttons toggle, as in Google Docs).
   */
  toggleList(type: Exclude<ListType, "none">): void {
    this.setList(this.formatState().list === type ? "none" : type);
  }

  setList(type: ListType): void {
    this.engine.set_list(this.anchor, this.focus, LISTS.indexOf(type));
    this.edited();
  }

  /** Increase (`deeper`) or decrease indent: a list level, or 0.5 in for other paragraphs. */
  indent(deeper: boolean): void {
    this.engine.indent(this.anchor, this.focus, deeper);
    this.edited();
  }

  /** Indents of the selected paragraphs, in points. */
  setIndents(start: number, end: number, first: number): void {
    this.engine.set_indents(this.anchor, this.focus, start, end, first);
    this.edited();
  }

  /** Ticks or clears the checklist item at the caret. */
  setChecked(checked: boolean): void {
    this.engine.set_checked(this.focus, checked);
    this.edited();
  }

  /**
   * Adds the header or footer (and puts the caret in it), or removes it with its text. Adding
   * one that exists just moves the caret there.
   */
  setMargin(region: Exclude<DocRegion, "body">, on: boolean): void {
    const e = this.engine;
    const has = region === "header" ? e.has_header() : e.has_footer();
    if (on && !has) {
      if (region === "header") e.set_header(true);
      else e.set_footer(true);
    }
    if (!on && has) {
      if (region === "header") e.set_header(false);
      else e.set_footer(false);
      if (e.region(this.focus) !== 0 || e.region(this.anchor) !== 0) this.collapseTo(0, false);
      this.edited();
      return;
    }
    e.layout();
    const start = e.region_start(REGIONS.indexOf(region));
    if (start !== undefined) {
      const [, end] = e.region_bounds(start);
      this.collapseTo(end, false);
    }
    this.edited();
  }

  /** Inserts a page number or page count at the caret, which must be in the header or footer. */
  insertField(kind: "page" | "pages"): void {
    if (this.engine.region(this.focus) === 0) return;
    const at = this.deleteSelectionRaw();
    this.collapseTo(this.engine.insert_field(at, kind === "pages" ? 1 : 0), false);
    this.edited();
  }

  /** Ctrl+Enter: continues on a new page from the caret (replacing any selection). */
  insertPageBreak(): void {
    const at = this.deleteSelectionRaw();
    this.collapseTo(this.engine.insert_page_break(at), false);
    this.edited();
  }

  /** Page size and margins in points, for everyone editing the document. */
  pageSetup(): { width: number; height: number; top: number; right: number; bottom: number; left: number } {
    const [width, height, top, right, bottom, left] = this.engine.page_setup();
    return { width, height, top, right, bottom, left };
  }

  /** Throws if the margins leave no room for text. */
  setPageSetup(p: { width: number; height: number; top: number; right: number; bottom: number; left: number }): void {
    this.engine.set_page_setup(p.width, p.height, p.top, p.right, p.bottom, p.left);
    this.edited();
  }

  /** Comments the selection (which must not be empty); returns the comment's id. */
  addComment(author: string, text: string): string {
    const id = this.engine.add_comment(this.anchor, this.focus, author, text, Date.now());
    this.engine.set_active_comment(id);
    this.edited();
    return id;
  }

  /** Every comment, open and resolved, in reading order. */
  comments(): Comment[] {
    return JSON.parse(this.engine.comments_json()) as Comment[];
  }

  replyToComment(id: string, author: string, text: string): void {
    this.engine.reply_comment(id, author, text, Date.now());
    this.edited();
  }

  resolveComment(id: string, resolved = true): void {
    this.engine.resolve_comment(id, resolved);
    this.edited();
  }

  editComment(id: string, text: string): void {
    this.engine.edit_comment(id, text);
    this.edited();
  }

  deleteComment(id: string): void {
    this.engine.delete_comment(id);
    this.edited();
  }

  /**
   * The open comment at the caret or the start of the selection (the most recent if several),
   * if any. A caret just after commented text counts, as after typing a comment's last word.
   */
  commentAtCaret(): string | null {
    const e = this.engine;
    const pos = Math.min(this.anchor, this.focus);
    const after = this.anchor === this.focus && pos > 0 ? e.comment_at(pos - 1) : undefined;
    return e.comment_at(pos) ?? after ?? null;
  }

  /** Shows one comment's text highlighted more strongly (or none), as when its card is open. */
  setActiveComment(id: string | null): void {
    this.engine.set_active_comment(id ?? "");
    this.relayout();
  }

  /** Gives the selected paragraphs a style (heading, title…). */
  setParagraphStyle(style: ParagraphStyleName): void {
    this.engine.set_paragraph_style(this.anchor, this.focus, PARAGRAPH_STYLES.indexOf(style));
    this.edited();
  }

  /** Titles and headings in reading order, for an outline. */
  headings(): Heading[] {
    const h = this.engine.headings();
    const out: Heading[] = [];
    for (let i = 0; i < h.length; i += 2) {
      const [start, end] = this.engine.paragraph_range(h[i]);
      out.push({ pos: h[i], level: h[i + 1], text: this.engine.slice(start, end) });
    }
    return out;
  }

  /** Selects from `anchor` to `focus` (positions; equal for a caret). */
  setSelection(anchor: number, focus: number): void {
    this.anchor = anchor;
    this.focus = focus;
    this.upstream = false;
    this.goalX = null;
    this.drawSelection();
  }

  /** Puts the caret at `pos` and scrolls it into view (e.g. from an outline). */
  goTo(pos: number): void {
    this.collapseTo(pos, false);
    this.goalX = null;
    this.focusEditor();
    this.drawSelection();
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
    // Positions run on past the body into the header and footer.
    const end = this.engine.end();
    this.anchor = Math.min(this.anchor, end);
    this.focus = Math.min(this.focus, end);
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
    // Read-only: no moving or resizing images, no ticking checkboxes; selecting still works.
    const locked = this.readOnly;
    const handle = (e.target as HTMLElement).closest<HTMLElement>(".opendox-handle");
    const pt = this.pagePoint(e.clientX, e.clientY);
    if (!locked && pt && (handle || (e.target as HTMLElement).closest(".opendox-image-frame"))) {
      this.startGesture(e, handle, pt);
      return;
    }
    const over = pt && !locked ? this.imageAt(pt.page, pt.x, pt.y) : undefined;
    if (over) {
      this.selectImage(over);
      this.focusEditor();
      this.startGesture(e, null, pt!);
      return;
    }
    if (this.image) this.selectImage(null);
    if (pt && !locked && this.toggleCheckbox(pt.page, pt.x, pt.y)) {
      e.preventDefault();
      this.focusEditor();
      return;
    }
    const hit = this.hitAt(e.clientX, e.clientY);
    if (!hit) return;
    // Keep focus in the textarea and stop the browser starting its own text selection.
    e.preventDefault();
    const url = (e.ctrlKey || e.metaKey) && this.engine.link_at(hit[0]);
    if (url) {
      this.onOpenLink(url);
      return;
    }
    this.focusEditor();
    this.setCaret(hit[0], hit[1] === 1, e.shiftKey);
    this.goalX = null;
    // Double-click selects a word, triple-click the paragraph, as in Google Docs. Counted
    // here: pointer events' `detail` is always 0 in Chrome.
    const now = performance.now();
    const near = Math.abs(e.clientX - this.lastClick.x) < 5 && Math.abs(e.clientY - this.lastClick.y) < 5;
    // A fourth click starts over as a single click.
    const clicks = near && now - this.lastClick.time < 500 ? (this.lastClick.count % 3) + 1 : 1;
    this.lastClick = { x: e.clientX, y: e.clientY, time: now, count: clicks };
    if (clicks >= 2 && !e.shiftKey) {
      const [a, b] = clicks === 2 ? this.engine.word_range(hit[0]) : this.engine.paragraph_range(hit[0]);
      this.anchor = a;
      this.focus = b;
      this.upstream = false;
      this.drawSelection();
      return;
    }
    this.dragging = true;
    this.container.setPointerCapture(e.pointerId);
  }

  /** Opens a link in a new tab; only web and mail links, never `javascript:` and the like. */
  private onOpenLink(url: string): void {
    const safe = /^(https?:|mailto:)/i.test(url) ? url : /^[\w.-]+\.[a-z]{2,}/i.test(url) ? `https://${url}` : null;
    if (safe) window.open(safe, "_blank", "noopener,noreferrer");
  }

  /** A click on a checklist item's box ticks or clears it. Returns whether it was one. */
  private toggleCheckbox(page: number, x: number, y: number): boolean {
    const item = this.engine.marker_at(page, x, y);
    if (item === undefined) return false;
    const f = this.engine.paragraph_format(item);
    if (LISTS[f[7]] !== "check") return false;
    this.engine.set_checked(item, f[9] !== 1);
    // Into the item, so redrawing the caret doesn't scroll the view back to where it was.
    this.collapseTo(item, false);
    this.edited();
    return true;
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
    if (this.readOnly) {
      e.preventDefault();
      return;
    }
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
    // Typing ends a multi-click: the next click is a single click.
    this.lastClick.count = 0;
    if (this.composing || e.isComposing) return;
    if (this.readOnly && editingKey(e)) {
      e.preventDefault();
      return;
    }
    if (this.image && this.onImageKey(e)) {
      e.preventDefault();
      return;
    }
    const mod = e.ctrlKey || e.metaKey;
    const extend = e.shiftKey;
    if (mod && !e.altKey && this.onShortcut(e.key.toLowerCase(), e.shiftKey, e.code)) {
      e.preventDefault();
      return;
    }
    // Ctrl+Alt+M: comment, as in Google Docs.
    if (mod && e.altKey && !e.shiftKey && e.code === "KeyM" && this.onCommentRequest) {
      this.onCommentRequest();
      e.preventDefault();
      return;
    }
    // Ctrl+Alt+0–6: normal text, heading 1–6, as in Google Docs.
    if (mod && e.altKey && !e.shiftKey && /^Digit[0-6]$/.test(e.code)) {
      const n = Number(e.code.slice(5));
      this.setParagraphStyle(n === 0 ? "normal" : PARAGRAPH_STYLES[2 + n]);
      e.preventDefault();
      return;
    }
    // Alt+Shift+5: strikethrough, as in Google Docs (by physical key: Shift changes the digit).
    if (e.altKey && e.shiftKey && !mod && e.code === "Digit5") {
      this.toggleStrikethrough();
      e.preventDefault();
      return;
    }
    switch (e.key) {
      case "ArrowLeft":
        this.moveHorizontal(false, extend, wordKey(e));
        break;
      case "ArrowRight":
        this.moveHorizontal(true, extend, wordKey(e));
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
        if (wordKey(e) && this.anchor === this.focus) this.deleteWord(false);
        else if (!this.removePageBreak() && !this.leaveList(false)) this.deleteBackward();
        break;
      case "Delete":
        if (wordKey(e) && this.anchor === this.focus) this.deleteWord(true);
        else this.deleteForward();
        break;
      case "Enter":
        if (mod) {
          this.insertPageBreak();
          break;
        }
        if (!e.shiftKey && this.leaveList(true)) break;
        this.insertText("\n");
        break;
      case "Tab":
        if (mod) return;
        if (this.tableInfo()) {
          this.tabToCell(!e.shiftKey);
        } else if (this.formatState().list !== "none") {
          this.indent(!e.shiftKey);
        } else {
          // Elsewhere Tab keeps its default job of moving focus out of the editor.
          return;
        }
        break;
      case "a":
      case "A": {
        if (!mod) return;
        // Everything in the caret's region: the body, or all of the header or footer.
        const [first, last] = this.engine.region_bounds(this.focus);
        this.anchor = first;
        this.focus = last;
        this.upstream = false;
        this.drawSelection();
        break;
      }
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
  private onShortcut(key: string, shift: boolean, code: string): boolean {
    const aligns: Record<string, Alignment> = { l: "left", e: "center", r: "right", j: "justify" };
    // By physical key: with Shift, the digit keys type other characters on most layouts.
    const lists: Record<string, Exclude<ListType, "none">> = { Digit7: "number", Digit8: "bullet", Digit9: "check" };
    if (shift && key in aligns) {
      this.setAlign(aligns[key]);
    } else if (shift && code in lists) {
      this.toggleList(lists[code]);
    } else if (!shift && (key === "]" || key === "[")) {
      this.indent(key === "]");
    } else if (key === "z") {
      if (shift) this.redo();
      else this.undo();
    } else if (key === "y" && !shift) {
      this.redo();
    } else if (key === "b" && !shift) {
      this.toggleBold();
    } else if (key === "i" && !shift) {
      this.toggleItalic();
    } else if (key === "u" && !shift) {
      this.toggleUnderline();
    } else if (code === "Period" && !shift) {
      this.toggleScript("super");
    } else if (code === "Comma" && !shift) {
      this.toggleScript("sub");
    } else if (key === "k" && !shift && this.onLinkRequest) {
      this.onLinkRequest();
    } else {
      return false;
    }
    return true;
  }

  /** Left/Right by a character, or by a word with Ctrl (Alt on macOS). */
  private moveHorizontal(forward: boolean, extend: boolean, word = false): void {
    this.goalX = null;
    if (!extend && !word && this.anchor !== this.focus) {
      const [lo, hi] = this.range();
      this.collapseTo(forward ? hi : lo, false);
      this.drawSelection();
      return;
    }
    const e = this.engine;
    const to = word
      ? e.move_word(this.focus, forward)
      : forward
        ? e.move_right(this.focus)
        : e.move_left(this.focus);
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
      const [first, last] = this.engine.region_bounds(this.focus);
      this.setCaret(down ? last : first, false, extend);
    }
    this.goalX = goal;
  }

  private moveToLineEdge(end: boolean, wholeDocument: boolean, extend: boolean): void {
    this.goalX = null;
    if (wholeDocument) {
      const [first, last] = this.engine.region_bounds(this.focus);
      this.setCaret(end ? last : first, false, extend);
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

  /** Backspace at the start of a paragraph that starts a new page removes the break first,
   * rather than joining it to the paragraph before. Returns whether it did. */
  private removePageBreak(): boolean {
    if (this.anchor !== this.focus) return false;
    const [start] = this.engine.paragraph_range(this.focus);
    if (this.focus !== start || !this.formatState().pageBreakBefore) return false;
    this.engine.set_page_break(this.focus, false);
    this.edited();
    return true;
  }

  /**
   * Enter on an empty list item, or Backspace at the start of one, takes it out of the list
   * (a nested item moves up a level on Enter), as in Google Docs. Returns whether it did.
   */
  private leaveList(enter: boolean): boolean {
    if (this.anchor !== this.focus) return false;
    const f = this.formatState();
    if (f.list === "none") return false;
    const [start, end] = this.engine.paragraph_range(this.focus);
    if (this.focus !== start || (enter && end !== start)) return false;
    if (enter && f.listLevel > 0) this.engine.indent(this.focus, this.focus, false);
    else this.engine.set_list(this.focus, this.focus, 0);
    this.edited();
    return true;
  }

  private deleteWord(forward: boolean): void {
    this.collapseTo(this.engine.delete_word(this.focus, forward), false);
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
    this.lastClick.count = 0;
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
    if (text && !this.readOnly) this.insertText(text);
  }

  private onPaste(e: ClipboardEvent): void {
    e.preventDefault();
    if (this.readOnly) return;
    const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
    if (files.length > 0) {
      void (async () => {
        for (const f of files) await this.insertImageFile(f);
      })();
      return;
    }
    // Formatted content first: opendox's own (exact) or other apps' HTML, as one undo step.
    const html = e.clipboardData?.getData("text/html");
    const clip = html ? htmlToClip(html) : null;
    if (clip) {
      const at = this.deleteSelectionRaw();
      this.collapseTo(this.engine.paste(at, clip), false);
      this.edited();
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
    e.clipboardData?.setData("text/html", this.engine.copy_html(lo, hi));
    if (cut && !this.readOnly) {
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
    if (extend) {
      // A selection stays in the region it started in.
      const [first, last] = this.engine.region_bounds(this.anchor);
      pos = Math.min(Math.max(pos, first), last);
    }
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
    const end = this.engine.end();
    const [a, f] = [Math.min(r.anchor, end), Math.min(r.focus, end)];
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
