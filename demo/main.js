import init, { Editor } from "../crates/opendox-wasm/pkg/opendox_wasm.js";
import { Collaboration, EditorView, LocalCopy } from "../packages/editor/dist/index.js";
import { setUpImages } from "./images.js";

// Collaboration: open /demo/?doc=<id> in several tabs (needs `npm run sync-server`).
// Optional: &server=ws://host:port, &name=<display name>, &token=<access token> for a server
// started with --secret (opendox-server --secret S --issue <doc> view|edit <seconds>).
const params = new URLSearchParams(location.search);
const DOC_ID = params.get("doc");
const SERVER = params.get("server") ?? "ws://127.0.0.1:5175";
// Collaborator colours carry identity; each keeps white name text above 4.5:1.
const PEOPLE = [
  ["Ada", "#0f766e"],
  ["Grace", "#be185d"],
  ["Alan", "#b45309"],
  ["Edsger", "#15803d"],
  ["Barbara", "#7e22ce"],
  ["Ken", "#9f1239"],
];

// The default family, loaded before first paint. Everything else streams in afterwards; until
// a face arrives its text falls back to these, then reflows (like a web-font swap).
const STARTUP_FONTS = [
  "../fonts/noto/NotoSans-Regular.ttf",
  "../fonts/noto/NotoSansArabic-Regular.ttf",
];
const LATER_FONTS = [
  "../fonts/noto/NotoSans-Bold.ttf",
  "../fonts/noto/NotoSans-Italic.ttf",
  "../fonts/noto/NotoSans-BoldItalic.ttf",
  "../fonts/noto/NotoSerif-Regular.ttf",
  "../fonts/noto/NotoSerif-Bold.ttf",
  "../fonts/noto/NotoSerif-Italic.ttf",
  "../fonts/noto/NotoSerif-BoldItalic.ttf",
  // List bullets and checkboxes (● ○ ■ ☐ ☑), which the text faces lack.
  "../fonts/noto/NotoSansSymbols2-Regular.ttf",
  // Emoji, as outlines (colour emoji formats can't be drawn as paths yet).
  "../fonts/noto/NotoEmoji-Regular.ttf",
];

// Fonts for scripts the bundled ones don't cover, downloaded by `npm run fetch-fonts` (too big
// for the repo) and loaded only when a document has characters nothing else can draw.
// Japanese kana selects JP so kanji take Japanese forms; Han alone, SC; Hangul, KR.
const ON_DEMAND = [
  { family: "Noto Sans JP", url: "../fonts/noto/NotoSansJP-Regular.otf", needs: /[\u3040-\u30ff]/ },
  { family: "Noto Sans SC", url: "../fonts/noto/NotoSansSC-Regular.otf", needs: /[\u3400-\u9fff\uf900-\ufaff\u3000-\u303f\uff00-\uffef]/ },
  { family: "Noto Sans KR", url: "../fonts/noto/NotoSansKR-Regular.otf", needs: /[\u1100-\u11ff\u3130-\u318f\uac00-\ud7af]/ },
];
const BASE_FAMILIES = "Noto Sans, Noto Sans Arabic, Noto Emoji";
const fetchFont = async (url) => new Uint8Array(await (await fetch(url)).arrayBuffer());
const DEFAULT_SIZE = 11;

// Opening of "Alice's Adventures in Wonderland" (Lewis Carroll, 1865; public domain).
const ALICE = [
  "Alice was beginning to get very tired of sitting by her sister on the bank, and of having nothing to do: once or twice she had peeped into the book her sister was reading, but it had no pictures or conversations in it, “and what is the use of a book,” thought Alice “without pictures or conversations?”",
  "So she was considering in her own mind (as well as she could, for the hot day made her feel very sleepy and stupid), whether the pleasure of making a daisy-chain would be worth the trouble of getting up and picking the daisies, when suddenly a White Rabbit with pink eyes ran close by her.",
  "There was nothing so very remarkable in that; nor did Alice think it so very much out of the way to hear the Rabbit say to itself, “Oh dear! Oh dear! I shall be late!” (when she thought it over afterwards, it occurred to her that she ought to have wondered at this, but at the time it all seemed quite natural); but when the Rabbit actually took a watch out of its waistcoat-pocket, and looked at it, and then hurried on, Alice started to her feet, for it flashed across her mind that she had never before seen a rabbit with either a waistcoat-pocket, or a watch to take out of it, and burning with curiosity, she ran across the field after it, and fortunately was just in time to see it pop down a large rabbit-hole under the hedge.",
  "In another moment down went Alice after it, never once considering how in the world she was to get out again.",
  "The rabbit-hole went straight on like a tunnel for some way, and then dipped suddenly down, so suddenly that Alice had not a moment to think about stopping herself before she found herself falling down a very deep well.",
  "Mixed scripts on one line: the word for hello is مرحبا, a family emoji 👨‍👩‍👧 stays whole, and café with a combining accent never splits.",
];

const CAST = [
  ["Character", "First seen", "Note"],
  ["Alice", "Chapter 1", "Follows the White Rabbit down the hole"],
  ["White Rabbit", "Chapter 1", "Carries a watch in its waistcoat-pocket"],
];

// A sample list at the end: numbered with a nested level, then a checklist.
const LIST = [
  ["Things Alice found on the way down", 0],
  ["Cupboards and book-shelves", 2, 0],
  ["Maps and pictures hung upon pegs", 2, 0],
  ["A jar labelled “ORANGE MARMALADE”", 2, 1],
  ["It was empty", 2, 2],
  ["A little golden key", 2, 0],
  ["Drink the bottle marked “DRINK ME”", 3, 0, true],
  ["Find the way into the loveliest garden", 3, 0],
];

const $ = (id) => document.getElementById(id);
/** Who comments: the collaboration name when sharing, else "You". */
let me = "You";
/** Length in Unicode code points, the unit the engine counts positions in. */
const cpLen = (s) => [...s].length;

async function main() {
  await init();
  const editor = new Editor();
  const later = LATER_FONTS.map(fetchFont);
  for (const bytes of await Promise.all(STARTUP_FONTS.map(fetchFont))) {
    editor.register_font(bytes);
  }
  editor.set_default_font(BASE_FAMILIES, DEFAULT_SIZE);
  editor.set_symbol_font("Noto Sans Symbols 2");

  // A shared document starts from whatever the server has, so no sample text is seeded
  // (two tabs seeding at once would each add a copy). Edits made offline last time are
  // loaded from this device first; connecting sends the server what it missed.
  const local = DOC_ID ? await LocalCopy.open(editor, DOC_ID) : null;
  if (!DOC_ID) seedSample(editor);
  // Building the sample isn't something to undo.
  editor.clear_history();

  const view = new EditorView(editor, $("pages"));
  view.onUpdate = ({ layoutMs, render }) => {
    $("s-pages").textContent = String(render.pages);
    $("s-layout").textContent = `${layoutMs.toFixed(1)} ms`;
    $("s-render").textContent = `${render.ms.toFixed(1)} ms`;
    refreshOutline();
    refreshComments();
  };
  const refreshToolbar = setUpFormatBar(view, editor);
  const refreshOutline = setUpOutline(view);
  const refreshComments = setUpComments(view, editor);
  view.onSelectionChange = () => {
    const t = view.tableInfo();
    $("s-cell").textContent = t ? `row ${t.row + 1}, col ${t.col + 1} of ${t.rows} × ${t.cols}` : "–";
    refreshToolbar();
    refreshOutline();
    refreshComments();
  };
  view.relayout();
  view.focusEditor();

  $("zoom").addEventListener("change", (e) => view.setZoom(Number(e.target.value)));
  setUpTablePicker(view);
  setUpPageSetup(view);
  setUpMargins(view);
  // Downloads run in parallel, but faces register in list order so font ids don't depend on
  // network timing (the browser checks rely on them).
  const fontsLoaded = (async () => {
    for (const bytes of later) {
      editor.register_font(await bytes);
      view.relayout();
    }
  })();
  // Characters still drawn as boxes once the bundled fonts are in: fetch a font for them.
  const loadForMissing = setUpFontFallback(editor, view);
  view.on("edit", loadForMissing);
  view.on("remote", loadForMissing);
  void fontsLoaded.then(loadForMissing);
  setUpExport(editor, fontsLoaded);
  setUpTableMenu(view);
  setUpImages(view);

  if (local) {
    local.attach(view);
    local.onSaved = (ok) => {
      $("s-saved").textContent = ok ? "Saved on this device" : "Couldn't save on this device";
    };
  }
  const collab = DOC_ID ? startCollaboration(editor, view) : null;

  // Exposed for automated browser checks.
  window.opendox = { editor, view, collab, local };
  // Checks compare glyph fonts, so they wait for every face, not just first paint.
  await fontsLoaded;
  document.body.dataset.ready = "true";
}

function seedSample(editor) {
  const sections = [];
  for (let i = 1; i <= 4; i++) sections.push(`Section ${i}`, ...ALICE);
  const text = sections.join("\n");
  editor.insert(0, text);
  editor.layout();
  // Section titles are Heading 1 (style code 3), so they show in the outline.
  for (let i = 1; i <= 4; i++) {
    const at = cpLen(text.slice(0, text.indexOf(`Section ${i}`)));
    editor.set_paragraph_style(at, at, 3);
  }
  editor.layout();

  // A sample table at the end of Section 1.
  const endOfSection1 = cpLen(text.slice(0, text.indexOf("\nSection 2")));
  let cell = editor.insert_table(endOfSection1, CAST.length, CAST[0].length);
  for (const value of CAST.flat()) {
    editor.layout();
    const after = editor.insert(cell, value);
    editor.layout();
    cell = editor.next_cell(after) ?? after;
  }

  // The list, after everything else. Kinds: 0 plain, 1 bullets, 2 numbers, 3 checklist.
  editor.layout();
  let at = editor.len();
  for (const [line, kind, level = 0, ticked = false] of LIST) {
    at = editor.insert(at, `\n${line}`);
    editor.layout();
    // A new line continues the list (and level) of the line before it, as Enter does.
    editor.set_list(at, at, kind);
    editor.layout();
    for (let cur = editor.paragraph_format(at)[8]; kind && cur !== level; cur += cur < level ? 1 : -1) {
      editor.indent(at, at, cur < level);
      editor.layout();
    }
    if (kind === 3) editor.set_checked(at, ticked);
    // The list's introduction is a Heading 2 (style code 4).
    if (line === LIST[0][0]) editor.set_paragraph_style(at, at, 4);
    editor.layout();
  }
}

function startCollaboration(editor, view) {
  const [defaultName, color] = PEOPLE[Math.floor(Math.random() * PEOPLE.length)];
  const name = params.get("name") ?? defaultName;
  me = name;
  const url = `${SERVER}/doc/${encodeURIComponent(DOC_ID)}`;
  // A server started with --secret needs a token from your app: &token=... in the page URL.
  const token = params.get("token") ?? undefined;
  const collab = new Collaboration(editor, view, url, { name, color, token });
  const LABELS = { connecting: "Connecting…", connected: "Connected", offline: "Offline, retrying" };
  let viewOnly = false;
  collab.onReadOnly = () => {
    viewOnly = true;
    // Editing controls do nothing for a viewer; disabling them says so.
    const controls = "#format-bar button, #format-bar select, #insert-table, #insert-image, #margins-trigger, #page-setup-trigger";
    for (const el of document.querySelectorAll(controls)) el.disabled = true;
    $("s-sync").textContent = "View only";
  };
  $("sync").hidden = false;
  $("s-doc").textContent = DOC_ID;
  collab.onStatus = (state, peers) => {
    $("s-sync").textContent = viewOnly && state === "connected" ? "View only" : LABELS[state];
    $("s-sync").dataset.state = state;
    const you = personItem(`${name} (you)`, color);
    $("people").replaceChildren(you, ...peers.map((p) => personItem(p.name, p.color)));
  };
  collab.connect();
  return collab;
}

function personItem(name, color) {
  const li = document.createElement("li");
  const swatch = document.createElement("span");
  swatch.className = "swatch";
  swatch.style.background = color;
  li.append(swatch, name);
  return li;
}

/** File downloads: PDF (exactly as laid out) and .docx (for editing in Word). */
/** `fontsLoaded`: exports wait for every face so a PDF never embeds fallback glyphs. */
function setUpExport(editor, fontsLoaded) {
  const save = (bytes, name, type) => {
    const url = URL.createObjectURL(new Blob([bytes], { type }));
    const a = Object.assign(document.createElement("a"), { href: url, download: name });
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  };
  const title = "opendox demo";
  $("export-pdf").addEventListener("click", async () => {
    await fontsLoaded;
    save(editor.export_pdf(title), "opendox.pdf", "application/pdf");
  });
  $("export-docx").addEventListener("click", async () => {
    await fontsLoaded;
    save(
      editor.export_docx(title),
      "opendox.docx",
      "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    );
  });
}

// Colour choices for text and highlight (Google Docs' palette, light row then strong row).
const SWATCHES = [
  "#f28b82", "#fbbc04", "#fff475", "#ccff90", "#a7ffeb", "#cbf0f8", "#aecbfa", "#d7aefb",
  "#5f6368", "#c5221f", "#e8710a", "#f9ab00", "#188038", "#1967d2", "#9334e6", "#b31412",
];

/**
 * Loads an on-demand font (CJK) when the layout reports characters no registered font can draw
 * and that font covers them; adds its family to the default list and lays out again. A font
 * that isn't there (not downloaded) is tried once and left; those characters stay boxes.
 */
function setUpFontFallback(editor, view) {
  const families = [BASE_FAMILIES];
  const tried = new Set();
  let busy = false;
  return async () => {
    if (busy) return;
    busy = true;
    try {
      for (const f of ON_DEMAND) {
        // Asked again each time: a font just loaded may cover more than its own script.
        if (tried.has(f.family) || !f.needs.test(editor.missing_chars())) continue;
        tried.add(f.family);
        const res = await fetch(f.url).catch(() => null);
        if (!res?.ok) {
          console.info(`opendox demo: ${f.family} not found (run npm run fetch-fonts)`);
          continue;
        }
        editor.register_font(new Uint8Array(await res.arrayBuffer()));
        families.push(f.family);
        editor.set_default_font(families.join(", "), DEFAULT_SIZE);
        view.relayout();
      }
    } finally {
      busy = false;
    }
  };
}

/** Undo/redo, font, size, character formats, colours, links, alignment, lists and spacing.
 * Returns a refresh function. */
function setUpFormatBar(view, editor) {
  const bar = $("format-bar");
  // Clicking a toolbar button shouldn't take focus from the text (and hide the caret).
  bar.addEventListener("pointerdown", (e) => {
    if (e.target.closest("button")) e.preventDefault();
  });
  const run = (fn) => () => {
    fn();
    view.focusEditor();
  };
  $("undo").addEventListener("click", run(() => view.undo()));
  $("redo").addEventListener("click", run(() => view.redo()));
  $("bold").addEventListener("click", run(() => view.toggleBold()));
  $("italic").addEventListener("click", run(() => view.toggleItalic()));
  $("underline").addEventListener("click", run(() => view.toggleUnderline()));
  $("strike").addEventListener("click", run(() => view.toggleStrikethrough()));
  const refreshColors = setUpColorPanel(view);
  const refreshLink = setUpLinks(view, editor);
  for (const b of bar.querySelectorAll("[data-align]")) {
    b.addEventListener("click", run(() => view.setAlign(b.dataset.align)));
  }
  for (const b of bar.querySelectorAll("[data-list]")) {
    b.addEventListener("click", run(() => view.toggleList(b.dataset.list)));
  }
  $("indent").addEventListener("click", run(() => view.indent(true)));
  $("outdent").addEventListener("click", run(() => view.indent(false)));
  const onChange = (id, fn) => $(id).addEventListener("change", (e) => run(() => fn(e.target.value))());
  onChange("para-style", (v) => view.setParagraphStyle(v));
  onChange("font", (v) => view.setFont(v || null));
  onChange("font-size", (v) => view.setFontSize(Number(v) === DEFAULT_SIZE ? null : Number(v)));
  onChange("line-spacing", (v) => view.setLineSpacing(Number(v)));
  const spacing = () => view.setParagraphSpacing(Number($("space-before").value), Number($("space-after").value));
  onChange("space-before", spacing);
  onChange("space-after", spacing);
  const spacingPanel = popover($("spacing-trigger"), $("spacing-panel"));
  $("spacing-trigger").addEventListener("click", () => {
    if (spacingPanel.isOpen()) spacingPanel.close(false);
    else spacingPanel.open($("line-spacing"));
  });

  return () => {
    const s = view.formatState();
    $("undo").disabled = !s.canUndo;
    $("redo").disabled = !s.canRedo;
    $("bold").setAttribute("aria-pressed", String(s.bold));
    $("italic").setAttribute("aria-pressed", String(s.italic));
    $("underline").setAttribute("aria-pressed", String(s.underline));
    $("strike").setAttribute("aria-pressed", String(s.strikethrough));
    refreshColors(s);
    refreshLink(s);
    for (const b of bar.querySelectorAll("[data-align]")) {
      b.setAttribute("aria-pressed", String(b.dataset.align === s.align));
    }
    for (const b of bar.querySelectorAll("[data-list]")) {
      b.setAttribute("aria-pressed", String(b.dataset.list === s.list));
    }
    $("outdent").disabled = s.list === "none" && s.indentStart <= 0;
    $("para-style").value = s.style;
    $("font").value = s.font ?? "";
    // A size not in the list (a heading's 20pt, say) is added for as long as it's shown.
    const size = String(Math.round((s.size ?? DEFAULT_SIZE) * 10) / 10);
    const sizes = $("font-size");
    if (![...sizes.options].some((o) => o.value === size)) {
      sizes.querySelector("[data-extra]")?.remove();
      const extra = new Option(size, size);
      extra.dataset.extra = "";
      sizes.add(extra, [...sizes.options].find((o) => Number(o.value) > Number(size)) ?? null);
    }
    sizes.value = size;
    $("line-spacing").value = String(s.lineSpacing);
    $("space-before").value = String(s.spaceBefore);
    $("space-after").value = String(s.spaceAfter);
  };
}

/**
 * A panel opened by a toolbar button: Escape closes it and returns focus to the button, a
 * click anywhere else closes it. `trigger` may change (one panel serving two buttons).
 */
function popover(trigger, panel) {
  let owner = trigger;
  const api = {
    isOpen: () => !panel.hidden,
    owner: () => owner,
    open(focus, by = trigger) {
      if (owner !== by) owner.setAttribute("aria-expanded", "false");
      owner = by;
      panel.hidden = false;
      owner.setAttribute("aria-expanded", "true");
      focus?.focus();
    },
    close(refocus) {
      panel.hidden = true;
      owner.setAttribute("aria-expanded", "false");
      if (refocus) owner.focus();
    },
  };
  panel.addEventListener("keydown", (e) => {
    if (e.key === "Escape") api.close(true);
  });
  document.addEventListener("pointerdown", (e) => {
    if (api.isOpen() && !panel.contains(e.target) && !owner.contains(e.target)) api.close(false);
  });
  return api;
}

/**
 * Insert ▸ Header & footer: turn them on or off, add page numbers (a right-aligned number in
 * the footer, as Google Docs does), or insert a page number or count at the cursor.
 */
function setUpMargins(view) {
  const trigger = $("margins-trigger");
  const panel = $("margins-panel");
  const pop = popover(trigger, panel);
  const refresh = () => {
    const s = view.formatState();
    $("has-header").checked = s.hasHeader;
    $("has-footer").checked = s.hasFooter;
    const inMargin = s.region !== "body";
    $("insert-page-number").disabled = !inMargin;
    $("insert-page-count").disabled = !inMargin;
    $("field-hint").hidden = inMargin;
  };
  trigger.addEventListener("click", () => {
    if (pop.isOpen()) return pop.close(false);
    refresh();
    pop.open($("has-header"));
  });
  const done = () => {
    pop.close(false);
    view.focusEditor();
  };
  $("has-header").addEventListener("change", (e) => {
    view.setMargin("header", e.target.checked);
    done();
  });
  $("has-footer").addEventListener("change", (e) => {
    view.setMargin("footer", e.target.checked);
    done();
  });
  $("add-page-numbers").addEventListener("click", () => {
    view.setMargin("footer", true);
    view.setAlign("right");
    view.insertField("page");
    done();
  });
  for (const [id, kind] of [["insert-page-number", "page"], ["insert-page-count", "pages"]]) {
    $(id).addEventListener("pointerdown", (e) => e.preventDefault());
    $(id).addEventListener("click", () => {
      view.insertField(kind);
      done();
    });
  }
}

/** File ▸ Page setup: paper size, orientation and margins, shared with everyone editing. */
function setUpPageSetup(view) {
  const trigger = $("page-setup-trigger");
  const form = $("page-setup");
  const pop = popover(trigger, form);
  const sides = ["top", "bottom", "left", "right"];
  const error = $("page-setup-error");
  const inches = (pt) => String(Math.round((pt / 72) * 100) / 100);
  trigger.addEventListener("click", () => {
    if (pop.isOpen()) return pop.close(false);
    const p = view.pageSetup();
    const [short, long] = [Math.min(p.width, p.height), Math.max(p.width, p.height)];
    // The closest listed paper size (a custom size shows as the nearest one).
    const papers = [...$("paper").options].map((o) => o.value.split("x").map(Number));
    const best = papers.reduce((a, b) => (Math.abs(b[0] - short) + Math.abs(b[1] - long) < Math.abs(a[0] - short) + Math.abs(a[1] - long) ? b : a));
    $("paper").value = best.join("x");
    form.elements.orientation.value = p.width > p.height ? "landscape" : "portrait";
    for (const side of sides) $(`margin-${side}`).value = inches(p[side]);
    error.hidden = true;
    pop.open($("paper"));
  });
  $("page-setup-cancel").addEventListener("click", () => pop.close(true));
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    const [short, long] = $("paper").value.split("x").map(Number);
    const landscape = form.elements.orientation.value === "landscape";
    const m = Object.fromEntries(sides.map((side) => [side, Number($(`margin-${side}`).value) * 72]));
    try {
      view.setPageSetup({ width: landscape ? long : short, height: landscape ? short : long, ...m });
    } catch {
      error.textContent = "Those margins leave no room for text. Make them smaller.";
      error.hidden = false;
      return;
    }
    pop.close(false);
    view.focusEditor();
  });
}

/**
 * Comments beside the pages, as in Google Docs: one card per open comment at the height of
 * its text (pushed down so cards never overlap), the open one ringed and its text highlighted
 * more strongly. Ctrl+Alt+M or the toolbar button comments the selection (or the word at the
 * caret). Cards are updated in place, so a reply being typed survives others' edits.
 */
function setUpComments(view, editor) {
  const column = $("comments");
  const cards = new Map();
  let active = null;
  let draft = null;

  const timeOf = (ms) => new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  const button = (label, className, onClick) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = className;
    b.textContent = label;
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      onClick();
    });
    return b;
  };
  const iconButton = (label, path, onClick) => {
    const b = button("", "tool icon", onClick);
    b.setAttribute("aria-label", label);
    b.title = label;
    b.innerHTML = `<svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true"><path d="${path}" /></svg>`;
    return b;
  };

  const activate = (id, moveCaret) => {
    if (active === id) return;
    active = id;
    view.setActiveComment(id);
    if (id && moveCaret) {
      const c = view.comments().find((c) => c.id === id);
      if (c) view.goTo(c.start);
    }
    update();
  };

  /** The reply box at the bottom of the open card. */
  const replyBox = (id) => {
    const box = document.createElement("div");
    box.className = "reply-box";
    const area = document.createElement("textarea");
    area.placeholder = "Reply";
    area.setAttribute("aria-label", "Reply");
    const send = () => {
      if (!area.value.trim()) return;
      view.replyToComment(id, me, area.value.trim());
      area.value = "";
    };
    area.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        send();
      }
      if (e.key === "Escape") activate(null, false);
    });
    const row = document.createElement("div");
    row.className = "panel-row";
    row.append(button("Reply", "text-button primary", send));
    box.append(area, row);
    return box;
  };

  const makeCard = (id) => {
    const card = document.createElement("article");
    card.className = "comment";
    card.dataset.id = id;
    card.addEventListener("click", () => activate(id, true));
    cards.set(id, card);
    column.append(card);
    return card;
  };

  /** Fills a card's text, keeping its reply box (and whatever is typed in it). */
  const fill = (card, c) => {
    const key = JSON.stringify([c.author, c.text, c.created, c.replies, active === c.id]);
    if (card.dataset.key === key) return;
    card.dataset.key = key;
    const reply = card.querySelector(".reply-box");
    card.replaceChildren();
    const head = document.createElement("header");
    const who = document.createElement("span");
    who.className = "who";
    who.textContent = c.author || "Someone";
    head.append(
      who,
      iconButton("Resolve", "M4 10.5l4 4 8-9", () => {
        view.resolveComment(c.id);
        if (active === c.id) activate(null, false);
      }),
      iconButton("Delete", "M5 6h10M8 6V4h4v2M6.5 6l.7 10h5.6l.7-10", () => view.deleteComment(c.id)),
    );
    const when = document.createElement("time");
    when.textContent = timeOf(c.created);
    const text = document.createElement("p");
    text.textContent = c.start === c.end ? `${c.text}\n(the commented text was deleted)` : c.text;
    card.append(head, when, text);
    for (const r of c.replies) {
      const div = document.createElement("div");
      div.className = "reply";
      const name = document.createElement("strong");
      name.textContent = r.author;
      const t = document.createElement("time");
      t.textContent = timeOf(r.created);
      const p = document.createElement("p");
      p.textContent = r.text;
      div.append(name, t, p);
      card.append(div);
    }
    card.setAttribute("aria-current", String(active === c.id));
    if (active === c.id) card.append(reply ?? replyBox(c.id));
  };

  /** The y (document coordinates) of a position's line. */
  const yOf = (pos) => {
    const r = editor.caret_rect(pos, false);
    const page = r && view.renderer.pages[r[0]];
    if (!page) return 0;
    return page.getBoundingClientRect().top + window.scrollY + r[2] * view.renderer.cssPerPt;
  };

  const update = () => {
    const comments = view.comments().filter((c) => !c.resolved);
    const seen = new Set();
    const placed = [];
    for (const c of comments) {
      seen.add(c.id);
      const card = cards.get(c.id) ?? makeCard(c.id);
      fill(card, c);
      placed.push([card, yOf(c.start)]);
    }
    for (const [id, card] of cards) {
      if (!seen.has(id)) {
        card.remove();
        cards.delete(id);
      }
    }
    if (draft) placed.push([draft.card, yOf(Math.min(draft.anchor, draft.focus))]);
    // Beside the first page, top-aligned with each comment's line, never overlapping.
    const first = view.renderer.pages[0];
    if (first) {
      // Beside the page, but never under the scrollbar or past the window's edge.
      const room = document.documentElement.clientWidth - column.offsetWidth - 16;
      const left = Math.min(first.getBoundingClientRect().right + 16, room);
      column.style.left = `${left + window.scrollX}px`;
    }
    placed.sort((a, b) => a[1] - b[1]);
    let bottom = 0;
    for (const [card, y] of placed) {
      const top = Math.max(y, bottom + 8);
      card.style.top = `${top}px`;
      bottom = top + card.offsetHeight;
    }
  };

  /** A new comment on the selection, or on the word at the caret. */
  const start = () => {
    if (view.readOnly || draft) return;
    let { anchor, focus } = view.selection;
    if (anchor === focus) {
      [anchor, focus] = editor.word_range(focus);
      if (anchor === focus) return;
    }
    const card = document.createElement("article");
    card.className = "comment";
    card.setAttribute("aria-current", "true");
    const area = document.createElement("textarea");
    area.placeholder = "Add a comment";
    area.setAttribute("aria-label", "New comment");
    const cancel = () => {
      card.remove();
      draft = null;
      view.focusEditor();
      update();
    };
    const save = () => {
      if (!area.value.trim()) return;
      view.goTo(focus);
      // The view comments its own selection: set it to the range being commented.
      view.setSelection(anchor, focus);
      const id = view.addComment(me, area.value.trim());
      card.remove();
      draft = null;
      active = id;
      update();
    };
    area.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        save();
      }
      if (e.key === "Escape") cancel();
    });
    const row = document.createElement("div");
    row.className = "panel-row";
    row.append(button("Cancel", "text-button", cancel), button("Comment", "text-button primary", save));
    card.append(area, row);
    column.append(card);
    draft = { card, anchor, focus };
    update();
    area.focus();
  };

  $("comment-trigger").addEventListener("pointerdown", (e) => e.preventDefault());
  $("comment-trigger").addEventListener("click", start);
  view.onCommentRequest = start;
  window.addEventListener("resize", update);
  return () => {
    // The caret's comment opens its card, as clicking commented text does in Google Docs;
    // moving out of it closes the card.
    const here = view.commentAtCaret();
    if (here !== active) activate(here, false);
    else update();
  };
}

/**
 * The outline beside the pages: one entry per title or heading, indented by level; the one
 * containing the caret is marked. Clicking an entry moves the caret there.
 */
function setUpOutline(view) {
  const list = $("outline-list");
  // Keep the outline below the sticky header, whose height changes as the toolbar wraps.
  const header = document.querySelector("header");
  new ResizeObserver(() => {
    document.documentElement.style.setProperty("--header-height", `${header.offsetHeight}px`);
  }).observe(header);
  let shown = "";
  return () => {
    const headings = view.headings();
    const at = view.selection.focus;
    const current = headings.findLast((h) => h.pos <= at);
    const key = JSON.stringify([headings, current?.pos]);
    if (key === shown) return;
    shown = key;
    $("outline-empty").hidden = headings.length > 0;
    list.replaceChildren(
      ...headings.map((h) => {
        const li = document.createElement("li");
        const b = document.createElement("button");
        b.type = "button";
        b.textContent = h.text.trim() || "(empty heading)";
        b.style.paddingLeft = `${8 + Math.max(0, h.level - 1) * 12}px`;
        if (h.level === 0) b.style.fontWeight = "600";
        b.setAttribute("aria-current", String(h === current));
        b.addEventListener("click", () => view.goTo(h.pos));
        li.append(b);
        return li;
      }),
    );
  };
}

/** Text colour and highlight: one swatch panel under whichever button opened it. */
function setUpColorPanel(view) {
  const panel = $("color-panel");
  const grid = $("swatches");
  const triggers = [$("color-trigger"), $("highlight-trigger")];
  const pop = popover(triggers[0], panel);
  const target = () => pop.owner().dataset.target;
  const apply = (color) => {
    if (target() === "color") view.setTextColor(color);
    else view.setHighlight(color);
    pop.close(false);
    view.focusEditor();
  };
  for (const c of SWATCHES) {
    const b = document.createElement("button");
    b.type = "button";
    b.dataset.color = c;
    b.style.background = c;
    b.setAttribute("aria-label", c);
    b.setAttribute("aria-pressed", "false");
    b.addEventListener("click", () => apply(c));
    grid.append(b);
  }
  $("color-none").addEventListener("click", () => apply(null));
  $("color-custom").addEventListener("change", (e) => apply(e.target.value));
  let state = null;
  const mark = () => {
    const current = target() === "color" ? state?.color : state?.highlight;
    for (const b of grid.children) b.setAttribute("aria-pressed", String(b.dataset.color === current));
    $("color-none").textContent = target() === "color" ? "Default" : "None";
  };
  for (const t of triggers) {
    // Keep the caret visible: the panel is a toolbar extension, not a dialog that takes over.
    t.addEventListener("pointerdown", (e) => e.preventDefault());
    t.addEventListener("click", () => {
      if (pop.isOpen() && pop.owner() === t) return pop.close(false);
      // The panel is positioned in the header; place it under this button.
      const at = t.getBoundingClientRect();
      // (Measured from the header: the panel is still hidden, so it has no offset parent.)
      const header = panel.closest("header").getBoundingClientRect();
      panel.style.left = `${at.left - header.left}px`;
      panel.style.top = `${at.bottom - header.top + 4}px`;
      pop.open(null, t);
      mark();
      grid.querySelector('[aria-pressed="true"]')?.focus() ?? grid.firstElementChild.focus();
    });
  }
  return (s) => {
    state = s;
    $("color-bar").style.background = s.color ?? "var(--text)";
    $("highlight-bar").style.background = s.highlight ?? "transparent";
    if (pop.isOpen()) mark();
  };
}

/**
 * Links: the toolbar button and Ctrl+K open a URL field for the selection (or the link at the
 * caret); with the caret in a link, a bubble under it shows the link with Edit and Remove.
 */
function setUpLinks(view, editor) {
  const trigger = $("link-trigger");
  const form = $("link-panel");
  const input = $("link-url");
  const bubble = $("link-bubble");
  const pop = popover(trigger, form);
  const open = () => {
    view.selectLink();
    input.value = view.formatState().link ?? "";
    pop.open(input);
    input.select();
  };
  trigger.addEventListener("click", () => (pop.isOpen() ? pop.close(false) : open()));
  view.onLinkRequest = open;
  form.addEventListener("submit", (e) => {
    e.preventDefault();
    if (input.value.trim()) view.setLink(input.value);
    pop.close(false);
    view.focusEditor();
  });
  $("link-remove").addEventListener("click", () => {
    view.setLink("");
    pop.close(false);
    view.focusEditor();
  });
  bubble.addEventListener("pointerdown", (e) => {
    if (e.target.closest("button")) e.preventDefault();
  });
  $("bubble-edit").addEventListener("click", open);
  $("bubble-remove").addEventListener("click", () => {
    view.selectLink();
    view.setLink("");
    view.focusEditor();
  });

  return (s) => {
    const { anchor, focus } = view.selection;
    const show = s.link && anchor === focus && !pop.isOpen();
    bubble.hidden = !show;
    if (!show) return;
    const a = $("bubble-url");
    a.textContent = s.link;
    a.href = /^(https?:|mailto:)/i.test(s.link) ? s.link : `https://${s.link}`;
    // Under the caret's line.
    const r = editor.caret_rect(focus, false);
    const page = r && view.renderer.pages[r[0]];
    if (!page) return;
    const box = page.getBoundingClientRect();
    const k = view.renderer.cssPerPt;
    bubble.style.left = `${box.left + window.scrollX + r[1] * k}px`;
    bubble.style.top = `${box.top + window.scrollY + r[3] * k + 6}px`;
  };
}

/** Insert ▸ Table: hover or arrow through a 10 × 10 grid, click or Enter to insert. */
function setUpTablePicker(view) {
  const SIZE = 10;
  const trigger = $("insert-table");
  const picker = $("picker");
  const grid = $("grid");
  const label = $("size");
  const cells = [];
  for (let r = 1; r <= SIZE; r++) {
    for (let c = 1; c <= SIZE; c++) {
      const b = document.createElement("button");
      b.type = "button";
      b.dataset.r = String(r);
      b.dataset.c = String(c);
      b.setAttribute("aria-label", `${r} by ${c} table`);
      b.tabIndex = -1;
      cells.push(b);
      grid.append(b);
    }
  }
  const at = (r, c) => cells[(r - 1) * SIZE + (c - 1)];
  const highlight = (rows, cols) => {
    for (const b of cells) b.classList.toggle("on", +b.dataset.r <= rows && +b.dataset.c <= cols);
    label.textContent = `${rows} × ${cols}`;
  };
  const open = () => {
    picker.hidden = false;
    trigger.setAttribute("aria-expanded", "true");
    highlight(1, 1);
    at(1, 1).tabIndex = 0;
    at(1, 1).focus();
  };
  const close = (refocus) => {
    picker.hidden = true;
    trigger.setAttribute("aria-expanded", "false");
    for (const b of cells) b.tabIndex = -1;
    if (refocus) trigger.focus();
  };

  trigger.addEventListener("click", () => (picker.hidden ? open() : close(false)));
  grid.addEventListener("pointerover", (e) => {
    const b = e.target.closest("button");
    if (b) highlight(+b.dataset.r, +b.dataset.c);
  });
  grid.addEventListener("focusin", (e) => highlight(+e.target.dataset.r, +e.target.dataset.c));
  grid.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    close(false);
    view.insertTable(+b.dataset.r, +b.dataset.c);
    view.focusEditor();
  });
  picker.addEventListener("keydown", (e) => {
    const b = document.activeElement;
    if (e.key === "Escape") {
      close(true);
      return;
    }
    if (!b?.dataset.r) return;
    let [r, c] = [+b.dataset.r, +b.dataset.c];
    if (e.key === "ArrowRight") c = Math.min(SIZE, c + 1);
    else if (e.key === "ArrowLeft") c = Math.max(1, c - 1);
    else if (e.key === "ArrowDown") r = Math.min(SIZE, r + 1);
    else if (e.key === "ArrowUp") r = Math.max(1, r - 1);
    else return;
    e.preventDefault();
    b.tabIndex = -1;
    at(r, c).tabIndex = 0;
    at(r, c).focus();
  });
  document.addEventListener("pointerdown", (e) => {
    if (!picker.hidden && !picker.contains(e.target) && e.target !== trigger) close(false);
  });
}

/** Right-click inside a table: row and column commands, as in Google Docs. */
function setUpTableMenu(view) {
  const menu = $("table-menu");
  const items = [...menu.querySelectorAll("button")];
  const close = () => {
    menu.hidden = true;
  };
  // Runs after the editor's own contextmenu handler has moved the caret to the click.
  $("pages").addEventListener("contextmenu", (e) => {
    if (!view.tableInfo()) {
      close();
      return;
    }
    e.preventDefault();
    menu.hidden = false;
    const { width, height } = menu.getBoundingClientRect();
    menu.style.left = `${Math.min(e.clientX, innerWidth - width - 8)}px`;
    menu.style.top = `${Math.min(e.clientY, innerHeight - height - 8)}px`;
    items[0].focus();
  });
  menu.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    close();
    view.tableCommand(b.dataset.cmd);
    view.focusEditor();
  });
  menu.addEventListener("keydown", (e) => {
    const i = items.indexOf(document.activeElement);
    if (e.key === "Escape") {
      close();
      view.focusEditor();
    } else if (e.key === "ArrowDown") {
      items[(i + 1) % items.length].focus();
    } else if (e.key === "ArrowUp") {
      items[(i - 1 + items.length) % items.length].focus();
    } else {
      return;
    }
    e.preventDefault();
  });
  document.addEventListener("pointerdown", (e) => {
    if (!menu.hidden && !menu.contains(e.target)) close();
  });
  addEventListener("scroll", close, { passive: true });
}

main().catch((err) => {
  const p = document.createElement("p");
  p.id = "error";
  p.textContent = `Failed to start: ${err}`;
  document.body.append(p);
  console.error(err);
});
