import { PageRenderer, type LayoutSource, type RenderStats } from "./renderer.js";

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
    container.addEventListener("pointerup", () => (this.dragging = false));
    container.addEventListener("pointercancel", () => (this.dragging = false));
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
    if (!this.dragging) return;
    const hit = this.hitAt(e.clientX, e.clientY);
    if (hit) this.setCaret(hit[0], hit[1] === 1, true);
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
    const mod = e.ctrlKey || e.metaKey;
    const extend = e.shiftKey;
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
      ".opendox-caret, .opendox-selection, .opendox-remote-caret, .opendox-remote-selection",
    )) {
      el.remove();
    }
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

function placeBox(el: HTMLElement, left: number, top: number, width: number, height: number) {
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
  el.style.width = `${width}px`;
  el.style.height = `${height}px`;
}
