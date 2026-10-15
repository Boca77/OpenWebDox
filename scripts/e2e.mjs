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
    const page = view.renderer.pages[r[0]].getBoundingClientRect(); const k = view.renderer.cssPerPt;
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
