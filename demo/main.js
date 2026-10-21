import init, { Editor } from "../crates/opendox-wasm/pkg/opendox_wasm.js";
import { Collaboration, EditorView } from "../packages/editor/dist/index.js";

// Collaboration: open /demo/?doc=<id> in several tabs (needs `npm run sync-server`).
// Optional: &server=ws://host:port, &name=<display name>.
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

const FONTS = [
  "../fonts/noto/NotoSans-Regular.ttf",
  "../fonts/noto/NotoSans-Bold.ttf",
  "../fonts/noto/NotoSans-Italic.ttf",
  "../fonts/noto/NotoSans-BoldItalic.ttf",
  "../fonts/noto/NotoSerif-Regular.ttf",
  "../fonts/noto/NotoSerif-Bold.ttf",
  "../fonts/noto/NotoSerif-Italic.ttf",
  "../fonts/noto/NotoSerif-BoldItalic.ttf",
  "../fonts/noto/NotoSansArabic-Regular.ttf",
];
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

const $ = (id) => document.getElementById(id);
/** Length in Unicode code points, the unit the engine counts positions in. */
const cpLen = (s) => [...s].length;

async function main() {
  await init();
  const editor = new Editor();
  for (const url of FONTS) {
    const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
    editor.register_font(bytes);
  }
  editor.set_default_font("Noto Sans, Noto Sans Arabic", DEFAULT_SIZE);

  // A shared document starts from whatever the server has, so no sample text is seeded
  // (two tabs seeding at once would each add a copy).
  if (!DOC_ID) seedSample(editor);
  // Building the sample isn't something to undo.
  editor.clear_history();

  const view = new EditorView(editor, $("pages"));
  view.onUpdate = ({ layoutMs, render }) => {
    $("s-pages").textContent = String(render.pages);
    $("s-layout").textContent = `${layoutMs.toFixed(1)} ms`;
    $("s-render").textContent = `${render.ms.toFixed(1)} ms`;
  };
  const refreshToolbar = setUpFormatBar(view);
  view.onSelectionChange = () => {
    const t = view.tableInfo();
    $("s-cell").textContent = t ? `row ${t.row + 1}, col ${t.col + 1} of ${t.rows} × ${t.cols}` : "–";
    refreshToolbar();
  };
  view.relayout();
  view.focusEditor();

  $("zoom").addEventListener("change", (e) => view.setZoom(Number(e.target.value)));
  setUpTablePicker(view);
  setUpExport(editor);
  setUpTableMenu(view);

  const collab = DOC_ID ? startCollaboration(editor, view) : null;

  // Exposed for automated browser checks.
  window.opendox = { editor, view, collab };
  document.body.dataset.ready = "true";
}

function seedSample(editor) {
  const sections = [];
  for (let i = 1; i <= 4; i++) sections.push(`Section ${i}`, ...ALICE);
  const text = sections.join("\n");
  editor.insert(0, text);
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
}

function startCollaboration(editor, view) {
  const [defaultName, color] = PEOPLE[Math.floor(Math.random() * PEOPLE.length)];
  const name = params.get("name") ?? defaultName;
  const url = `${SERVER}/doc/${encodeURIComponent(DOC_ID)}`;
  const collab = new Collaboration(editor, view, url, { name, color });
  const LABELS = { connecting: "Connecting…", connected: "Connected", offline: "Offline, retrying" };
  $("sync").hidden = false;
  $("s-doc").textContent = DOC_ID;
  collab.onStatus = (state, peers) => {
    $("s-sync").textContent = LABELS[state];
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
function setUpExport(editor) {
  const save = (bytes, name, type) => {
    const url = URL.createObjectURL(new Blob([bytes], { type }));
    const a = Object.assign(document.createElement("a"), { href: url, download: name });
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  };
  const title = "opendox demo";
  $("export-pdf").addEventListener("click", () =>
    save(editor.export_pdf(title), "opendox.pdf", "application/pdf"),
  );
  $("export-docx").addEventListener("click", () =>
    save(
      editor.export_docx(title),
      "opendox.docx",
      "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
  );
}

/** Undo/redo, font, size, bold, italic, alignment and spacing. Returns a refresh function. */
function setUpFormatBar(view) {
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
  for (const b of bar.querySelectorAll("[data-align]")) {
    b.addEventListener("click", run(() => view.setAlign(b.dataset.align)));
  }
  const onChange = (id, fn) => $(id).addEventListener("change", (e) => run(() => fn(e.target.value))());
  onChange("font", (v) => view.setFont(v || null));
  onChange("font-size", (v) => view.setFontSize(Number(v) === DEFAULT_SIZE ? null : Number(v)));
  onChange("line-spacing", (v) => view.setLineSpacing(Number(v)));
  const spacing = () => view.setParagraphSpacing(Number($("space-before").value), Number($("space-after").value));
  onChange("space-before", spacing);
  onChange("space-after", spacing);

  return () => {
    const s = view.formatState();
    $("undo").disabled = !s.canUndo;
    $("redo").disabled = !s.canRedo;
    $("bold").setAttribute("aria-pressed", String(s.bold));
    $("italic").setAttribute("aria-pressed", String(s.italic));
    for (const b of bar.querySelectorAll("[data-align]")) {
      b.setAttribute("aria-pressed", String(b.dataset.align === s.align));
    }
    $("font").value = s.font ?? "";
    $("font-size").value = String(s.size ?? DEFAULT_SIZE);
    $("line-spacing").value = String(s.lineSpacing);
    $("space-before").value = String(s.spaceBefore);
    $("space-after").value = String(s.spaceAfter);
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
