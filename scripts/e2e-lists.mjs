// End-to-end check for lists: drives the demo through the Chrome DevTools Protocol (typing
// shortcuts like "* ", Enter, Tab, Backspace, Ctrl+Shift+7, checkbox clicks) and checks the
// document's list state after each, plus that markers are drawn and both exports carry lists.
//
// Needs `npm run serve` running. Usage: node scripts/e2e-lists.mjs [screenshot.png]
// Set CHROME to the browser binary if it isn't at the default Windows path.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2];
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const chrome = spawn(CHROME, [
  "--headless=new",
  "--remote-debugging-port=9336",
  "--window-size=1000,1000",
  "--user-data-dir=" + process.env.TEMP + "/opendox-lists-profile",
  "about:blank",
]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 50 && !targets; i++) {
  await sleep(200);
  targets = await fetch("http://127.0.0.1:9336/json").then((r) => r.json()).catch(() => null);
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

const CTRL = 2;
const SHIFT = 8;
const KEYS = {
  Backspace: [8, "Backspace"], Enter: [13, "Enter"], Home: [36, "Home"], End: [35, "End"], Tab: [9, "Tab"],
  "&": [55, "Digit7"], "*": [56, "Digit8"],
};
async function key(k, modifiers = 0) {
  const [code, physical] = KEYS[k];
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: k, code: physical, windowsVirtualKeyCode: code, modifiers });
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
/** `[list, level, checked]` of the paragraph at `pos` (list: 0 none, 1 bullet, 2 number, 3 check). */
const listAt = (pos) => ev(`(() => { const f = window.opendox.editor.paragraph_format(${pos}); return [f[7], f[8], f[9]]; })()`);
const caret = () => ev("window.opendox.view.selection.focus");
const text = () => ev("window.opendox.editor.text()");
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

await send("Page.navigate", { url: "http://127.0.0.1:5174/demo/" });
for (let i = 0; i < 100 && !(await ev("document.body?.dataset.ready === 'true'")); i++) await sleep(100);

// 1. The sample list: numbers with nested levels, then a checklist with one ticked item.
// The table sits before the list, so text offsets and flat positions differ: ask the engine.
const posOf = (s) =>
  ev(`(() => { const e = window.opendox.editor; const want = ${JSON.stringify(s)};
    for (let p = 0; p <= e.len(); p++) { const [a, b] = e.paragraph_range(p); if (e.slice(a, b) === want) return a; }
    return -1; })()`);
const cupboards = await posOf("Cupboards and book-shelves");
const empty = await posOf("It was empty");
const drink = await posOf("Drink the bottle marked “DRINK ME”");
check("sample numbered item", same(await listAt(cupboards), [2, 0, 0]), JSON.stringify(await listAt(cupboards)));
check("sample nested item", same(await listAt(empty), [2, 2, 0]), JSON.stringify(await listAt(empty)));
check("sample ticked item", same(await listAt(drink), [3, 0, 1]), JSON.stringify(await listAt(drink)));
const symbolFont = await ev(`(() => { const e = window.opendox.editor; const n = e.layout(); const g = e.page_glyphs(n - 1);
  const fonts = new Set(); for (let i = 0; i < g.length; i += 7) fonts.add(g[i]); return [...fonts]; })()`);
check("markers drawn from the symbols font", symbolFont.includes(9), JSON.stringify(symbolFont));

// 2. Markers sit in the hanging indent: text at 1.5 in from the page edge for level 0.
const textX = (await ev(`window.opendox.editor.caret_rect(${cupboards}, false)`))[1];
check("level 0 text at margin + 36pt", Math.abs(textX - 108) < 0.01, String(textX));
const nestedX = (await ev(`window.opendox.editor.caret_rect(${empty}, false)`))[1];
check("level 2 text at margin + 108pt", Math.abs(nestedX - 180) < 0.01, String(nestedX));

if (OUT) {
  const [, y] = await clientOf(cupboards);
  const shot = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: Math.max(0, y - 60) + (await ev("window.scrollY")), width: 1000, height: 300, scale: 1 } });
  writeFileSync(OUT, Buffer.from(shot.result.data, "base64"));
}

// 3. Clicking a checkbox ticks or clears it; undo brings the tick back.
let [x, y] = await clientOf(drink, -12);
await click(x, y);
check("click on checkbox clears it", same(await listAt(drink), [3, 0, 0]), JSON.stringify(await listAt(drink)));
await click(x, y);
check("second click ticks it again", same(await listAt(drink), [3, 0, 1]), JSON.stringify(await listAt(drink)));
await ev("window.opendox.view.undo()");
check("undo clears the tick", same(await listAt(drink), [3, 0, 0]), JSON.stringify(await listAt(drink)));
await ev("window.opendox.view.redo()");

// 4. At the end of the document: Enter after a checklist item continues the checklist, unticked
// even after a ticked one; Enter on the empty item ends the list.
const end = await ev("window.opendox.editor.len()");
[x, y] = await clientOf(end);
await click(x + 40, y);
check("caret at the end", (await caret()) === end, String(await caret()));
await key("Enter");
check("Enter continues the checklist", same(await listAt(await caret()), [3, 0, 0]), JSON.stringify(await listAt(await caret())));
await key("Enter");
check("Enter on an empty item ends the list", same(await listAt(await caret()), [0, 0, 0]), JSON.stringify(await listAt(await caret())));

// 5. "* " at the start of a plain paragraph makes a bullet and the asterisk goes away.
await type("*");
await type(" ");
const bulletStart = await caret();
check("'* ' starts a bulleted list", same(await listAt(bulletStart), [1, 0, 0]), JSON.stringify(await listAt(bulletStart)));
check("the '*' was removed", !(await text()).endsWith("*"), JSON.stringify((await text()).slice(-5)));
await type("alpha");
await key("Enter");
await type("beta");
check("Enter continues the bullets", same(await listAt(await caret()), [1, 0, 0]));

// 6. Tab and Shift+Tab change the level; Ctrl+Shift+7 switches to numbers.
await key("Tab");
check("Tab nests the item", same(await listAt(await caret()), [1, 1, 0]), JSON.stringify(await listAt(await caret())));
check("Tab didn't move focus away", await ev("document.activeElement?.className === 'opendox-input'"));
await key("Tab", SHIFT);
check("Shift+Tab un-nests it", same(await listAt(await caret()), [1, 0, 0]), JSON.stringify(await listAt(await caret())));
await key("&", CTRL | SHIFT);
check("Ctrl+Shift+7 makes it numbered", same(await listAt(await caret()), [2, 0, 0]), JSON.stringify(await listAt(await caret())));
await key("&", CTRL | SHIFT);
check("again turns the list off", same(await listAt(await caret()), [0, 0, 0]), JSON.stringify(await listAt(await caret())));
await key("*", CTRL | SHIFT);
check("Ctrl+Shift+8 makes it a bullet", same(await listAt(await caret()), [1, 0, 0]));

// 7. Backspace at the start of an item takes it out of the list and keeps its text.
const before = await text();
await key("Home");
await key("Backspace");
check("Backspace at the start leaves the list", same(await listAt(await caret()), [0, 0, 0]), JSON.stringify(await listAt(await caret())));
check("…without deleting anything", (await text()) === before);

// 8. Toolbar: the bullets button reflects the caret's list and toggles it.
[x, y] = await clientOf(cupboards, 10);
await click(x, y);
check("numbered button pressed in a numbered item",
  (await ev(`document.querySelector('[data-list="number"]').getAttribute('aria-pressed')`)) === "true");
await ev(`document.querySelector('[data-list="bullet"]').click()`);
check("bullets button switches the item", same(await listAt(cupboards), [1, 0, 0]), JSON.stringify(await listAt(cupboards)));
await ev(`document.getElementById('indent').click()`);
check("indent button nests it", same(await listAt(cupboards), [1, 1, 0]), JSON.stringify(await listAt(cupboards)));

// 9. Exports: the .docx gets Word numbering, the PDF draws the markers' glyphs.
const exported = await ev(`(() => { const e = window.opendox.editor;
  const docx = new TextDecoder("latin1").decode(e.export_docx(""));
  const pdf = new TextDecoder("latin1").decode(e.export_pdf(""));
  return { numbering: docx.includes("word/numbering.xml"), symbols: pdf.includes("NotoSansSymbols2") }; })()`);
check(".docx has numbering", exported.numbering);
check("PDF embeds the symbols font", exported.symbols);

console.log(failures ? `${failures} FAILED` : "ALL PASSED");
ws.close();
chrome.kill();
process.exit(failures ? 1 : 0);
