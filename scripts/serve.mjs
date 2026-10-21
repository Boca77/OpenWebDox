// Minimal static file server for the demo (wasm needs a real HTTP origin and the right MIME
// type). Serves the repo root on http://127.0.0.1:5174, compressed like a real host would
// (brotli, else gzip) so load sizes in devtools are realistic. No dependencies.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { brotliCompress, constants, gzip } from "node:zlib";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const port = Number(process.env.PORT ?? 5174);
const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".ttf": "font/ttf",
  ".otf": "font/otf",
  ".map": "application/json",
};

const brotli = promisify(brotliCompress);
const gz = promisify(gzip);
// Quality 5, not 11: 11 takes ~9 s per request on the wasm. It is ~18% larger than 11, which a
// production host gets by compressing once at build time.
const brotliOptions = { params: { [constants.BROTLI_PARAM_QUALITY]: 5 } };

createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", "http://localhost");
  let path = normalize(join(root, decodeURIComponent(url.pathname)));
  if (path !== root && !path.startsWith(root + sep)) {
    res.writeHead(403).end();
    return;
  }
  if (url.pathname.endsWith("/")) path = join(path, "index.html");
  try {
    let body = await readFile(path);
    const headers = {
      "content-type": types[extname(path)] ?? "application/octet-stream",
      "cache-control": "no-store",
      vary: "accept-encoding",
    };
    const accepts = String(req.headers["accept-encoding"] ?? "");
    if (/\bbr\b/.test(accepts)) {
      body = await brotli(body, brotliOptions);
      headers["content-encoding"] = "br";
    } else if (/\bgzip\b/.test(accepts)) {
      body = await gz(body);
      headers["content-encoding"] = "gzip";
    }
    res.writeHead(200, headers);
    res.end(body);
  } catch {
    res.writeHead(404).end("not found");
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`opendox demo: http://127.0.0.1:${port}/demo/`);
});
