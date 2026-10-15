import { PageRenderer, type LayoutSource, type RenderStats } from "./renderer.js";

/**
 * Everything the view needs from the engine. The wasm `Editor` implements it. Positions are
 * document char indices (Unicode scalar values, not UTF-16 units).
 */
export interface EditorEngine extends LayoutSource {
  layout(): number;
  len(): number;
  slice(start: number, end: number): string;
  /** Returns the caret position after the inserted text. */
  insert(pos: number, text: string): number;
  delete(start: number, end: number): void;
  prev_boundary(pos: number): number | undefined;
  next_boundary(pos: number): number | undefined;
  /** `[pos, upstream]` */
  hit_test(page: number, x: number, y: number): Uint32Array | undefined;
  /** `[page, x, top, bottom]` in points */
  caret_rect(pos: number, upstream: boolean): Float32Array | undefined;
  move_vertical(pos: number, upstream: boolean, goalX: number, down: boolean): Uint32Array | undefined;
  line_start(pos: number, upstream: boolean): Uint32Array | undefined;
  line_end(pos: number, upstream: boolean): Uint32Array | undefined;
  /** Groups of `page, x, y, width, height` in points */
  selection_rects(a: number, b: number): Float32Array;
}

export interface EditStats {
  layoutMs: number;
  render: RenderStats;
}

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

  /** Current selection as char indices; `anchor === focus` when it's just a caret. */
  get selection(): { anchor: number; focus: number } {
    return { anchor: this.anchor, focus: this.focus };
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
    this.relayout();
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
    const to = forward
      ? this.engine.next_boundary(this.focus)
      : this.engine.prev_boundary(this.focus);
    this.setCaret(to ?? this.focus, false, extend);
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

  private deleteBackward(): void {
    if (this.anchor === this.focus) {
      const from = this.engine.prev_boundary(this.focus);
      if (from === undefined) return;
      this.anchor = from;
    }
    this.deleteSelectionRaw();
    this.relayout();
  }

  private deleteForward(): void {
    if (this.anchor === this.focus) {
      const to = this.engine.next_boundary(this.focus);
      if (to === undefined) return;
      this.anchor = to;
    }
    this.deleteSelectionRaw();
    this.relayout();
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
      this.relayout();
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
      this.relayout();
    }
  }

  // --- selection state -------------------------------------------------------------------

  private range(): [number, number] {
    return this.anchor <= this.focus ? [this.anchor, this.focus] : [this.focus, this.anchor];
  }

  /** Deletes the selection without relayout; returns where the caret ends up. */
  private deleteSelectionRaw(): number {
    const [lo, hi] = this.range();
    if (lo !== hi) this.engine.delete(lo, hi);
    this.collapseTo(lo, false);
    return lo;
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
    for (const el of this.container.querySelectorAll(".opendox-caret, .opendox-selection")) {
      el.remove();
    }
    if (this.anchor !== this.focus) {
      const rects = this.engine.selection_rects(this.anchor, this.focus);
      for (let i = 0; i < rects.length; i += RECT_STRIDE) {
        const box = document.createElement("div");
        box.className = "opendox-selection";
        placeBox(box, rects[i + 1] * k, rects[i + 2] * k, rects[i + 3] * k, rects[i + 4] * k);
        this.renderer.overlay(rects[i])?.append(box);
      }
    }

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
}

function placeBox(el: HTMLElement, left: number, top: number, width: number, height: number) {
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
  el.style.width = `${width}px`;
  el.style.height = `${height}px`;
}
