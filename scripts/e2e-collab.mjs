// End-to-end collaboration check: two browser tabs on the same document through the real sync
// server. Live typing both ways, remote carets, concurrent edits, offline edits merged on
// reconnect, a reload restoring the document from the server.
//
// Needs `npm run serve` and `npm run sync-server` running.
// Usage: node scripts/e2e-collab.mjs [screenshot.png]
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const OUT = process.argv[2];
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const PORT = 9335;
const DOC = `e2e-${Date.now().toString(36)}`;
const chrome = spawn(CHROME, [
  "--headless=new",
  `--remote-debugging-port=${PORT}`,
  "--window-size=1000,900",
  "--user-data-dir=" + process.env.TEMP + "/opendox-collab-profile",
  // Both tabs must keep running timers (edits are flushed on a timer) while not in front.
  "--disable-background-timer-throttling",
  "--disable-renderer-backgrounding",
  "--disable-backgrounding-occluded-windows",
  "about:blank",
]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let ready = false;
for (let i = 0; i < 50 && !ready; i++) {
  await sleep(200);
  ready = await fetch(`http://127.0.0.1:${PORT}/json/version`).then(() => true, () => false);
}

/** One tab, driven over its own DevTools connection. */
async function openTab(name) {
  const url = `http://127.0.0.1:5174/demo/?doc=${DOC}&name=${name}`;
  const t = await (await fetch(`http://127.0.0.1:${PORT}/json/new?${encodeURIComponent(url)}`, { method: "PUT" })).json();
  const ws = new WebSocket(t.webSocketDebuggerUrl);
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
  await send("Emulation.setFocusEmulationEnabled", { enabled: true });
  const tab = {
    name,
    send,
    ev,
    text: () => ev("window.opendox.editor.text()"),
    status: () => ev("document.getElementById('s-sync').textContent"),
    sel: () => ev("(() => { const s = window.opendox.view.selection; return [s.anchor, s.focus]; })()"),
    async type(text) {
      await send("Input.insertText", { text });
    },
    /** Puts the caret at `pos` with a real click. */
    async clickAt(pos) {
      const [x, y] = await ev(`(() => { const {editor, view} = window.opendox; const r = editor.caret_rect(${pos}, false);
        const page = view.renderer.pages[r[0]].getBoundingClientRect(); const k = view.renderer.cssPerPt;
        return [page.left + r[1] * k, page.top + (r[2] + r[3]) / 2 * k]; })()`);
      await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
      await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 });
    },
    async waitReady() {
      for (let i = 0; i < 100; i++) {
        if (await ev("document.body?.dataset.ready === 'true'").catch(() => false)) {
          if ((await this.status()) === "Connected") return true;
        }
        await sleep(100);
      }
      return false;
    },
    close: () => ws.close(),
  };
  return tab;
}

/** Polls until `fn` returns true, up to `ms`. */
async function until(fn, ms = 5000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await fn()) return true;
    await sleep(50);
  }
  return false;
}

let failures = 0;
const check = (name, ok, detail = "") => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : "  -> " + detail}`);
  if (!ok) failures++;
};

const a = await openTab("Ada");
const b = await openTab("Grace");
check("both tabs connect", (await a.waitReady()) && (await b.waitReady()), `${await a.status()} / ${await b.status()}`);

// 1. Live typing A → B.
await a.clickAt(0);
await a.type("Hello from Ada.");
check("A's typing appears in B", await until(async () => (await b.text()) === "Hello from Ada."), JSON.stringify(await b.text()));

// 2. Live typing B → A, at the end.
await b.clickAt(15);
await b.type(" Grace here.");
const both = "Hello from Ada. Grace here.";
check("B's typing appears in A", await until(async () => (await a.text()) === both), JSON.stringify(await a.text()));

// 3. Remote carets and the people list.
check(
  "B shows A's caret with A's name",
  await until(async () => (await b.ev("[...document.querySelectorAll('.opendox-remote-name')].map(e => e.textContent).join()")) === "Ada"),
  await b.ev("document.querySelectorAll('.opendox-remote-caret').length"),
);
check("A shows B's caret", await until(async () => (await a.ev("document.querySelectorAll('.opendox-remote-caret').length")) === 1));
check("people list shows both", (await b.ev("document.querySelectorAll('#people li').length")) === 2);

// 4. A remote insert before B's caret moves B's caret with its text.
const [, bFocus] = await b.sel();
await a.clickAt(0);
await a.type(">> ");
await until(async () => (await b.text()).startsWith(">> "));
check("B's caret follows its text after A's insert", (await b.sel())[1] === bFocus + 3, `${bFocus} -> ${(await b.sel())[1]}`);

// 5. Concurrent edits converge.
await a.clickAt(3);
await b.clickAt(await b.ev("window.opendox.editor.len()"));
await Promise.all([a.type("[a1]"), b.type("[b1]")]);
await Promise.all([a.type("[a2]"), b.type("[b2]")]);
check(
  "concurrent edits converge",
  await until(async () => (await a.text()) === (await b.text()) && (await a.text()).includes("[a2]") && (await a.text()).includes("[b2]")),
  `${JSON.stringify(await a.text())} vs ${JSON.stringify(await b.text())}`,
);

// 6. A goes offline, both keep typing, A reconnects.
await a.ev("window.opendox.collab.disconnect()");
check("A shows offline", (await a.status()).startsWith("Offline"));
check("B drops A's caret", await until(async () => (await b.ev("document.querySelectorAll('.opendox-remote-caret').length")) === 0));
await a.clickAt(0);
await a.type("(offline A) ");
await b.type(" (B meanwhile)");
await sleep(300);
check("offline edit stays local", !(await b.text()).includes("(offline A)"));
await a.ev("window.opendox.collab.connect()");
check(
  "offline edits merge on reconnect",
  await until(async () => {
    const [ta, tb] = [await a.text(), await b.text()];
    return ta === tb && ta.includes("(offline A)") && ta.includes("(B meanwhile)");
  }),
  `${JSON.stringify(await a.text())} vs ${JSON.stringify(await b.text())}`,
);

// 7. Tables sync: A inserts one, B types into a cell.
await a.clickAt(await a.ev("window.opendox.editor.len()"));
await a.ev("window.opendox.view.insertTable(2, 2)");
check("table reaches B", await until(async () => (await b.ev("(() => { const e = window.opendox.editor; for (let p = 0; p <= e.len(); p++) if (e.table_info(p)) return true; return false; })()"))));
const cell = await b.ev("(() => { const e = window.opendox.editor; for (let p = 0; p <= e.len(); p++) { const t = e.table_info(p); if (t && t[0] === 1 && t[1] === 1) return p; } return -1; })()");
await b.clickAt(cell);
await b.type("cell");
check("cell text reaches A", await until(async () => (await a.text()) === (await b.text()) && (await a.text()).includes("cell")));

// 8. Reloading B restores the document from the server.
const expected = await a.text();
await b.send("Page.reload");
await sleep(300);
check("reloaded tab reconnects", await b.waitReady());
check("reloaded tab has the whole document", await until(async () => (await b.text()) === expected), JSON.stringify(await b.text()));

if (OUT) {
  await a.clickAt(5);
  await sleep(300);
  const shot = await b.send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width: 1000, height: 420, scale: 1 } });
  writeFileSync(OUT, Buffer.from(shot.result.data, "base64"));
}

console.log(failures ? `${failures} FAILED` : "ALL PASSED");
a.close();
b.close();
chrome.kill();
process.exit(failures ? 1 : 0);
