// End-to-end check for page layout: Ctrl+Enter page breaks, Backspace removing one, page
// setup (paper size, orientation, margins), headers and footers with page numbers, and both
// exports.
//
// Needs `npm run serve` running. Usage: node scripts/e2e-pages.mjs [screenshot.png]
// Set CHROME to the browser binary if it isn't at the default Windows path.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2];
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const chrome = spawn(CHROME, [
  "--headless=new",
  "--remote-debugging-port=9342",
  "--window-size=1000,1000",
  "--user-data-dir=" + process.env.TEMP + "/opendox-pages-profile",
  "about:blank",
]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 50 && !targets; i++) {
  await sleep(200);
  targets = await fetch("http://127.0.0.1:9342/json").then((r) => r.json()).catch(() => null);
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
  Enter: [13, "Enter"], u: [85, "KeyU"], k: [75, "KeyK"], ".": [190, "Period"], "%": [53, "Digit5"],
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

const KEYS2 = { Backspace: [8, "Backspace"], Home: [36, "Home"] };
Object.assign(KEYS, KEYS2);
const pageOf = (pos) => ev(`window.opendox.editor.caret_rect(${pos}, false)[0]`);

await send("Page.navigate", { url: "http://127.0.0.1:5174/demo/" });
for (let i = 0; i < 100 && !(await ev("document.body?.dataset.ready === 'true'")); i++) await sleep(100);

// 1. Ctrl+Enter in the middle of Section 1's first paragraph puts the rest on a new page.
// Where Section 2's heading is drawn: `[page, top]`.
const s2 = await posOf("Section 2");
const where = () => ev(`(() => { const r = window.opendox.editor.caret_rect(${s2}, false); return [r[0], r[2]]; })()`);
const before = JSON.stringify(await where());
await select(30, 30);
await key("Enter", CTRL);
const after = await caret();
check("Ctrl+Enter puts the caret after the break", after === 31, String(after));
check("the rest starts on the next page", (await pageOf(after)) === (await pageOf(29)) + 1);
check("…at the top margin", (await ev(`window.opendox.editor.caret_rect(${after}, false)[2]`)) < 72 + 20);
check("later text moves down", JSON.stringify(await where()) !== before, before);
check("toolbar state knows", (await fmt()).pageBreakBefore);

// 2. Backspace at the start of that paragraph removes the break, keeping both paragraphs.
const text = await ev("window.opendox.editor.text()");
let [x, y] = await clientOf(after);
await click(x, y);
await key("Backspace");
check("Backspace removes the break first", !(await fmt()).pageBreakBefore);
check("…without joining", (await ev("window.opendox.editor.text()")) === text);
await key("Backspace");
check("a second Backspace joins", (await ev("window.opendox.editor.text()")).length === text.length - 1);
check("…and later text is back where it was", JSON.stringify(await where()) === before);
await ev("window.opendox.view.undo()");
await ev("window.opendox.view.undo()");
await ev("window.opendox.view.undo()");

// 3. Page setup: A4 landscape with 0.5 in margins, through the panel.
await clickEl("#page-setup-trigger");
check("page setup opens", !(await hidden("page-setup")));
check("it shows the current paper", (await ev(`document.getElementById('paper').value`)) === "612x792");
check("…and margins in inches", (await ev(`document.getElementById('margin-left').value`)) === "1");
await ev(`(() => { document.getElementById('paper').value = '595.28x841.89';
  document.querySelector('[name=orientation][value=landscape]').checked = true;
  for (const s of ['top','bottom','left','right']) document.getElementById('margin-' + s).value = '0.5'; })()`);
await ev(`document.querySelector('#page-setup button[type=submit]').click()`);
check("panel closes", await hidden("page-setup"));
const setup = await ev("Array.from(window.opendox.editor.page_setup())");
check("page is A4 landscape", Math.abs(setup[0] - 841.89) < 0.01 && Math.abs(setup[1] - 595.28) < 0.01, JSON.stringify(setup));
check("margins are 36pt", setup.slice(2).every((m) => m === 36), JSON.stringify(setup));
check("pages drawn landscape", await ev(`(() => { const c = window.opendox.view.renderer.pages[0].querySelector('canvas'); return c.clientWidth > c.clientHeight; })()`));
check("text starts at the new margin", (await ev(`window.opendox.editor.caret_rect(0, false)[1]`)) === 36);

// 4. Margins that leave no room are refused with a message, and nothing changes.
await clickEl("#page-setup-trigger");
await ev(`document.getElementById('margin-left').value = '6'; document.getElementById('margin-right').value = '6';`);
await ev(`document.querySelector('#page-setup button[type=submit]').click()`);
check("impossible margins show an error", !(await hidden("page-setup-error")) && !(await hidden("page-setup")));
check("…and change nothing", (await ev("window.opendox.editor.page_setup()[5]")) === 36);
await clickEl("#page-setup-cancel");

// 5. Both exports use the page setup.
const exported = await ev(`(async () => {
  const e = window.opendox.editor;
  const pdf = new TextDecoder("latin1").decode(e.export_pdf(""));
  const bytes = e.export_docx("");
  const dv = new DataView(bytes.buffer, bytes.byteOffset); let pos = 0; let doc = "";
  while (dv.getUint32(pos, true) === 0x04034b50) {
    const method = dv.getUint16(pos + 8, true), size = dv.getUint32(pos + 18, true);
    const nameLen = dv.getUint16(pos + 26, true), extra = dv.getUint16(pos + 28, true);
    const name = new TextDecoder().decode(bytes.subarray(pos + 30, pos + 30 + nameLen));
    const start = pos + 30 + nameLen + extra; const data = bytes.subarray(start, start + size);
    if (name === "word/document.xml") {
      doc = method === 0 ? new TextDecoder().decode(data)
        : await new Response(new Blob([data]).stream().pipeThrough(new DecompressionStream("deflate-raw"))).text();
    }
    pos = start + size;
  }
  return { media: pdf.includes("/MediaBox [0 0 841.89 595.28]"), pgSz: doc.includes('<w:pgSz w:w="16838" w:h="11906"/>'), pgMar: doc.includes('w:left="720"') }; })()`);
check("PDF pages are A4 landscape", exported.media);
check(".docx page size and margins", exported.pgSz && exported.pgMar, JSON.stringify(exported));

// 6. Header & footer: turning the header on puts the caret in it; what's typed shows on every
// page, above the top margin.
await ev("window.opendox.view.setPageSetup({ width: 612, height: 792, top: 72, right: 72, bottom: 72, left: 72 })");
await clickEl("#margins-trigger");
check("header & footer panel opens", !(await hidden("margins-panel")));
check("page number buttons need the cursor in a margin", await ev(`document.getElementById('insert-page-number').disabled`));
await ev(`(() => { const c = document.getElementById('has-header'); c.checked = true; c.dispatchEvent(new Event('change')); })()`);
check("header added, caret in it", (await fmt()).region === "header" && (await fmt()).hasHeader);
await type("Quarterly report");
const pageCount = await ev("window.opendox.editor.layout()");
/** Glyphs drawn on page `p` between y0 and y1 (points from the top). */
const glyphsIn = (p, y0, y1) =>
  ev(`(() => { const g = window.opendox.editor.page_glyphs(${p}); const out = [];
    for (let i = 0; i < g.length; i += 7) if (g[i + 4] > ${y0} && g[i + 4] < ${y1}) out.push(g[i + 2]); return out; })()`);
for (let p = 0; p < pageCount; p++) {
  const head = await glyphsIn(p, 36, 72);
  check(`header drawn on page ${p + 1}`, head.length === "Quarterly report".length, String(head.length));
}

// 7. Add page numbers: a footer with a right-aligned number, a different one on each page.
await clickEl("#margins-trigger");
await clickEl("#add-page-numbers");
check("footer added", (await fmt()).hasFooter && (await fmt()).region === "footer");
const foot = async (p) => JSON.stringify(await glyphsIn(p, 792 - 72, 792 - 30));
check("page 1 and 2 show different numbers", (await foot(0)) !== (await foot(1)) && (await foot(0)) !== "[]", `${await foot(0)} ${await foot(1)}`);
const right = await ev(`(() => { const g = window.opendox.editor.page_glyphs(0); let x = 0;
  for (let i = 0; i < g.length; i += 7) if (g[i + 4] > 720) x = Math.max(x, g[i + 3]); return x; })()`);
check("the number sits at the right margin", right > 612 - 72 - 12 && right < 612 - 72, String(right));

// 8. A click in page 2's footer puts the caret there, on page 2.
const footerPos = await ev("window.opendox.editor.region_start(2)");
await ev(`window.opendox.view.renderer.pages[1].scrollIntoView({ block: "end" })`);
const [fx, fy] = await ev(`(() => { const v = window.opendox.view; const r = v.renderer.pages[1].getBoundingClientRect(); const k = v.renderer.cssPerPt;
  return [r.left + 300 * k, r.top + (792 - 44) * k]; })()`);
await click(fx, fy);
check("click lands in the footer", (await fmt()).region === "footer", String(await caret()));
check("caret drawn on page 2", (await ev(`window.opendox.editor.caret_rect(${await caret()}, false)[0]`)) === 1);

// 9. Select all in the footer selects only the footer; Ctrl+End stays in it.
await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "a", code: "KeyA", windowsVirtualKeyCode: 65, modifiers: CTRL });
await send("Input.dispatchKeyEvent", { type: "keyUp", key: "a", code: "KeyA", windowsVirtualKeyCode: 65, modifiers: CTRL });
const all = await ev("[window.opendox.view.selection.anchor, window.opendox.view.selection.focus]");
check("Ctrl+A in the footer selects the footer", all[0] === footerPos && all[1] > all[0], JSON.stringify(all));

// 10. The panel inserts a page count at the cursor in the footer.
await select(all[1], all[1]);
await type(" of ");
await clickEl("#margins-trigger");
check("page number buttons enabled in the footer", !(await ev(`document.getElementById('insert-page-number').disabled`)));
await clickEl("#insert-page-count");
check("page count drawn", (await foot(0)).length > 2);

// 11. Exports: the .docx has header and footer parts with Word fields.
const parts = await ev(`(async () => {
  const bytes = window.opendox.editor.export_docx("");
  const dv = new DataView(bytes.buffer, bytes.byteOffset); let pos = 0; const out = {};
  while (dv.getUint32(pos, true) === 0x04034b50) {
    const method = dv.getUint16(pos + 8, true), size = dv.getUint32(pos + 18, true);
    const nameLen = dv.getUint16(pos + 26, true), extra = dv.getUint16(pos + 28, true);
    const name = new TextDecoder().decode(bytes.subarray(pos + 30, pos + 30 + nameLen));
    const start = pos + 30 + nameLen + extra; const data = bytes.subarray(start, start + size);
    if (name.startsWith("word/header") || name.startsWith("word/footer")) {
      out[name] = method === 0 ? new TextDecoder().decode(data)
        : await new Response(new Blob([data]).stream().pipeThrough(new DecompressionStream("deflate-raw"))).text();
    }
    pos = start + size;
  }
  return out; })()`);
check(".docx header part", (parts["word/header1.xml"] ?? "").includes("Quarterly report"));
check(".docx footer fields", (parts["word/footer1.xml"] ?? "").includes(" PAGE ") && parts["word/footer1.xml"].includes(" NUMPAGES "));

// 12. Removing the header takes it off every page.
await clickEl("#margins-trigger");
await ev(`(() => { const c = document.getElementById('has-header'); c.checked = false; c.dispatchEvent(new Event('change')); })()`);
check("header removed", !(await fmt()).hasHeader);
check("…and no longer drawn", (await glyphsIn(0, 36, 70)).length === 0);

if (OUT) {
  await ev("window.scrollTo(0, 0)");
  const shot = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width: 1000, height: 700, scale: 1 } });
  writeFileSync(OUT, Buffer.from(shot.result.data, "base64"));
}

console.log(failures ? `${failures} FAILED` : "ALL PASSED");
ws.close();
chrome.kill();
process.exit(failures ? 1 : 0);
