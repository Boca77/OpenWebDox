// Image checks: drives the demo through the Chrome DevTools Protocol with real pointer and key
// events: insert, select, corner resize, side squish, rotate, the wrap menu, dragging, the
// options panel, delete and undo.
//
// Needs `npm run serve` running. Usage: node scripts/e2e-images.mjs [screenshot.png]
// Set CHROME to the browser binary if it isn't at the default Windows path.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2];
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const chrome = spawn(CHROME, [
  "--headless=new",
  "--remote-debugging-port=9336",
  "--window-size=1200,1000",
  "--user-data-dir=" + process.env.TEMP + "/opendox-images-profile",
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
const near = (a, b, tol) => Math.abs(a - b) <= tol;

async function mouse(type, x, y, buttons = 1, modifiers = 0) {
  await send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons, clickCount: 1, modifiers });
}
async function drag(from, to, modifiers = 0) {
  await mouse("mouseMoved", from[0], from[1], 0);
  await mouse("mousePressed", from[0], from[1], 1, modifiers);
  const steps = 6;
  for (let i = 1; i <= steps; i++) {
    const t = i / steps;
    await mouse("mouseMoved", from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t, 1, modifiers);
  }
  await mouse("mouseReleased", to[0], to[1], 0, modifiers);
}
async function click(x, y) {
  await mouse("mousePressed", x, y);
  await mouse("mouseReleased", x, y, 0);
}
async function key(k, code) {
  await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: k, code: k, windowsVirtualKeyCode: code });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code: k, windowsVirtualKeyCode: code });
}
const props = () => ev("window.opendox.view.imageProps()");
/** Client centre of an element. */
const centreOf = (sel) =>
  ev(`(() => { const el = document.querySelector(${JSON.stringify(sel)}); el.scrollIntoView({ block: "nearest" });
    const r = el.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()`);
const handle = (h) => centreOf(`.opendox-handle[data-handle="${h}"]`);

await send("Page.navigate", { url: "http://127.0.0.1:5174/demo/" });
for (let i = 0; i < 100 && !(await ev("document.body?.dataset.ready === 'true'")); i++) await sleep(100);

// 1. Insert a generated 400 x 300 px JPEG after "Section 1".
await ev(`(async () => {
  const c = document.createElement("canvas"); c.width = 400; c.height = 300;
  const g = c.getContext("2d");
  const grad = g.createLinearGradient(0, 0, 400, 300); grad.addColorStop(0, "#2f5bd3"); grad.addColorStop(1, "#e2a33b");
  g.fillStyle = grad; g.fillRect(0, 0, 400, 300); g.fillStyle = "#fff"; g.beginPath(); g.arc(200, 150, 80, 0, 7); g.fill();
  const blob = await new Promise((r) => c.toBlob(r, "image/png"));
  const { view } = window.opendox;
  view.selectImage(null);
  // Caret at the start of the first Alice paragraph.
  window.opendox.view.applyRemote(10, 10, false);
  await view.insertImageFile(blob);
})()`);
let p = await props();
check("image inserted and selected", p && (await ev("window.opendox.view.selectedImage")) !== null, JSON.stringify(p));
check("stored as JPEG (opaque) at 96 dpi", p && near(p.width, 300, 0.01) && near(p.height, 225, 0.01) && (await ev(`window.opendox.editor.image_mime(window.opendox.view.selectedImage)`)) === "image/jpeg", JSON.stringify(p));
check("frame with 8 handles + rotate", (await ev("document.querySelectorAll('.opendox-image-frame .opendox-handle').length")) === 9);
check("image toolbar shown", await ev("!document.getElementById('image-bar').hidden"));
await sleep(300);
check("picture drawn on the page", await ev(`(() => { const { view, editor } = window.opendox;
  const d = editor.page_objects(0); const k = view.renderer.cssPerPt * devicePixelRatio;
  const ctx = view.renderer.pages[0].querySelector('canvas').getContext('2d');
  const px = ctx.getImageData(Math.round((d[2] + 5) * k), Math.round((d[3] + 5) * k), 1, 1).data;
  return px[2] > 150 && px[0] < 120; })()`));

// 2. Corner handle keeps proportions.
let [hx, hy] = await handle("1,1");
await drag([hx, hy], [hx + 80, hy + 20]);
p = await props();
check("corner resize keeps aspect ratio", near(p.width / p.height, 4 / 3, 0.01) && p.width > 330, JSON.stringify(p));

// 3. Side handle squishes one way.
const before = p;
[hx, hy] = await handle("1,0");
await drag([hx, hy], [hx - 120, hy]);
p = await props();
check("side handle squishes width only", p.width < before.width - 60 && near(p.height, before.height, 0.01), JSON.stringify(p));

// 4. Rotate handle: drag to the right of centre = 90°.
const [cx, cy] = await centreOf(".opendox-image-frame");
const [rx, ry] = await handle("rotate");
await drag([rx, ry], [cx + 200, cy + 2]);
p = await props();
check("rotate handle snaps to 90°", near(p.rotation, 90, 0.01), JSON.stringify(p));
await ev("window.opendox.view.setImageRotation(0)");

// 5. Wrap menu: Wrap text.
await click(...(await centreOf("#wrap-trigger")));
check("wrap menu opens", await ev("!document.getElementById('wrap-menu').hidden"));
check("In line is checked", await ev(`document.querySelector('#wrap-menu [data-mode="inline"]').getAttribute('aria-checked') === 'true'`));
await click(...(await centreOf('#wrap-menu [data-mode="wrap"]')));
p = await props();
check("Wrap text chosen", p.wrap === "wrap", JSON.stringify(p));
check("text wraps beside the image", await ev(`(() => { const { editor } = window.opendox;
  const o = editor.page_objects(0); const right = o[2] + o[4];
  const g = editor.page_glyphs(0); let beside = 0;
  for (let i = 0; i < g.length; i += 7) if (g[i + 4] > o[3] + 12 && g[i + 4] < o[3] + o[5] && g[i + 3] > right) beside++;
  return beside > 20; })()`));

// 6. Drag the image down and right.
const start = await ev("Array.from(window.opendox.editor.page_objects(0).slice(2, 4))");
const [mx, my] = await centreOf(".opendox-image-frame");
await drag([mx, my], [mx + 60, my + 150]);
const after = await ev("Array.from(window.opendox.editor.page_objects(0).slice(2, 4))");
const k = await ev("window.opendox.view.renderer.cssPerPt");
check("dragging moves the image", near(after[0] - start[0], 60 / k, 1.5) && near(after[1] - start[1], 150 / k, 1.5), JSON.stringify([start, after]));

// 7. Options panel: grayscale and an adjustment, alt text.
await click(...(await centreOf("#image-options")));
check("options panel opens", await ev("!document.getElementById('image-panel').hidden"));
await ev(`(() => { for (const d of document.querySelectorAll('#image-panel details')) d.open = true; })()`);
await click(...(await centreOf('[data-recolor="grayscale"]')));
await ev(`(() => { const s = document.getElementById('img-brightness'); s.value = '30'; s.dispatchEvent(new Event('input')); s.dispatchEvent(new Event('change')); })()`);
await ev(`(() => { const t = document.getElementById('img-alt'); t.value = 'A blue and gold gradient with a white circle'; t.dispatchEvent(new Event('change')); })()`);
p = await props();
check("panel sets recolor, brightness, alt", p.recolor === "grayscale" && p.brightness === 30 && (await ev("window.opendox.view.imageAlt()")).startsWith("A blue"), JSON.stringify(p));
const ratio = p.width / p.height;
await ev(`(() => { const w = document.getElementById('img-width'); w.value = '2'; w.dispatchEvent(new Event('change')); })()`);
p = await props();
check("panel width keeps the locked ratio", near(p.width, 144, 0.01) && near(p.width / p.height, ratio, 0.01), JSON.stringify(p));

if (OUT) {
  await sleep(300);
  const shot = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(OUT, Buffer.from(shot.result.data, "base64"));
}

// 8. Delete key, then Ctrl+Z.
await ev("window.opendox.view.focusEditor()");
await key("Delete", 46);
check("Delete removes the image", (await ev("window.opendox.editor.page_objects(0).length")) === 0);
await send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "z", code: "KeyZ", windowsVirtualKeyCode: 90, modifiers: 2 });
await send("Input.dispatchKeyEvent", { type: "keyUp", key: "z", code: "KeyZ", windowsVirtualKeyCode: 90, modifiers: 2 });
check("Ctrl+Z brings it back", (await ev("window.opendox.editor.page_objects(0).length")) === 9);

// 9. PDF and .docx exports include it.
const exported = await ev(`(() => { const e = window.opendox.editor; const pdf = new TextDecoder('latin1').decode(e.export_pdf('t'));
  const docx = e.export_docx('t'); const name = new TextDecoder('latin1').decode(docx);
  return [pdf.includes('/Subtype /Image'), name.includes('word/media/image1.jpeg')]; })()`);
check("PDF embeds the image", exported[0]);
if (OUT) {
  // The files themselves, for checking in a PDF viewer and in Word.
  for (const [ext, call] of [["pdf", "export_pdf"], ["docx", "export_docx"]]) {
    const bytes = await ev(`Array.from(window.opendox.editor.${call}('t'))`);
    writeFileSync(OUT.replace(/\.png$/, `.${ext}`), Buffer.from(bytes));
  }
}
check(".docx carries the image", exported[1]);

chrome.kill();
console.log(failures ? `${failures} failed` : "all passed");
process.exit(failures ? 1 : 0);
