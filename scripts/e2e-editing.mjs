// End-to-end check for editing by words and the clipboard: Ctrl+Arrow, Ctrl+Shift+Arrow,
// Ctrl+Backspace/Delete, double/triple-click, Shift+click; copying formatted text and pasting
// it back exactly; pasting Google Docs-style HTML.
//
// Needs `npm run serve` running. Usage: node scripts/e2e-editing.mjs
// Set CHROME to the browser binary if it isn't at the default Windows path.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2];
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const chrome = spawn(CHROME, [
  "--headless=new",
  "--remote-debugging-port=9348",
  "--window-size=1000,1000",
  "--user-data-dir=" + process.env.TEMP + "/opendox-editing-profile",
  "about:blank",
]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 50 && !targets; i++) {
  await sleep(200);
  targets = await fetch("http://127.0.0.1:9348/json").then((r) => r.json()).catch(() => null);
}
const ws = new WebSocket(targets.find((t) => t.type === "page").webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0;
const pending = new Map();
ws.addEventListener("message", (e) => {
  const m = JSON.parse(e.data);
  if (pending.has(m.id)) pending.get(m.id)(m);
});
const send = (method, params = {}) =>
  new Promise((r) => {
    pending.set(++id, r);
    ws.send(JSON.stringify({ id, method, params }));
  });
const ev = async (expr) => {
  const m = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (m.result?.exceptionDetails) throw new Error(JSON.stringify(m.result.exceptionDetails));
  return m.result.result.value;
};

let failures = 0;
const check = (name, ok, detail = "") => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : "  -> " + detail}`);
  if (!ok) failures++;
};

const ALT = 1;
const CTRL = 2;
const SHIFT = 8;
const KEYS = {
  Enter: [13, "Enter"], ArrowLeft: [37, "ArrowLeft"], ArrowRight: [39, "ArrowRight"],
  Backspace: [8, "Backspace"], Delete: [46, "Delete"],
};
async function key(k, modifiers = 0) {
  const [code, physical] = KEYS[k];
  // Enter as a full key press (with its text), so it also submits a focused form field.
  const text = k === "Enter" ? { type: "keyDown", text: "\r" } : { type: "rawKeyDown" };
  await send("Input.dispatchKeyEvent", { ...text, key: k, code: physical, windowsVirtualKeyCode: code, modifiers });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code: physical, windowsVirtualKeyCode: code, modifiers });
}
async function click(x, y) {
  await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 });
}
const type = (text) => send("Input.insertText", { text });
/** Client coordinates of caret position `pos`, scrolled into view if hidden; `dx` in points. */
const clientOf = (pos, dx = 0) =>
  ev(`(() => { const {editor, view} = window.opendox; const r = editor.caret_rect(${pos}, false);
    const k = view.renderer.cssPerPt;
    // Scroll only if the point is hidden (under the sticky header or below the window).
    const top = document.querySelector('header').getBoundingClientRect().bottom;
    let page = view.renderer.pages[r[0]].getBoundingClientRect(); const cy = page.top + (r[2] + r[3]) / 2 * k;
    if (cy < top + 8 || cy > innerHeight - 8) { scrollBy(0, cy - (top + innerHeight) / 2); page = view.renderer.pages[r[0]].getBoundingClientRect(); }
    return [page.left + (r[1] + ${dx}) * k, page.top + (r[2] + r[3]) / 2 * k]; })()`);
const caret = () => ev("window.opendox.view.selection.focus");
/** Selects through the view's API, as a toolbar would see it, and refreshes the toolbar. */
const select = (a, b) =>
  ev(`(() => { const v = window.opendox.view; v.anchor = ${a}; v.focus = ${b}; v.focusEditor(); v.relayout(); })()`);
const fmt = () => ev("window.opendox.view.formatState()");
/** Glyph colours (numbers, -1 = ink) and decoration kinds drawn on page `p`. */
const drawn = (p) =>
  ev(`(() => { const e = window.opendox.editor; e.layout(); const g = e.page_glyphs(${p}); const d = e.page_decorations(${p});
    const colors = new Set(); for (let i = 0; i < g.length; i += 7) colors.add(g[i + 6]);
    const kinds = []; for (let i = 0; i < d.length; i += 6) kinds.push(d[i]); return { colors: [...colors], kinds }; })()`);
const posOf = (s) =>
  ev(`(() => { const e = window.opendox.editor; const want = ${JSON.stringify(s)};
    for (let p = 0; p <= e.len(); p++) { const [a, b] = e.paragraph_range(p); if (e.slice(a, b) === want) return a; }
    return -1; })()`);
const hidden = (id) => ev(`document.getElementById(${JSON.stringify(id)}).hidden`);
const clickEl = (sel) => ev(`document.querySelector(${JSON.stringify(sel)}).click()`);

/** A click with a click count (2 = double, 3 = triple) and modifiers. */
async function clickN(x, y, count, modifiers = 0) {
  // Separate gestures: presses within half a second at one spot count as one multi-click.
  await sleep(600);
  for (let n = 1; n <= count; n++) {
    await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: n, modifiers });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: n, modifiers });
  }
}
const sel = () => ev("(() => { const s = window.opendox.view.selection; return [s.anchor, s.focus]; })()");
const text = () => ev("window.opendox.editor.text()");
/** Copy (or cut) through a real clipboard event; returns what was put on the clipboard. */
const copy = (cut = false) =>
  ev(`(() => { const dt = new DataTransfer(); const ev = new ClipboardEvent(${JSON.stringify(cut ? "cut" : "copy")}, { clipboardData: dt, bubbles: true, cancelable: true });
    document.querySelector('.opendox-input').dispatchEvent(ev); return { text: dt.getData('text/plain'), html: dt.getData('text/html') }; })()`);
/** Paste HTML (and its plain text) through a real clipboard event. */
const paste = (html, plain = "") =>
  ev(`(() => { const dt = new DataTransfer(); dt.setData('text/html', ${JSON.stringify(html)}); dt.setData('text/plain', ${JSON.stringify(plain)});
    document.querySelector('.opendox-input').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true })); })()`);
const formatsAt = (pos) => ev(`(() => { const e = window.opendox.editor; const c = Array.from(e.char_format(${pos}, ${pos + 1}));
  const p = Array.from(e.paragraph_format(${pos})); return { bold: c[0] === 1, underline: c[3] === 1, color: e.char_color(${pos}, ${pos + 1}), link: e.link_at(${pos}) ?? null, list: p[7], level: p[8], style: p[10] }; })()`);

await send("Page.navigate", { url: "http://127.0.0.1:5174/demo/" });
for (let i = 0; i < 100 && !(await ev("document.body?.dataset.ready === 'true'")); i++) await sleep(100);

// Section 1's first paragraph starts at 10: "Alice was beginning to get very tired…"
// 1. Ctrl+Right steps to word starts; Ctrl+Left back.
await select(10, 10);
await key("ArrowRight", CTRL);
check("Ctrl+Right to the next word", (await caret()) === 16, String(await caret()));
await key("ArrowRight", CTRL);
check("…and the next", (await caret()) === 20, String(await caret()));
await key("ArrowLeft", CTRL);
check("Ctrl+Left back to that word's start", (await caret()) === 16, String(await caret()));
// 2. Ctrl+Shift+Right selects word by word.
await key("ArrowRight", CTRL | SHIFT);
check("Ctrl+Shift+Right selects a word", JSON.stringify(await sel()) === "[16,20]", JSON.stringify(await sel()));

// 3. Ctrl+Backspace deletes the word before; Ctrl+Delete the one after.
const original = await text();
await select(20, 20);
await key("Backspace", CTRL);
check("Ctrl+Backspace deletes the word before", (await text()).startsWith("Section 1\nAlice beginning"), JSON.stringify((await text()).slice(0, 30)));
await key("Delete", CTRL);
check("Ctrl+Delete deletes the word after", (await text()).startsWith("Section 1\nAlice to get"), JSON.stringify((await text()).slice(0, 30)));
await ev("window.opendox.view.undo()");
await ev("window.opendox.view.undo()");
check("undo brings both back", (await text()) === original);

// 4. Double-click selects a word, triple-click the paragraph; Shift+click extends.
let [x, y] = await clientOf(12);
await clickN(x, y, 2);
check("double-click selects the word", JSON.stringify(await sel()) === "[10,15]", JSON.stringify(await sel()));
await clickN(x, y, 3);
const para = await ev("Array.from(window.opendox.editor.paragraph_range(12))");
check("triple-click selects the paragraph", JSON.stringify(await sel()) === JSON.stringify(para), `${JSON.stringify(await sel())} vs ${JSON.stringify(para)}`);
await sleep(600);
await click(x, y);
[x, y] = await clientOf(30);
await clickN(x, y, 1, SHIFT);
const s = await sel();
check("Shift+click extends the selection", s[0] === 12 && s[1] === 30, JSON.stringify(s));

// 5. Copy formatted text: plain text and HTML with tags and the exact content.
await select(10, 15);
await ev("window.opendox.view.toggleBold(); window.opendox.view.setTextColor('#c5221f');");
await select(10, 20);
const copied = await copy();
check("copy puts plain text", copied.text === "Alice was ", JSON.stringify(copied.text));
check("…and HTML with formatting", copied.html.includes("<b>") && copied.html.includes("color:#c5221f"), copied.html.slice(0, 200));
check("…carrying the exact content", copied.html.includes("data-opendox-clip"));

// 6. Pasting it back at the end of the document keeps bold and colour.
const end = await ev("window.opendox.editor.len()");
await select(end, end);
await key("Enter");
const at = await caret();
await paste(copied.html, copied.text);
check("paste inserts the text", (await ev(`window.opendox.editor.slice(${at}, ${at + 10})`)) === "Alice was ");
let f = await formatsAt(at);
check("pasted text keeps bold and colour", f.bold && f.color === "#c5221f", JSON.stringify(f));
check("…and the rest stays plain", !(await formatsAt(at + 6)).bold);
await ev("window.opendox.view.undo()");
check("a paste is one undo step", (await ev(`window.opendox.editor.slice(${at}, ${at + 5})`)) !== "Alice");

// 7. Google Docs-style HTML: heading, bold, red, link, bullets with a nested level.
const gdocs = `<meta charset="utf-8"><b style="font-weight:normal;" id="docs-internal-guid-1">
  <h1 dir="ltr"><span style="font-size:20pt;font-weight:400;color:#000000;">Pasted title</span></h1>
  <p dir="ltr"><span style="font-weight:700;">Bold</span><span> and </span><span style="color:#ff0000;">red</span><span> and </span><a href="https://example.org"><span style="text-decoration:underline;">a link</span></a></p>
  <ul><li dir="ltr" aria-level="1"><p dir="ltr"><span>first</span></p></li><li dir="ltr" aria-level="2"><p dir="ltr"><span>nested</span></p></li></ul></b>`;
const at2 = await caret();
await paste(gdocs, "Pasted title\nBold and red and a link\nfirst\nnested");
const t2 = await ev(`window.opendox.editor.slice(${at2}, window.opendox.editor.len())`);
check("Docs paste brings the paragraphs", t2 === "Pasted title\nBold and red and a link\nfirst\nnested", JSON.stringify(t2));
check("the heading is Heading 1", (await formatsAt(at2)).style === 3, JSON.stringify(await formatsAt(at2)));
const p2 = at2 + "Pasted title\n".length;
check("bold kept", (await formatsAt(p2)).bold);
check("not bold where Docs said normal", !(await formatsAt(at2)).bold);
check("red kept", (await formatsAt(p2 + 9)).color === "#ff0000", JSON.stringify(await formatsAt(p2 + 9)));
check("link kept", (await formatsAt(p2 + 17)).link === "https://example.org", JSON.stringify(await formatsAt(p2 + 17)));
const li = p2 + "Bold and red and a link\n".length;
check("bullets become a list", (await formatsAt(li)).list === 1);
check("…with the nested level", (await formatsAt(li + 6)).level === 1, JSON.stringify(await formatsAt(li + 6)));

// 8. Cut removes the selection and puts it on the clipboard.
await select(li, li + 5);
const cut = await copy(true);
check("cut copies", cut.text === "first");
check("…and deletes", (await ev(`window.opendox.editor.slice(${li}, ${li + 1})`)) !== "f");

// 9. Emoji and CJK: drawn with real glyphs (emoji from the bundled font; CJK fonts load on
// demand when `npm run fetch-fonts` has downloaded them).
const haveCjk = await ev(`fetch("../fonts/noto/NotoSansSC-Regular.otf", { method: "HEAD" }).then((r) => r.ok, () => false)`);
const endPos = await ev("window.opendox.editor.len()");
await select(endPos, endPos);
await key("Enter");
await type("Emoji 😀👨‍👩‍👧");
check("emoji have glyphs", !/[😀👨]/u.test(await ev("window.opendox.editor.missing_chars()")), await ev("window.opendox.editor.missing_chars()"));
await key("Enter");
await type("中文 한국어 日本語のかな");
if (haveCjk) {
  const deadline = Date.now() + 8000;
  while (Date.now() < deadline && (await ev("window.opendox.editor.missing_chars()")) !== "") await sleep(100);
  check("CJK fonts load and cover the text", (await ev("window.opendox.editor.missing_chars()")) === "", await ev("window.opendox.editor.missing_chars()"));
} else {
  console.log("SKIP  CJK fonts not downloaded (npm run fetch-fonts)");
}

console.log(failures ? `${failures} FAILED` : "ALL PASSED");
ws.close();
chrome.kill();
process.exit(failures ? 1 : 0);
