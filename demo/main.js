import init, { Editor } from "../crates/opendox-wasm/pkg/opendox_wasm.js";
import { PageRenderer } from "../packages/editor/dist/index.js";

const FONTS = [
  "../fonts/noto/NotoSans-Regular.ttf",
  "../fonts/noto/NotoSans-Bold.ttf",
  "../fonts/noto/NotoSansArabic-Regular.ttf",
];

// Opening of "Alice's Adventures in Wonderland" (Lewis Carroll, 1865; public domain).
const ALICE = [
  "Alice was beginning to get very tired of sitting by her sister on the bank, and of having nothing to do: once or twice she had peeped into the book her sister was reading, but it had no pictures or conversations in it, “and what is the use of a book,” thought Alice “without pictures or conversations?”",
  "So she was considering in her own mind (as well as she could, for the hot day made her feel very sleepy and stupid), whether the pleasure of making a daisy-chain would be worth the trouble of getting up and picking the daisies, when suddenly a White Rabbit with pink eyes ran close by her.",
  "There was nothing so very remarkable in that; nor did Alice think it so very much out of the way to hear the Rabbit say to itself, “Oh dear! Oh dear! I shall be late!” (when she thought it over afterwards, it occurred to her that she ought to have wondered at this, but at the time it all seemed quite natural); but when the Rabbit actually took a watch out of its waistcoat-pocket, and looked at it, and then hurried on, Alice started to her feet, for it flashed across her mind that she had never before seen a rabbit with either a waistcoat-pocket, or a watch to take out of it, and burning with curiosity, she ran across the field after it, and fortunately was just in time to see it pop down a large rabbit-hole under the hedge.",
  "In another moment down went Alice after it, never once considering how in the world she was to get out again.",
  "The rabbit-hole went straight on like a tunnel for some way, and then dipped suddenly down, so suddenly that Alice had not a moment to think about stopping herself before she found herself falling down a very deep well.",
  "Mixed scripts on one line: the word for hello is مرحبا, a family emoji 👨‍👩‍👧 stays whole, and café with a combining accent never splits.",
];

const $ = (id) => document.getElementById(id);

async function main() {
  await init();
  const editor = new Editor();
  for (const url of FONTS) {
    const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
    editor.register_font(bytes);
  }
  editor.set_default_font("Noto Sans, Noto Sans Arabic", 11);

  const sections = [];
  for (let i = 1; i <= 4; i++) sections.push(`Section ${i}`, ...ALICE);
  editor.insert(0, sections.join("\n"));

  const t0 = performance.now();
  const pageCount = editor.layout();
  const layoutMs = performance.now() - t0;

  const renderer = new PageRenderer(editor, $("pages"));
  const draw = () => {
    const stats = renderer.render(pageCount);
    $("s-pages").textContent = String(stats.pages);
    $("s-glyphs").textContent = stats.glyphs.toLocaleString();
    $("s-layout").textContent = `${layoutMs.toFixed(1)} ms`;
    $("s-render").textContent = `${stats.ms.toFixed(1)} ms`;
  };
  draw();

  $("zoom").addEventListener("change", (e) => {
    renderer.zoom = Number(e.target.value);
    draw();
  });

  document.body.dataset.ready = "true";
}

main().catch((err) => {
  const p = document.createElement("p");
  p.id = "error";
  p.textContent = `Failed to start: ${err}`;
  document.body.append(p);
  console.error(err);
});
