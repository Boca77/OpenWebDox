// End-to-end check for comments: commenting a selection and a word, the card beside the text,
// the highlight, replying, opening a card by clicking its text, resolving, deleting, and the
// .docx export.
//
// Needs `npm run serve` running. Usage: node scripts/e2e-comments.mjs [screenshot.png]
// Set CHROME to the browser binary if it isn't at the default Windows path.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2];
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const chrome = spawn(CHROME, [
  "--headless=new",
  "--remote-debugging-port=9354",
  "--window-size=1400,1000",
  "--user-data-dir=" + process.env.TEMP + "/opendox-comments-profile",
  "about:blank",
]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 50 && !targets; i++) {
  await sleep(200);
  targets = await fetch("http://127.0.0.1:9354/json").then((r) => r.json()).catch(() => null);
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

const comments = () => ev("window.opendox.view.comments()");
const cards = () => ev(`[...document.querySelectorAll('#comments .comment')].map((c) => ({ id: c.dataset.id ?? null, current: c.getAttribute('aria-current'), text: c.textContent, top: parseFloat(c.style.top) }))`);
/** Highlight colours (0xRRGGBB) drawn on page 0. */
const highlights = () => ev(`(() => { const e = window.opendox.editor; e.layout(); const d = e.page_decorations(0); const out = [];
  for (let i = 0; i < d.length; i += 6) if (d[i] === 0) out.push(d[i + 5]); return [...new Set(out)]; })()`);

await send("Page.navigate", { url: "http://127.0.0.1:5174/demo/" });
for (let i = 0; i < 100 && !(await ev("document.body?.dataset.ready === 'true'")); i++) await sleep(100);

// 1. Comment "Alice" (chars 10..15) from the toolbar button.
await select(10, 15);
await clickEl("#comment-trigger");
check("new-comment box opens, focused", await ev(`document.activeElement?.getAttribute('aria-label') === 'New comment'`));
await type("Who is she?");
await key("Enter");
let cs = await comments();
check("comment added", cs.length === 1 && cs[0].text === "Who is she?" && cs[0].start === 10 && cs[0].end === 15, JSON.stringify(cs));
check("…by You", cs[0].author === "You");
let cd = await cards();
check("its card is shown and open", cd.length === 1 && cd[0].current === "true", JSON.stringify(cd));
const lineY = await ev(`(() => { const v = window.opendox.view; const r = window.opendox.editor.caret_rect(10, false);
  return v.renderer.pages[0].getBoundingClientRect().top + scrollY + r[2] * v.renderer.cssPerPt; })()`);
check("the card sits level with its text", Math.abs(cd[0].top - lineY) < 2, `${cd[0].top} vs ${lineY}`);
check("its text is highlighted strongly while open", (await highlights()).includes(0xffd966), JSON.stringify(await highlights()));

// 2. Reply from the open card.
await ev(`document.querySelector('#comments .comment[aria-current=true] textarea').focus()`);
await type("The heroine.");
await key("Enter");
cs = await comments();
check("reply added", cs[0].replies.length === 1 && cs[0].replies[0].text === "The heroine.", JSON.stringify(cs[0].replies));

// 3. Moving the caret away closes the card; clicking the commented text opens it again.
let [x, y] = await clientOf(30);
await click(x, y);
check("caret away: no card open", (await cards()).every((c) => c.current === "false"));
check("…and the highlight is the light one", (await highlights()).includes(0xfff2cc) && !(await highlights()).includes(0xffd966));
[x, y] = await clientOf(12);
await click(x, y);
check("clicking commented text opens its card", (await cards())[0].current === "true");

// 4. Ctrl+Alt+M with just a caret comments the word there.
[x, y] = await clientOf(18);
await click(x, y);
await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "m", code: "KeyM", windowsVirtualKeyCode: 77, modifiers: CTRL | ALT });
await send("Input.dispatchKeyEvent", { type: "keyUp", key: "m", code: "KeyM", windowsVirtualKeyCode: 77, modifiers: CTRL | ALT });
await type("Which way?");
await key("Enter");
cs = await comments();
check("Ctrl+Alt+M comments the word at the caret", cs.length === 2 && cs[1].start === 16 && cs[1].end === 19, JSON.stringify(cs.map((c) => [c.start, c.end])));
cd = await cards();
check("cards don't overlap", cd.length === 2 && Math.abs(cd[1].top - cd[0].top) >= 20, JSON.stringify(cd.map((c) => c.top)));

// 5. The .docx carries both comments.
const docx = await ev(`(async () => {
  const bytes = window.opendox.editor.export_docx("");
  const dv = new DataView(bytes.buffer, bytes.byteOffset); let pos = 0; let xml = "";
  while (dv.getUint32(pos, true) === 0x04034b50) {
    const method = dv.getUint16(pos + 8, true), size = dv.getUint32(pos + 18, true);
    const nameLen = dv.getUint16(pos + 26, true), extra = dv.getUint16(pos + 28, true);
    const name = new TextDecoder().decode(bytes.subarray(pos + 30, pos + 30 + nameLen));
    const start = pos + 30 + nameLen + extra; const data = bytes.subarray(start, start + size);
    if (name === "word/comments.xml") {
      xml = method === 0 ? new TextDecoder().decode(data)
        : await new Response(new Blob([data]).stream().pipeThrough(new DecompressionStream("deflate-raw"))).text();
    }
    pos = start + size;
  }
  return xml; })()`);
check(".docx has both comments and the reply", docx.includes("Who is she?") && docx.includes("Which way?") && docx.includes("The heroine."));

if (OUT) {
  await ev("window.scrollTo(0, 0)");
  await sleep(200);
  const shot = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 150, width: 1400, height: 450, scale: 1 } });
  writeFileSync(OUT, Buffer.from(shot.result.data, "base64"));
}

// 6. Resolve the first from its card; delete the second.
await ev(`document.querySelector('#comments .comment[data-id="${cs[0].id}"] [aria-label=Resolve]').click()`);
cs = await comments();
check("resolve marks it resolved", cs.find((c) => c.text === "Who is she?")?.resolved === true);
check("…and its card goes", (await cards()).length === 1);
await ev(`document.querySelector('#comments .comment [aria-label=Delete]').click()`);
check("delete removes it", (await comments()).every((c) => c.text !== "Which way?"));
check("no open comment highlights left", (await highlights()).every((c) => c !== 0xfff2cc && c !== 0xffd966), JSON.stringify(await highlights()));

console.log(failures ? `${failures} FAILED` : "ALL PASSED");
ws.close();
chrome.kill();
process.exit(failures ? 1 : 0);
