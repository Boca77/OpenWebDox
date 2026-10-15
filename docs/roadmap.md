# Roadmap

Work one milestone at a time. A milestone is done only when every done-criterion has a passing test
or a recorded manual check.

## M0 — Toolchain and skeleton ✅
- Rust stable + MSVC build tools + `wasm32-unknown-unknown` target + wasm-pack installed.
- Cargo workspace with `opendox-model` and `opendox-layout` crates that build and test.
- Done: `cargo test --workspace`, `cargo clippy -- -D warnings` and `cargo fmt --check` all pass.

## M1 — Plain-text model on Loro ✅
- `Document` wrapper: insert/delete text at a position, read text, split into paragraphs on `\n`.
- Stable positions (Loro cursors) for carets that survive remote edits.
- Grapheme-safe position API.
- Done: unit tests for edits; property test — 3 replicas, random concurrent edits, exchanged in
  random order, always converge to identical text; cursor test — a remote insert before a caret
  moves the caret correctly.

## M2 — Paragraph layout (spike + build) ✅
- Spike: shape and line-break one paragraph with parley and with cosmic-text; record choice in
  decisions.md.
- Bundle one free font family (e.g. Noto Sans / Inter) for tests.
- `layout_paragraph(text, style_runs, width_pt) -> ParagraphLayout` with lines, glyph runs,
  baselines, line heights from max ascent/descent.
- Done: golden tests for wrapping (normal text, overlong word, mixed font sizes, CJK, emoji), stable
  across machines.

## M3 — Pagination
- Page geometry (size, margins in pt). Stack paragraph layouts into pages; split paragraphs across
  pages at line boundaries. Per-paragraph cache; edit re-lays out one paragraph and restacks.
- Done: golden page-break tests; test that editing paragraph N re-shapes only paragraph N.

## M4 — Browser rendering
- `opendox-wasm` bindings; `packages/editor` draws pages on canvas from Rust glyph positions.
- Spike glyph drawing (Path2D outlines vs atlas); record choice.
- Done: demo page renders a multi-page doc; screenshot matches across Chrome and Firefox.

## M5 — Hit testing, caret, input
- Point → text position (with line-wrap affinity), caret drawing, selection, arrow keys with goal-x.
- Hidden textarea input incl. IME composition.
- Done: tests for hit testing edge cases (margins, between lines, between pages); manual IME check.

## M6 — Sync server
- WebSocket server sharing `opendox-model`; storage trait with SQLite adapter; update batching;
  snapshot compaction with watermark; remote carets/presence.
- Done: two browsers edit the same doc live; offline edits merge on reconnect; compaction test.

## M7 — Rich text and undo
- Bold/italic/size/font marks, paragraph attributes (alignment, spacing), local-only undo/redo.

## Later
Lists and numbering, tables, images, headers/footers, footnotes, bidi, comments/suggestions,
clipboard HTML, PDF and .docx export, accessibility mirror DOM.
