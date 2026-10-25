// End-to-end check for character formatting: underline, strikethrough, colours, superscript
// and links, through shortcuts and the toolbar, plus the link bubble, how ticked checklist
// items look, and both exports.
//
// Needs `npm run serve` running. Usage: node scripts/e2e-formats.mjs [screenshot.png]
// Set CHROME to the browser binary if it isn't at the default Windows path.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2];
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const chrome = spawn(CHROME, [
  "--headless=new",
  "--remote-debugging-port=9340",
  "--window-size=1000,1000",
  "--user-data-dir=" + process.env.TEMP + "/opendox-formats-profile",
  "about:blank",
]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 50 && !targets; i++) {
  await sleep(200);
  targets = await fetch("http://127.0.0.1:9340/json").then((r) => r.json()).catch(() => null);
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

await send("Page.navigate", { url: "http://127.0.0.1:5174/demo/" });
for (let i = 0; i < 100 && !(await ev("document.body?.dataset.ready === 'true'")); i++) await sleep(100);

// "Alice" in Section 1 is chars 10..15.
await select(10, 15);

// 1. Ctrl+U underlines; Alt+Shift+5 strikes through; again removes them.
await key("u", CTRL);
check("Ctrl+U underlines", (await fmt()).underline);
await key("%", ALT | SHIFT);
check("Alt+Shift+5 strikes through", (await fmt()).strikethrough);
let d = await drawn(0);
check("underline and strikethrough drawn", d.kinds.includes(1) && d.kinds.includes(2), JSON.stringify(d.kinds));
await key("u", CTRL);
await key("%", ALT | SHIFT);
check("pressing them again removes both", !(await fmt()).underline && !(await fmt()).strikethrough);

// 2. Text colour from the toolbar panel.
await clickEl("#color-trigger");
check("colour panel opens", !(await hidden("color-panel")));
await clickEl('#swatches [data-color="#c5221f"]');
check("swatch sets the text colour", (await fmt()).color === "#c5221f", JSON.stringify((await fmt()).color));
check("panel closes", await hidden("color-panel"));
check("red glyphs drawn", (await drawn(0)).colors.includes(0xc5221f));
check(
  "colour bar shows it",
  (await ev(`getComputedStyle(document.getElementById('color-bar')).backgroundColor`)) === "rgb(197, 34, 31)",
);

// 3. Highlight, then None removes it.
await clickEl("#highlight-trigger");
check("None reads None for highlight", (await ev(`document.getElementById('color-none').textContent`)) === "None");
await clickEl('#swatches [data-color="#fff475"]');
check("highlight set", (await fmt()).highlight === "#fff475");
check("highlight drawn first", (await drawn(0)).kinds[0] === 0);
await clickEl("#highlight-trigger");
await clickEl("#color-none");
check("None removes the highlight", (await fmt()).highlight === null);

// 4. Ctrl+. superscript, and again back to normal.
await select(15, 16);
await key(".", CTRL);
check("Ctrl+. makes superscript", (await fmt()).script === "super");
await key(".", CTRL);
check("again makes it normal", (await fmt()).script === "normal");

// 5. Ctrl+K: link the selection ("was": "Alice" is red now, and its own colour would win
// over link blue) through the link field.
await select(16, 19);
await key("k", CTRL);
check("Ctrl+K opens the link field", !(await hidden("link-panel")));
check("link field focused", await ev(`document.activeElement?.id === 'link-url'`));
await type("example.org/was");
await key("Enter");
check("Enter applies the link", (await ev("window.opendox.editor.link_at(17)")) === "example.org/was");
check("link text drawn blue", (await drawn(0)).colors.includes(0x1155cc));

// 6. The bubble shows with the caret in the link; typing after the link isn't linked.
let [x, y] = await clientOf(17);
await click(x, y);
check("bubble shown in a link", !(await hidden("link-bubble")));
check("bubble names the URL", (await ev(`document.getElementById('bubble-url').textContent`)) === "example.org/was");
check("bubble opens it over https", (await ev(`document.getElementById('bubble-url').href`)) === "https://example.org/was");
[x, y] = await clientOf(19, 0.5);
await click(x, y);
await type("Z");
check(
  "typing after a link isn't linked",
  (await ev(`window.opendox.editor.slice(19, 20)`)) === "Z" && (await ev(`window.opendox.editor.link_at(20)`)) === undefined,
);
await ev("window.opendox.view.undo()");

// 7. Remove from the bubble.
[x, y] = await clientOf(17);
await click(x, y);
await clickEl("#bubble-remove");
check("bubble Remove unlinks it", (await ev("window.opendox.editor.link_at(17)")) === undefined);
check("bubble hidden after removing", await hidden("link-bubble"));

// 8. Ctrl+K with nothing selected inserts the URL as a link.
const end = await ev("window.opendox.editor.len()");
[x, y] = await clientOf(end);
await click(x + 40, y);
await key("Enter");
await key("k", CTRL);
await type("https://opendox.dev");
await key("Enter");
const at = await caret();
check("inserted URL is linked", (await ev(`window.opendox.editor.link_at(${at - 2})`)) === "https://opendox.dev");

// 9. A ticked checklist item is struck through and grey.
const drink = await posOf("Drink the bottle marked “DRINK ME”");
const page = (await ev(`window.opendox.editor.caret_rect(${drink}, false)`))[0];
d = await drawn(page);
check("ticked item drawn grey", d.colors.includes(0x80868b), JSON.stringify(d.colors));
check("ticked item struck through", d.kinds.includes(2));

// 10. The .docx carries the hyperlink; the PDF gets a link annotation.
const exported = await ev(`(async () => {
  const e = window.opendox.editor;
  const pdf = new TextDecoder("latin1").decode(e.export_pdf(""));
  const bytes = e.export_docx("");
  const dv = new DataView(bytes.buffer, bytes.byteOffset); let pos = 0; let rels = "";
  while (dv.getUint32(pos, true) === 0x04034b50) {
    const method = dv.getUint16(pos + 8, true), size = dv.getUint32(pos + 18, true);
    const nameLen = dv.getUint16(pos + 26, true), extra = dv.getUint16(pos + 28, true);
    const name = new TextDecoder().decode(bytes.subarray(pos + 30, pos + 30 + nameLen));
    const start = pos + 30 + nameLen + extra; const data = bytes.subarray(start, start + size);
    if (name === "word/_rels/document.xml.rels") {
      rels = method === 0 ? new TextDecoder().decode(data)
        : await new Response(new Blob([data]).stream().pipeThrough(new DecompressionStream("deflate-raw"))).text();
    }
    pos = start + size;
  }
  return { annot: pdf.includes("/Annots"), rels }; })()`);
check("PDF has a link annotation", exported.annot);
check(".docx has the hyperlink", exported.rels.includes('Target="https://opendox.dev" TargetMode="External"'));

// 11. Paragraph styles: the sample's sections are Heading 1 and show in the outline; the style
// menu and Ctrl+Alt+2 restyle a paragraph; clicking an outline entry moves the caret there.
const outline = () => ev(`[...document.querySelectorAll('#outline-list button')].map((b) => b.textContent)`);
check("outline lists the sample's headings",
  JSON.stringify((await outline()).slice(0, 4)) === JSON.stringify(["Section 1", "Section 2", "Section 3", "Section 4"]),
  JSON.stringify(await outline()));
check("style menu shows Heading 1 in a section title", (await (async () => { await select(3, 3); return (await fmt()).style; })()) === "h1");
const para2 = await posOf(await ev(`window.opendox.editor.slice(...window.opendox.editor.paragraph_range(20))`));
await select(para2 + 2, para2 + 2);
await ev(`(() => { const s = document.getElementById('para-style'); s.value = 'title'; s.dispatchEvent(new Event('change')); })()`);
check("style menu makes a title", (await fmt()).style === "title");
check("the title is drawn at 26pt", (await ev(`(() => { const e = window.opendox.editor; const r = e.caret_rect(${para2 + 2}, false); return r[3] - r[2]; })()`)) > 30);
await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "2", code: "Digit2", windowsVirtualKeyCode: 50, modifiers: CTRL | ALT });
await send("Input.dispatchKeyEvent", { type: "keyUp", key: "2", code: "Digit2", windowsVirtualKeyCode: 50, modifiers: CTRL | ALT });
check("Ctrl+Alt+2 makes heading 2", (await fmt()).style === "h2");
check("outline gains it", (await outline()).length === 6, JSON.stringify(await outline()));
await ev(`[...document.querySelectorAll('#outline-list button')].find((b) => b.textContent === 'Section 3').click()`);
const s3 = await posOf("Section 3");
check("outline entry moves the caret to its heading", (await caret()) === s3, `${await caret()} vs ${s3}`);
await ev("window.opendox.view.undo()");
await ev("window.opendox.view.undo()");

if (OUT) {
  // Section 1's first line, styled: red link, then the rest.
  await select(10, 10);
  await ev("window.scrollTo(0, 0)");
  const shot = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width: 1000, height: 420, scale: 1 } });
  writeFileSync(OUT, Buffer.from(shot.result.data, "base64"));
}

console.log(failures ? `${failures} FAILED` : "ALL PASSED");
ws.close();
chrome.kill();
process.exit(failures ? 1 : 0);
