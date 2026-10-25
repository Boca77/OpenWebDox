// Downloads the optional CJK fonts into fonts/noto/ (they're too big to keep in the repo:
// 4.5–8.3 MB each). Static Regular OTFs from the official noto-cjk project (SIL OFL 1.1, the
// same licence as the other Noto fonts, see fonts/noto/OFL.txt). The demo loads one only when
// a document has characters no other font can draw.
//
// Usage: node scripts/fetch-fonts.mjs   (or npm run fetch-fonts)
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const BASE = "https://github.com/notofonts/noto-cjk/raw/main/Sans/SubsetOTF";
const FONTS = [
  // Chinese (Simplified) Han, plus kana.
  ["SC", "NotoSansSC-Regular.otf"],
  // Japanese kanji forms, plus kana.
  ["JP", "NotoSansJP-Regular.otf"],
  // Korean Hangul, plus kana.
  ["KR", "NotoSansKR-Regular.otf"],
];

const dir = join(ROOT, "fonts", "noto");
mkdirSync(dir, { recursive: true });
let failed = 0;
for (const [region, file] of FONTS) {
  const target = join(dir, file);
  if (existsSync(target)) {
    console.log(`have ${file}`);
    continue;
  }
  const url = `${BASE}/${region}/${file}`;
  const res = await fetch(url).catch((e) => ({ ok: false, statusText: String(e) }));
  if (!res.ok) {
    console.error(`could not download ${url}: ${res.status ?? ""} ${res.statusText}`);
    failed++;
    continue;
  }
  const bytes = new Uint8Array(await res.arrayBuffer());
  // An OpenType CFF font starts with "OTTO".
  if (String.fromCharCode(...bytes.subarray(0, 4)) !== "OTTO") {
    console.error(`${url} isn't an OpenType font`);
    failed++;
    continue;
  }
  writeFileSync(target, bytes);
  console.log(`saved ${file} (${(bytes.length / 1e6).toFixed(1)} MB)`);
}
process.exit(failed ? 1 : 0);
