/**
 * Pasted HTML → opendox's clipboard format (the JSON `Editor.paste` takes).
 *
 * HTML copied from opendox carries the exact content in a `data-opendox-clip` attribute,
 * which is used as is. Anything else (Google Docs, Word, web pages) is read for what the
 * editor can keep: paragraphs, headings, lists, alignment, bold, italic, underline,
 * strikethrough, superscript/subscript, text and highlight colours, sizes and links. Fonts,
 * images and tables are left out (table cells become paragraphs).
 */

interface Run {
  start: number;
  end: number;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  size?: number;
  color?: string;
  highlight?: string;
  script?: "super" | "sub";
  link?: string;
}

interface Paragraph {
  text: string;
  runs: Run[];
  style?: string;
  align?: string;
  list?: { kind: "bullet" | "number"; level: number; id: string };
}

/** What a stretch of text looks like while walking the tree. */
type Look = Omit<Run, "start" | "end">;

const BLOCKS = new Set([
  "P", "DIV", "H1", "H2", "H3", "H4", "H5", "H6", "LI", "BLOCKQUOTE", "PRE", "TR", "TD", "TH",
  "UL", "OL", "TABLE", "SECTION", "ARTICLE", "HEADER", "FOOTER", "DT", "DD", "FIGURE",
]);
const SKIP = new Set(["SCRIPT", "STYLE", "HEAD", "META", "TITLE", "IMG", "SVG", "TEMPLATE", "NOSCRIPT", "OBJECT"]);

/** The JSON to paste for `html`, or `null` if it holds no text worth pasting. */
export function htmlToClip(html: string): string | null {
  const dom = new DOMParser().parseFromString(html, "text/html");
  const own = dom.querySelector("[data-opendox-clip]")?.getAttribute("data-opendox-clip");
  if (own) return own;

  const paragraphs: Paragraph[] = [];
  let current: Paragraph | null = null;
  let chars = 0;
  const lists: { kind: "bullet" | "number"; id: string }[] = [];
  let listCount = 0;

  const open = (block?: Element): Paragraph => {
    const p: Paragraph = { text: "", runs: [] };
    if (block) {
      const tag = block.tagName;
      if (/^H[1-6]$/.test(tag)) p.style = tag.toLowerCase();
      const align = alignOf(block);
      if (align) p.align = align;
      const li = block.closest("li");
      if (li && lists.length > 0) {
        const aria = Number(li.getAttribute("aria-level"));
        const level = aria > 0 ? aria - 1 : lists.length - 1;
        const top = lists[lists.length - 1];
        p.list = { kind: top.kind, level: Math.min(level, 8), id: lists[0].id };
      }
    }
    paragraphs.push(p);
    chars = 0;
    return p;
  };
  /** Ends the paragraph being filled; the next text starts a new one. */
  const close = () => {
    current = null;
  };

  const text = (s: string, look: Look, pre: boolean) => {
    let t = pre ? s : s.replace(/[\t\n\r ]+/g, " ");
    if (!current) {
      t = pre ? t : t.replace(/^ +/, "");
      if (!t) return;
      current = open();
    }
    if (!t) return;
    if (!pre && current.text.endsWith(" ") && t.startsWith(" ")) t = t.slice(1);
    // The model counts Unicode scalar values.
    const n = [...t].length;
    if (n === 0) return;
    const run: Run = { start: chars, end: chars + n, ...look };
    const styled = Object.keys(look).length > 0;
    const last = current.runs[current.runs.length - 1];
    if (styled) {
      if (last && last.end === chars && sameLook(last, look)) last.end += n;
      else current.runs.push(run);
    }
    current.text += t;
    chars += n;
  };

  const walk = (node: Node, look: Look, pre: boolean): void => {
    if (node.nodeType === Node.TEXT_NODE) {
      text(node.textContent ?? "", look, pre);
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const el = node as HTMLElement;
    const tag = el.tagName;
    if (SKIP.has(tag)) return;
    if (tag === "BR") {
      if (!current) current = open();
      close();
      return;
    }
    const isList = tag === "UL" || tag === "OL";
    if (isList) {
      lists.push({ kind: tag === "OL" ? "number" : "bullet", id: lists[0]?.id ?? `paste-${listCount++}` });
    }
    const block = BLOCKS.has(tag) && !isList && tag !== "TABLE" && tag !== "TR";
    if (block) {
      // An outer block with no text of its own (a wrapping <div>) gives way to this one.
      if (current && !current.text && paragraphs[paragraphs.length - 1] === current) paragraphs.pop();
      close();
      current = open(el);
    }
    const inner = { ...look, ...lookOf(el, look) };
    const isPre = pre || tag === "PRE";
    for (const child of el.childNodes) walk(child, inner, isPre);
    if (isList) lists.pop();
    if (block || tag === "TR" || tag === "TABLE") close();
  };

  walk(dom.body, {}, false);
  // Drop empty paragraphs at the ends (HTML's own whitespace), and trailing spaces.
  for (const p of paragraphs) {
    const trimmed = p.text.replace(/ +$/, "");
    const n = [...trimmed].length;
    p.text = trimmed;
    p.runs = p.runs.map((r) => ({ ...r, end: Math.min(r.end, n) })).filter((r) => r.start < r.end);
  }
  while (paragraphs.length && !paragraphs[0].text) paragraphs.shift();
  while (paragraphs.length && !paragraphs[paragraphs.length - 1].text) paragraphs.pop();
  if (paragraphs.length === 0) return null;
  return JSON.stringify({ paragraphs });
}

function sameLook(a: Look, b: Look): boolean {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)].filter((k) => k !== "start" && k !== "end"));
  for (const k of keys) {
    if ((a as Record<string, unknown>)[k] !== (b as Record<string, unknown>)[k]) return false;
  }
  return true;
}

function alignOf(el: Element): string | undefined {
  const a = ((el as HTMLElement).style?.textAlign || el.getAttribute("align") || "").toLowerCase();
  return a === "center" || a === "right" || a === "justify" ? a : undefined;
}

/** What an element changes about the look of the text inside it. */
function lookOf(el: HTMLElement, outer: Look): Look {
  const look: Look = {};
  const tag = el.tagName;
  const st = el.style;
  if (tag === "B" || tag === "STRONG") look.bold = true;
  if (tag === "I" || tag === "EM") look.italic = true;
  if (tag === "U" || tag === "INS") look.underline = true;
  if (tag === "S" || tag === "STRIKE" || tag === "DEL") look.strike = true;
  if (tag === "SUP") look.script = "super";
  if (tag === "SUB") look.script = "sub";
  if (tag === "A") {
    const href = el.getAttribute("href") ?? "";
    if (/^(https?:|mailto:)/i.test(href)) look.link = href;
  }
  // Inline styles win over tags (Google Docs wraps everything in <b style="font-weight:normal">).
  const weight = st.fontWeight;
  if (weight) look.bold = weight === "bold" || weight === "bolder" || Number(weight) >= 600;
  if (st.fontStyle) look.italic = st.fontStyle === "italic" || st.fontStyle === "oblique";
  const deco = `${st.textDecorationLine || st.textDecoration || ""}`;
  if (deco.includes("underline")) look.underline = true;
  if (deco.includes("line-through")) look.strike = true;
  if (st.verticalAlign === "super") look.script = "super";
  if (st.verticalAlign === "sub") look.script = "sub";
  const color = hex(st.color);
  // Plain black is the default ink, not a colour choice; Google Docs writes it on everything.
  if (color && color !== "#000000") look.color = color;
  const bg = hex(st.backgroundColor);
  if (bg && bg !== "#ffffff") look.highlight = bg;
  const size = points(st.fontSize);
  if (size && !(look.script ?? outer.script)) look.size = size;
  // Default false values clutter the runs; drop them unless they undo an outer format.
  for (const k of ["bold", "italic", "underline", "strike"] as const) {
    if (look[k] === false && !outer[k]) delete look[k];
  }
  return look;
}

/** A CSS colour as `#rrggbb`: hex or rgb()/rgba() with full opacity; anything else is dropped. */
function hex(css: string): string | undefined {
  if (!css) return undefined;
  const h = css.trim().toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(h)) return h;
  if (/^#[0-9a-f]{3}$/.test(h)) return `#${[...h.slice(1)].map((c) => c + c).join("")}`;
  const m = h.match(/^rgba?\(\s*(\d+)[ ,]+(\d+)[ ,]+(\d+)(?:[ ,/]+([\d.]+%?))?\s*\)$/);
  if (!m) return undefined;
  if (m[4] !== undefined && parseFloat(m[4]) < (m[4].endsWith("%") ? 100 : 1)) return undefined;
  return `#${[m[1], m[2], m[3]].map((v) => Math.min(255, Number(v)).toString(16).padStart(2, "0")).join("")}`;
}

/** A CSS font size in points (pt as is, px at 0.75pt). */
function points(css: string): number | undefined {
  const m = css.match(/^([\d.]+)(pt|px)$/);
  if (!m) return undefined;
  const v = Number(m[1]) * (m[2] === "px" ? 0.75 : 1);
  return v >= 4 && v <= 400 ? Math.round(v * 2) / 2 : undefined;
}
