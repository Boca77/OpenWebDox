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

## M3 — Pagination ✅
- Page geometry (size, margins in pt). Stack paragraph layouts into pages; split paragraphs across
  pages at line boundaries. Per-paragraph cache; edit re-lays out one paragraph and restacks.
- Done: golden page-break tests; test that editing paragraph N re-shapes only paragraph N.

## M4 — Browser rendering ✅
Firefox not checked yet (not installed on the dev machine); Chrome and Edge match.
- `opendox-wasm` bindings; `packages/editor` draws pages on canvas from Rust glyph positions.
- Spike glyph drawing (Path2D outlines vs atlas); record choice.
- Done: demo page renders a multi-page doc; screenshot matches across Chrome and Firefox.

## M5 — Hit testing, caret, input ✅
IME tested with simulated composition events (CDP `Input.imeSetComposition`), not yet with a real
OS input method. `npm run e2e` covers clicks, drags, keys, typing, IME and emoji deletion.
- Point → text position (with line-wrap affinity), caret drawing, selection, arrow keys with goal-x.
- Hidden textarea input incl. IME composition.
- Done: tests for hit testing edge cases (margins, between lines, between pages); manual IME check.

## M5.5 — Block structure + tables ✅
- Model: tables (D9), outline with flat positions, table row/column/table commands, editing rules
  across cells and tables. Property test: 3 replicas, concurrent table/row/column/cell edits
  converge and every table stays rectangular.
- Layout: equal-width columns, 5pt cell padding, borders, hit testing by cell, Up/Down stay in
  the caret's column. Rows that don't fit split across pages at line boundaries (Google Docs
  default): the piece on the first page reaches the bottom margin, the rest continues at the top
  of the next page with its own borders. A row with no line that fits moves to the next page.
- Editor: Insert table grid picker (10 × 10), right-click menu (insert row above/below, column
  left/right, delete row/column/table), Tab / Shift+Tab, Tab in last cell adds a row.
- `npm run e2e` covers clicking/typing in cells, Tab, Arrow Down, the picker and the menu.

## M6 — Sync server ✅
`npm run e2e:collab` drives two Chrome tabs through the real server (needs `npm run serve` and
`npm run sync-server`). Tested on one machine over localhost only.
- WebSocket server sharing `opendox-model`; storage trait with SQLite adapter; update batching;
  snapshot compaction with watermark; remote carets/presence.
- Done: two browsers edit the same doc live; offline edits merge on reconnect; compaction test.
- Built: `opendox-sync` (protocol + client session state, D10), `opendox-server` (axum WebSocket
  at `/doc/{id}`, `Storage` trait with SQLite and in-memory adapters, compaction every 200
  updates and when the last client leaves), wasm sync calls, `Collaboration` in the editor
  package (batched sends every 30 ms, reconnect with backoff, remote carets with name flags).

## M7 — Rich text and undo ✅
`npm run e2e` covers Ctrl+B/I, the font menu, Ctrl+Shift+E, undo/redo keys and the Undo
button; `npm run e2e:collab` checks formatting reaches the other tab.
- Bold/italic/size/font marks, paragraph attributes (alignment, spacing), local-only undo/redo.
- Built (D11): character formats as Loro marks; paragraph formats (alignment, line spacing,
  space before/after) on each paragraph's line break; Enter keeps the format on both halves,
  joining keeps the first paragraph's. Layout: italic faces, alignment incl. justify (carets
  match the stretched spaces), line spacing, spacing in body and cells. Undo on Loro's
  `UndoManager`: only this user's edits, restores the selection, a burst of typing is one
  step. Bundled Noto Sans Italic/Bold Italic and Noto Serif (4 faces). Demo toolbar.
- Property test: 3 replicas, concurrent text edits, character and paragraph formatting,
  delivered in random order, converge to identical text and formats.

## M8 — Export ✅
Checked by hand: the demo's PDF read back with poppler (`pdftotext`) and PyMuPDF, and its
.docx opened in Microsoft Word, which reports the same formats, a 3×3 bordered table and the
same 3 pages, and renders it close to the editor (differences in Known debt).
- `opendox-export` (D12): **PDF** drawn from layout output: every glyph at the position layout
  computed, table borders, subsetted embedded fonts, ToUnicode + ActualText so text extracts
  correctly (ligatures, combining marks, emoji, Arabic). **.docx** from the model: text,
  bold/italic/size/font, alignment, line spacing, space before/after, tables with borders and
  equal columns, page size and margins.
- Done: tests read the PDF back and check page count/size, every glyph position against layout,
  borders, embedded subset fonts, extractable text and byte-identical output; .docx tests check
  the package parts and the generated XML. Demo has Download PDF / Download .docx;
  `npm run e2e` checks both exports from the live page.

## M9 — Images ✅
`npm run e2e:images` drives the demo: insert, select, corner resize, side squish, rotate,
wrap menu, drag, options panel, Delete + Ctrl+Z, and checks both exports carry the image.
Checked by hand: the .docx opens in Microsoft Word with the picture, size, square wrapping and
alt text, and Word's rendering matches our PDF.
- D13: placeholder in the paragraph + per-property image record; in line, wrap text, break
  text, behind text, in front of text; size, rotation, offset, recolor, brightness, contrast,
  transparency, alt text. Insert button, paste and drag-and-drop of image files.
- Editor: frame with 8 handles (corners keep proportions, Shift frees them; sides squish),
  rotate handle (snaps to right angles, Shift = 15° steps), drag to move (positioned images
  re-anchor to the paragraph they land beside), arrow keys nudge, Delete removes. Demo:
  floating toolbar (wrap menu, Image options, Delete) and the Image options panel.
- Tests: model (convergence, undo, paragraph formats beside images, cells), layout (inline
  boxes, carets, break bands, wrapping incl. following paragraphs, too-wide images, layers,
  repeatability), wasm (placement, wrap switch keeps position, drag, delete/undo), export
  (PDF picture + mask + transform, .docx parts and XML).

## Known debt
- Images: wrapped text uses one side only (the wider one). No cropping, no "fix position on
  page" (positioned images always move with their paragraph), no image margins setting, no
  copy/paste of images inside the editor (pasted text drops them), no alt text in the PDF
  (needs tagged PDF), no rich picture of the image for screen readers in the editor (canvas).
  Images live inside the document, so many large photos make big snapshots (server message
  limit is 32 MB).
- Images: a slow live preview on very large images (adjustment sliders re-process the pixels
  in JS on every move).
- `opendox-model` grapheme checks scan the whole document text on every edit (O(n)). Fine for
  now; must become per-paragraph before large documents are usable.
- WASM bundle is 5.5 MB (mostly Loro; PDF writing and font subsetting added 1.2 MB), not
  size-tuned yet (no LTO / opt-level "z"). Export could move to a separately loaded module.
- Renderer draws every page on every render; should draw only pages in view.
- No emoji or CJK font bundled: those characters render as empty boxes (layout is correct).
- Pagination restacks every line on every call.
- Editing gaps: no Ctrl+Arrow word movement, no double/triple-click selection, no rich
  clipboard (plain text only, so pasting drops formatting).
- Export gaps: the .docx names fonts but doesn't embed them, so Word on a machine without Noto
  substitutes other fonts (and then breaks lines differently). Word also places the extra
  space of line spacing below each line while our layout splits it above and below, so 1.5 and
  double spacing look slightly different in Word. The PDF positions every glyph separately
  (larger content streams than run-based text), has no tagged structure (not PDF/UA), no
  outline/bookmarks and no hyperlinks.
- Formatting gaps: no underline, strikethrough, colour, highlight, links, styles (Heading 1…),
  indents or lists. Only two font families (Noto Sans, Noto Serif). A family with no italic
  face (Noto Sans Arabic) draws upright: we never slant glyphs synthetically.
- Every layout reads all marks of every container (`to_delta`), O(n) per keystroke like the
  outline rebuild.
- Undo steps are split by a heuristic: typing that continues where the last edit ended joins
  the step if within 1 s; anything else starts a new one. Google Docs also splits at words.
- A format chosen at a caret (Ctrl+B, then type) is remembered by position only; moving away
  and back to the same spot before typing still applies it.
- Two people giving the same last paragraph of a cell a format at the same time: last writer
  wins per property (plain map values), same as for marks.
- Arrow keys move in logical order; in right-to-left text Left/Right feel reversed. Visual
  movement for bidi text is still to do.
- Every keystroke redraws every page (about 1.5 ms per page); should redraw only changed,
  visible pages.
- The editor injects its own stylesheet; apps can't theme the caret/selection yet.
- Table gaps: no column resizing (always equal widths), no merged cells, no nested tables, no
  header-row repeat, no "keep row together" option (rows always split when they don't fit), no
  cell-rectangle selection (selection inside a table is linear, row by row), no cell
  background/border styles.
- Sync server has no authentication or authorization: anyone who can reach it can open and
  edit any document id. It binds to 127.0.0.1 by default. Needs an auth trait (D4).
- Server storage calls are synchronous and run on the async runtime's threads while the room is
  locked. Fine for local SQLite; a networked store needs its own queue.
- Presence names and colours are whatever the client sends; nothing stops two people picking
  the same colour.
- A client keeps unsent edits only in memory: closing the tab while offline loses them. Needs
  local persistence (IndexedDB).
- Remote edits relayout the whole document, like local ones.
- Outline and `Document::text` rebuild from the whole document on every keystroke (O(n)). Cheap (arithmetic only), but could stop early
  once page breaks match the previous pass.

## Later
Lists and numbering, tables, headers/footers, footnotes, bidi, comments/suggestions,
clipboard HTML, PDF and .docx export, accessibility mirror DOM.
