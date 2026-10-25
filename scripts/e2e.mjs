// End-to-end check: drives the demo with real input events through the Chrome DevTools
// Protocol (clicks, drags, keys, typing, IME composition) and checks the document after each.
//
// Needs `npm run serve` running. Usage: node scripts/e2e.mjs [screenshot.png]
// Set CHROME to the browser binary if it isn't at the default Windows path.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2];
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const chrome = spawn(CHROME, [
  "--headless=new",
  "--remote-debugging-port=9334",
  "--window-size=1000,1000",
  "--user-data-dir=" + process.env.TEMP + "/opendox-interact-profile",
  "about:blank",
]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 50 && !targets; i++) {
  await sleep(200);
  targets = await fetch("http://127.0.0.1:9334/json").then((r) => r.json()).catch(() => null);
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

const KEYS = {
  ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40, Backspace: 8, Delete: 46,
  Enter: 13, Home: 36, End: 35, Shift: 16,
};
async function key(k, modifiers = 0) {
  const code = KEYS[k];
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: k, code: k, windowsVirtualKeyCode: code, modifiers });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code: k, windowsVirtualKeyCode: code, modifiers });
}
async function mouse(type, x, y, buttons = 1) {
  await send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons, clickCount: 1 });
}
/** Client coordinates of the caret position `pos` (via the engine), in the middle of the line. */
const clientOf = (pos) =>
  ev(`(() => { const {editor, view} = window.opendox; const r = editor.caret_rect(${pos}, false);
    const k = view.renderer.cssPerPt;
    // Scroll only if the point is hidden (under the sticky header or below the window), so
    // positions read earlier stay valid.
    const top = document.querySelector('header').getBoundingClientRect().bottom;
    let p = view.renderer.pages[r[0]].getBoundingClientRect(); const cy = p.top + (r[2] + r[3]) / 2 * k;
    if (cy < top + 8 || cy > innerHeight - 8) { scrollBy(0, cy - (top + innerHeight) / 2); p = view.renderer.pages[r[0]].getBoundingClientRect(); }
    const page = p;
    return [page.left + r[1] * k, page.top + (r[2] + r[3]) / 2 * k]; })()`);
const text = () => ev("window.opendox.editor.text()");
const sel = () => ev("(() => { const s = window.opendox.view.selection; return [s.anchor, s.focus]; })()");

await send("Page.navigate", { url: "http://127.0.0.1:5174/demo/" });
for (let i = 0; i < 100 && !(await ev("document.body?.dataset.ready === 'true'")); i++) await sleep(100);

// 1. Click inside "Alice" (char 10 = "Section 1\n" is 10 chars, so "Alice" starts at 10).
const original = await text();
let [x, y] = await clientOf(13);
await mouse("mousePressed", x, y);
await mouse("mouseReleased", x, y, 0);
check("click places caret", JSON.stringify(await sel()) === "[13,13]", JSON.stringify(await sel()));
check("textarea focused after click", await ev("document.activeElement?.className === 'opendox-input'"));

// 2. Typing inserts at the caret.
await send("Input.insertText", { text: "XYZ" });
check("typing inserts at caret", (await text()).startsWith("Section 1\nAliXYZce was"), (await text()).slice(0, 30));
check("caret after typed text", JSON.stringify(await sel()) === "[16,16]", JSON.stringify(await sel()));

// 3. Backspace x3 restores the text.
for (let i = 0; i < 3; i++) await key("Backspace");
check("backspace deletes", (await text()) === original);

// 4. Arrow keys.
await key("ArrowRight");
await key("ArrowRight");
check("arrow right moves 2", JSON.stringify(await sel()) === "[15,15]", JSON.stringify(await sel()));
await key("Home");
check("Home goes to line start", JSON.stringify(await sel()) === "[10,10]", JSON.stringify(await sel()));
await key("ArrowDown");
const down = (await sel())[1];
check("arrow down moves to next line", down > 60 && down < 140, String(down));
await key("ArrowUp");
check("arrow up returns to same column", JSON.stringify(await sel()) === "[10,10]", JSON.stringify(await sel()));

// 5. Shift+Right x5 selects, highlight boxes drawn, typing replaces the selection.
for (let i = 0; i < 5; i++) await key("ArrowRight", 8 /* shift */);
check("shift+arrow selects", JSON.stringify(await sel()) === "[10,15]", JSON.stringify(await sel()));
check("selection box drawn", (await ev("document.querySelectorAll('.opendox-selection').length")) === 1);
await send("Input.insertText", { text: "Bob" });
check("typing replaces selection", (await text()).startsWith("Section 1\nBob was beginning"), (await text()).slice(0, 30));

// 6. Enter splits the paragraph.
await key("Enter");
check("enter inserts paragraph break", (await text()).startsWith("Section 1\nBob\n was"), JSON.stringify((await text()).slice(0, 20)));

// 7. Drag selection across lines.
const [x1, y1] = await clientOf(10);
const [x2, y2] = await clientOf(40);
await mouse("mousePressed", x1, y1);
await mouse("mouseMoved", (x1 + x2) / 2, (y1 + y2) / 2);
await mouse("mouseMoved", x2, y2);
await mouse("mouseReleased", x2, y2, 0);
check("drag selects range", JSON.stringify(await sel()) === "[10,40]", JSON.stringify(await sel()));
const boxes = await ev("document.querySelectorAll('.opendox-selection').length");
check("multi-line selection draws several boxes", boxes >= 2, String(boxes));

// 8. IME composition (simulated: what a Japanese IME sends), then commit.
[x, y] = await clientOf(0);
await mouse("mousePressed", x, y);
await mouse("mouseReleased", x, y, 0);
await send("Input.imeSetComposition", { text: "にほん", selectionStart: 3, selectionEnd: 3 });
const composingVisible = await ev("document.querySelector('.opendox-input').classList.contains('opendox-composing')");
const midText = await text();
await send("Input.insertText", { text: "日本" });
check("IME preedit shown in textarea", composingVisible);
check("IME preedit not written to document", !midText.includes("にほん"));
check("IME commit inserts text", (await text()).startsWith("日本Section 1"), (await text()).slice(0, 15));
check("composing state cleared", !(await ev("document.querySelector('.opendox-input').classList.contains('opendox-composing')")));

// 9. Emoji: one Backspace removes the whole family emoji (5 chars).
await send("Input.insertText", { text: "👨‍👩‍👧" });
const lenBefore = await ev("window.opendox.editor.len()");
await key("Backspace");
check("backspace removes whole emoji", lenBefore - (await ev("window.opendox.editor.len()")) === 5);

// --- tables --------------------------------------------------------------------------------
await ev(`window.opendox.cellPos = (r, c, nth = 0) => { const e = window.opendox.editor; let seen = -1, last = null;
  for (let p = 0; p <= e.len(); p++) { const t = e.table_info(p); if (!t) { last = null; continue; }
    const id = t[2] + 'x' + t[3]; if (last === null) seen++; last = id;
    if (seen === nth && t[0] === r && t[1] === c) return p; } return -1; }`);
const cellPos = (r, c, nth = 0) => ev(`window.opendox.cellPos(${r}, ${c}, ${nth})`);
const info = () => ev("(() => { const t = window.opendox.view.tableInfo(); return t ? [t.row, t.col, t.rows, t.cols] : null; })()");
async function click(x, y, button = "left") {
  await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button, buttons: button === "left" ? 1 : 2, clickCount: 1 });
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button, buttons: 0, clickCount: 1 });
}
const rectOf = (sel) => ev(`(() => { const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);

// 11. Click into the sample table's second cell and type.
const sample01 = await cellPos(0, 1);
check("sample table present", sample01 > 0, String(sample01));
[x, y] = await clientOf(sample01);
await click(x + 2, y);
check("click lands in clicked cell", JSON.stringify((await info())?.slice(0, 2)) === "[0,1]", JSON.stringify(await info()));
await key("End");
await send("Input.insertText", { text: "!" });
check("typing goes into the cell", (await ev(`window.opendox.editor.slice(${sample01}, ${sample01} + 11)`)) === "First seen!");

// 12. Tab / Shift+Tab move between cells; Arrow Down moves to the cell below.
await key("Tab");
check("Tab moves to next cell", JSON.stringify((await info())?.slice(0, 2)) === "[0,2]", JSON.stringify(await info()));
await key("Tab", 8);
check("Shift+Tab moves back", JSON.stringify((await info())?.slice(0, 2)) === "[0,1]", JSON.stringify(await info()));
await key("ArrowDown");
check("Arrow Down goes to the cell below", JSON.stringify((await info())?.slice(0, 2)) === "[1,1]", JSON.stringify(await info()));

// 13. Backspace at the start of a cell changes nothing.
const before = await text();
await key("Home");
await key("Backspace");
check("Backspace at cell start is a no-op", (await text()) === before);

// 14. Tab in the last cell adds a row.
const last = await cellPos(2, 2);
[x, y] = await clientOf(last);
await click(x + 2, y);
await key("End");
await key("Tab");
check("Tab in last cell adds a row", JSON.stringify(await info()) === "[3,0,4,3]", JSON.stringify(await info()));

// 15. Insert table from the toolbar grid.
[x, y] = await clientOf(0);
await click(x, y);
await click(...(await rectOf("#insert-table")));
check("picker opens", !(await ev("document.getElementById('picker').hidden")));
const [gx, gy] = await rectOf('#grid button[data-r="2"][data-c="3"]');
await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: gx, y: gy });
check("picker shows hovered size", (await ev("document.getElementById('size').textContent")) === "2 × 3");
await click(gx, gy);
check("toolbar inserts a 2 × 3 table, caret in first cell", JSON.stringify(await info()) === "[0,0,2,3]", JSON.stringify(await info()));
check("picker closed", await ev("document.getElementById('picker').hidden"));
check("editor focused after insert", await ev("document.activeElement?.className === 'opendox-input'"));
await send("Input.insertText", { text: "new" });
check("type into new table", (await ev(`(() => { const e = window.opendox.editor; const p = window.opendox.view.selection.focus; return e.slice(p - 3, p); })()`)) === "new");

// 16. Right-click menu: insert column right, delete row, delete table.
[x, y] = await clientOf(await ev("window.opendox.view.selection.focus"));
await click(x, y, "right");
check("right-click in table opens menu", !(await ev("document.getElementById('table-menu').hidden")));
await click(...(await rectOf('#table-menu [data-cmd="insertColumnRight"]')));
check("insert column right", JSON.stringify(await info()) === "[0,1,2,4]", JSON.stringify(await info()));
[x, y] = await clientOf(await ev("window.opendox.view.selection.focus"));
await click(x, y, "right");
await click(...(await rectOf('#table-menu [data-cmd="deleteRow"]')));
check("delete row", JSON.stringify(await info()) === "[0,1,1,4]", JSON.stringify(await info()));
[x, y] = await clientOf(await ev("window.opendox.view.selection.focus"));
await click(x, y, "right");
await click(...(await rectOf('#table-menu [data-cmd="deleteTable"]')));
check("delete table", (await info()) === null, JSON.stringify(await info()));
check("menu closed", await ev("document.getElementById('table-menu').hidden"));
[x, y] = await clientOf(5);
await click(x, y, "right");
check("no table menu outside tables", await ev("document.getElementById('table-menu').hidden"));

// --- formatting and undo -------------------------------------------------------------------
const CTRL = 2;
const SHIFT = 8;
/** A Ctrl/Shift letter shortcut, e.g. chord("b", CTRL). */
async function chord(letter, modifiers) {
  const k = letter.toUpperCase();
  const base = { key: modifiers & SHIFT ? k : letter, code: `Key${k}`, windowsVirtualKeyCode: k.charCodeAt(0), modifiers };
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base });
  await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
}
const fmt = () => ev("(() => { const s = window.opendox.view.formatState(); return { bold: s.bold, italic: s.italic, align: s.align, font: s.font }; })()");
/** Font ids drawn on page 0. Demo order: 0 sans, 1 arabic, 2 bold, 3 italic, 4 bold italic, 5-8 serif. */
const fontsOnPage = () => ev(`(() => { const g = window.opendox.editor.page_glyphs(0); const s = new Set();
  for (let i = 0; i < g.length; i += 7) s.add(g[i]); return [...s].sort((a, b) => a - b); })()`);
const pressed = (id) => ev(`document.getElementById(${JSON.stringify(id)}).getAttribute('aria-pressed') === 'true'`);

// 17. Select "Section", Ctrl+B, undo, redo.
const s1 = (await text()).indexOf("Section 1");
[x, y] = await clientOf(s1);
await click(x, y);
for (let i = 0; i < 7; i++) await key("ArrowRight", SHIFT);
await chord("b", CTRL);
check("Ctrl+B bolds the selection", (await fontsOnPage()).includes(2) && (await fmt()).bold, JSON.stringify(await fontsOnPage()));
check("toolbar shows bold", await pressed("bold"));
await chord("z", CTRL);
check("Ctrl+Z undoes bold", !(await fontsOnPage()).includes(2), JSON.stringify(await fontsOnPage()));
check("undo restores the selection", JSON.stringify(await sel()) === `[${s1},${s1 + 7}]`, JSON.stringify(await sel()));
await chord("y", CTRL);
check("Ctrl+Y redoes it", (await fontsOnPage()).includes(2));

// 18. Ctrl+Shift+E centres the paragraph.
const leftX = (await clientOf(s1))[0];
await chord("e", CTRL | SHIFT);
check("Ctrl+Shift+E centres", (await fmt()).align === "center" && (await clientOf(s1))[0] > leftX + 100, `${(await fmt()).align} ${leftX} -> ${(await clientOf(s1))[0]}`);
check("toolbar shows centre", (await ev("document.querySelector('[data-align=center]').getAttribute('aria-pressed')")) === "true");

// 19. Ctrl+I at a caret applies to what's typed next; undo removes the typing.
const endOfHeading = s1 + 9;
[x, y] = await clientOf(endOfHeading);
await click(x + 1, y);
await key("End");
await chord("i", CTRL);
check("Ctrl+I at a caret shows italic pending", (await fmt()).italic);
const textBefore = await text();
await send("Input.insertText", { text: "!" });
check("typed text is italic", (await fontsOnPage()).includes(3), JSON.stringify(await fontsOnPage()));
await chord("z", CTRL);
check("undo removes the typed text", (await text()) === textBefore);

// 20. Toolbar font menu and the Undo button.
[x, y] = await clientOf(s1);
await click(x, y);
for (let i = 0; i < 7; i++) await key("ArrowRight", SHIFT);
await ev(`(() => { const s = document.getElementById('font'); s.value = 'Noto Serif'; s.dispatchEvent(new Event('change')); })()`);
check("font menu switches to serif (bold kept)", (await fontsOnPage()).includes(6) && (await fmt()).font === "Noto Serif", JSON.stringify(await fontsOnPage()));
check("editor keeps focus after the toolbar", await ev("document.activeElement?.className === 'opendox-input'"));
await click(...(await rectOf("#undo")));
check("Undo button reverts the font", !(await fontsOnPage()).includes(6) && (await fmt()).font === null);

// 21. Export: both files come out of the real document, PDF with one page per editor page.
const exported = await ev(`(() => { const e = window.opendox.editor; const pages = e.layout();
  const pdf = e.export_pdf('e2e'); const docx = e.export_docx('e2e');
  const head = (b, n) => String.fromCharCode(...b.slice(0, n));
  const pdfText = new TextDecoder('latin1').decode(pdf);
  return { pages, pdfHead: head(pdf, 5), pdfPages: (pdfText.match(/\\/Type \\/Page\\b/g) || []).length,
    docxHead: head(docx, 2), pdfKb: Math.round(pdf.length / 1024) }; })()`);
check("PDF export", exported.pdfHead === "%PDF-" && exported.pdfPages === exported.pages, JSON.stringify(exported));
check(".docx export is a zip", exported.docxHead === "PK", JSON.stringify(exported));
check("export buttons present", await ev("!!document.getElementById('export-pdf') && !!document.getElementById('export-docx')"));

// 10. Typing latency on a 3-page document.
const t = await ev(`(async () => { const v = window.opendox.view; const times = [];
  for (let i = 0; i < 20; i++) { const t0 = performance.now(); v.insertText('a'); times.push(performance.now() - t0); }
  times.sort((a, b) => a - b); return [times[10], times[19]]; })()`);
console.log(`keystroke (insert + layout + render 3 pages): median ${t[0].toFixed(1)} ms, max ${t[1].toFixed(1)} ms`);

// Screenshot with a selection for a visual check.
const [a1, b1] = await clientOf(30);
const [a2, b2] = await clientOf(120);
await mouse("mousePressed", a1, b1);
await mouse("mouseMoved", a2, b2);
await mouse("mouseReleased", a2, b2, 0);
if (OUT) {
  const shot = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width: 1000, height: 520, scale: 1 } });
  writeFileSync(OUT, Buffer.from(shot.result.data, "base64"));
}

console.log(failures ? `${failures} FAILED` : "ALL PASSED");
ws.close();
chrome.kill();
process.exit(failures ? 1 : 0);
