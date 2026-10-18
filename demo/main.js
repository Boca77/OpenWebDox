import init, { Editor } from "../crates/opendox-wasm/pkg/opendox_wasm.js";
import { EditorView } from "../packages/editor/dist/index.js";

const FONTS = [
  "../fonts/noto/NotoSans-Regular.ttf",
  "../fonts/noto/NotoSans-Bold.ttf",
  "../fonts/noto/NotoSansArabic-Regular.ttf",
];

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
  editor.set_default_font("Noto Sans, Noto Sans Arabic", 11);

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

  const view = new EditorView(editor, $("pages"));
  view.onUpdate = ({ layoutMs, render }) => {
    $("s-pages").textContent = String(render.pages);
    $("s-layout").textContent = `${layoutMs.toFixed(1)} ms`;
    $("s-render").textContent = `${render.ms.toFixed(1)} ms`;
  };
  view.onSelectionChange = () => {
    const t = view.tableInfo();
    $("s-cell").textContent = t ? `row ${t.row + 1}, col ${t.col + 1} of ${t.rows} × ${t.cols}` : "–";
  };
  view.relayout();
  view.focusEditor();

  $("zoom").addEventListener("change", (e) => view.setZoom(Number(e.target.value)));
  setUpTablePicker(view);
  setUpTableMenu(view);

  // Exposed for automated browser checks.
  window.opendox = { editor, view };
  document.body.dataset.ready = "true";
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
