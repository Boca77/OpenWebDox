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

## M10 — Lists, numbering and indents ✅
`npm run e2e:lists` drives the demo: the sample list, checkbox clicks + undo, Enter/Enter on an
empty item, "* " autoformat, Tab/Shift+Tab, Ctrl+Shift+7/8, Backspace at an item's start, the
toolbar, and both exports. `npm run e2e:collab` checks a list made in one tab reaches the other.
- D14: bullets (● ○ ■ by level), numbers (1. a. i. by level), checklists (☐/☑), nine levels,
  list id so separate lists number separately and one list continues across other paragraphs.
  Left, right and first-line indents for any paragraph. Numbering is computed, never stored.
- Editor: toolbar buttons (toggle), increase/decrease indent, Tab/Shift+Tab change level,
  Enter on an empty item outdents or ends the list, Backspace at an item's start ends it,
  Enter after a ticked item gives an unticked one, typing "* ", "- ", "1. ", "1) " or "[] " at a
  paragraph start makes a list, click a checkbox to tick it, Ctrl+[ / Ctrl+].
- Demo: the three spacing selects moved behind one "Line and paragraph spacing" button (as in
  Google Docs) so the format bar still fits one row at 1000 px with the list buttons.
- Export: PDF draws markers from layout (they extract as text before their item); .docx gets
  `word/numbering.xml`, one numbering per list, `<w:numPr>` and `<w:ind>` per paragraph.
- Tests: model (numbering by level/list, Enter/join, indent, ticks, cells, undo, concurrent
  toggles, 3-replica convergence property test), layout (indents vs a narrower paragraph,
  hanging marker, wide marker, empty item, centring, wrapping, pagination), export (PDF text
  extraction, .docx numbering XML).

## M11 — Underline, strikethrough, colour, highlight, super/subscript, links ✅
`npm run e2e:formats` drives the demo: Ctrl+U, Alt+Shift+5, the colour and highlight panels,
Ctrl+., Ctrl+K into the link field, the link bubble (shows, opens over https, Remove), typing
after a link, Ctrl+K at a caret inserting the URL, ticked checklist items, and both exports.
- D15: six new character formats as Loro marks (`link` never expands at its edges, so typing
  just after a link isn't linked; the rest expand after like bold). Link text is blue and
  underlined unless it has its own colour. Ticked checklist items are drawn grey and struck
  through.
- Layout: `TextStyle` gained colour, highlight, underline, strikethrough and baseline shift;
  each glyph run carries its colour; lines carry decoration rectangles placed from each font's
  own underline/strikethrough metrics; superscript/subscript shift glyphs without moving
  carets or the line. Colour changes split shaped runs without moving any glyph.
- Editor: toolbar U, S, text colour, highlight and link buttons; Ctrl+U, Alt+Shift+5, Ctrl+.,
  Ctrl+,, Ctrl+K; Ctrl+click opens a link (http, https and mailto only); a bubble under a link
  at the caret with Edit / Remove.
- Export: PDF fills each run in its colour, draws highlights behind and lines over the text,
  and adds a link annotation per line of each link. .docx gets `w:u`, `w:strike`, `w:color`,
  run shading for highlights, `w:vertAlign`, and `w:hyperlink` with external relationships.
- Tests: model (marks, link edges, paragraph-start typing, mixed selections, links from two
  replicas; the formatting convergence property test now includes all six), layout (run
  splitting keeps glyphs and text mapping, decoration placement, superscript, pagination,
  cache), export (PDF colours/rects/annotation, .docx XML).
- The demo format bar wraps to two rows below ~1250 px; the browser checks now scroll a target
  into view when it's hidden.

## M12 — Paragraph styles, outline and bookmarks ✅
`npm run e2e:formats` also covers styles: the sample's Heading 1 sections in the outline, the
style menu, Ctrl+Alt+2, the title's size, and clicking an outline entry.
- D16: Normal text, Title, Subtitle, Heading 1–6 as a `p_style` paragraph key, with Google
  Docs' default sizes, colours and spacing in one table (`StyleName::look`). Text with its own
  size/colour keeps it; a paragraph's own spacing wins over the style's when non-zero. Enter
  at the end of a heading or title starts normal text.
- Editor: style menu, Ctrl+Alt+0–6, `headings()` / `goTo()`; the demo shows a document
  outline left of the pages when the window is at least 1320 px wide, marking the caret's
  section. The size box shows the style's size (adding it to the list when missing).
- Export: .docx paragraphs use Word's Title/Subtitle/Heading 1–6 styles (defined in
  `styles.xml` with outline levels, so Word's navigation pane and TOC see them); the PDF gets
  nested bookmarks (title above headings) opening each heading's page.
- Tests: model (styles stored by name, Enter at end/inside, join, convergence incl. styles),
  wasm (size, spacing, headings, Enter), export (.docx styles, nested PDF bookmarks with the
  right pages).

## M13 — Page breaks, page setup, headers, footers, page numbers ✅
`npm run e2e:pages` drives the demo: Ctrl+Enter, Backspace removing a break before joining,
the Page setup panel (A4 landscape, 0.5 in margins, impossible margins refused), the Header &
footer panel (header on every page, Add page numbers, a click in page 2's footer, Ctrl+A in
the footer, page count at the cursor, removing the header) and both exports.
`npm run e2e:collab` checks a header reaches the other tab.
- D17: page break = a `p_break` paragraph key (stays with the first half on Enter; one undo
  step for Ctrl+Enter). Page setup = a `page` map, one key per value (merges per margin;
  combined values that leave no room fall back to the default). Header and footer = their
  own text containers enabled by `page` flags, in their own regions of the position space
  after the body; moving, deleting and selecting never cross regions. Page number / page
  count fields = the object placeholder marked `field`.
- Layout: `layout_document` lays out the header and footer after the body (so the page count
  is known), places them 0.5 in from the page edges on every page, and fills each field per
  page in a box sized for the widest number (aligned with the paragraph). Carets, Up/Down
  and selections in a header or footer use the copy on the page clicked
  (`set_margin_page`); Up/Down never leave it.
- Demo: Page setup and Header & footer panels; pages wider than the window now scroll
  instead of being cut off on the left.
- Export: PDF draws headers, footers and each page's numbers (extracting in reading order);
  .docx gets `w:pageBreakBefore`, page size/margins from the document, and `header1.xml` /
  `footer1.xml` with `PAGE` / `NUMPAGES` fields.
- Tests: model (page setup merge/validation, Ctrl+Enter undo, break placement, regions,
  fields, removal, anchors, sync), layout (break never makes an empty page, margins on every
  page, per-page numbers, body unchanged, field caret stops, margin page, Up/Down,
  right-aligned numbers), export (PDF page numbers per page, .docx parts and fields), wasm.

## M14 — Word-level editing ✅
`npm run e2e:editing` drives Ctrl+Left/Right, Ctrl+Shift+Right, Ctrl+Backspace/Delete (and
undo), double-click, triple-click and Shift+click.
- Words are UAX #29 word segments (`unicode-segmentation`): "can't", "3.50" and "café" are one
  word each, each Han character is one. Ctrl+Right goes to the next word's start, then the
  paragraph's end, then the next paragraph (Windows / Google Docs); Ctrl+Left to the start of
  the word before. Never across the body/header/footer boundary. Alt on macOS.
- Double-click selects the word (or run of spaces) under the pointer, triple-click the
  paragraph. The view counts clicks itself (Chrome's pointer events always report
  `detail = 0`); typing between clicks starts a new count.
- Tests: model (stops, apostrophes/numbers/accents, CJK, emoji, regions, double-click ranges).

## M15 — Rich clipboard ✅
`npm run e2e:editing` also copies formatted text (plain text + HTML with the exact content),
pastes it back (bold and colour kept, one undo step), pastes Google Docs-shaped HTML
(heading, bold, a `font-weight:normal` override, red, a link, nested bullets) and cuts.
- D18: a `Clip` (paragraphs with their character and paragraph formats) is what copy takes and
  paste puts in, as one change. On the clipboard: `text/plain`, and `text/html` with ordinary
  tags and inline styles for other editors plus the clip's JSON in `data-opendox-clip`.
  Pasting prefers that JSON, else converts the HTML in the editor (`htmlToClip`), else plain
  text. Pasted paragraphs only take their own paragraph format where they fill a paragraph;
  pasted lists become new lists; pasted text never picks up the format around it.
- Tests: model (copy cut to range, JSON, lossless paste into another document, landing in a
  paragraph, whole-paragraph rule, no inherited format, one undo step, HTML tags and lists).

## M16 — Access control, view-only mode, offline copy ✅
Rust tests cover tokens (issue, expiry, other documents, forged access/expiry/document, other
secrets, junk), view-only sessions (told first, edits refused) and the real server refusing
connections without a valid token. `npm run e2e:collab` now also: opens a document with no
server reachable, types, waits for the device copy, reopens online and checks the other tab
gets the text; and checks a read-only editor ignores typing and Backspace.
- D19: the server asks an `Authorizer` per connection (`token` query parameter). Built in:
  `AllowAll` (development) and `SignedTokens` (HMAC-SHA256 `{doc}.{view|edit}.{expiry}.{mac}`,
  `*` for all documents; issued by the application, `opendox-server --issue` for testing).
  `--secret` (or `OPENDOX_SECRET`) turns it on; without one the server refuses to listen
  beyond localhost unless `--insecure`. View-only connections get a `ReadOnly` message,
  their edits are refused, and the client stops sending edits.
- Editor: `setReadOnly()` (typing, deleting, paste, drop, cut, image moves and checkbox
  clicks do nothing; moving, selecting and copying work; commands throw), set automatically
  from the server. `Collaboration` takes a `token`. `LocalCopy` keeps each shared document in
  IndexedDB (saved 300 ms after changes and when the page is hidden), loaded before
  connecting. Demo: `&token=`, "View only" status, disabled controls, "Saved on this device".

## M17 — Long documents: draw what's seen, edit locally ✅
Measured in headless Chrome on a 115-page, 400k-character document (median of 15):
keystroke (insert + layout + render + caret) 214 ms → 12.5 ms; render 170 ms → 2.4 ms; model
insert 16 ms → 0.1 ms. `cargo run --release -p opendox-model --example keystroke` times the
model calls natively. All browser suites pass with the new renderer.
- Renderer: pages keep their size but only those within a window height of the screen are
  painted; the rest are marked stale and painted by an `IntersectionObserver` as they come
  near, and far pages free their canvas (115 full canvases were ~400 MB). Canvases are only
  resized when the size changes. `paintAll()` paints everything (printing, screenshots).
- Model: grapheme checks look at a 64-char window around the position; a paragraph's format
  is found by reading forward from the caret to its end; one character's or a selection's
  format is read with `slice_delta` — none of them scan the whole document any more.

## M18 — Emoji and CJK ✅
`npm run e2e:editing` types emoji (drawn from the bundled font) and Chinese, Korean and
Japanese (fonts loaded on demand; the check reports SKIP when they aren't downloaded). Layout
tests: emoji incl. a ZWJ family as one glyph, no taller lines; CJK coverage and wrapping.
- D20: Noto Emoji (monochrome outlines, 2 MB) is bundled and in the demo's default family
  list. CJK fonts (static Noto Sans SC/JP/KR Regular OTFs, 4.5–8.3 MB) are downloaded by
  `npm run fetch-fonts` into `fonts/noto/` (git-ignored) and loaded only when
  `missing_chars()` reports characters no registered font draws; kana picks JP, other Han
  SC, Hangul KR.
- Engine: `missing_chars()` — the characters drawn with the missing-glyph box in the last
  layout, for any app's own font loading.

## M19 — Comments ✅
`npm run e2e:comments` (1400 px window) comments a selection from the toolbar and a word with
Ctrl+Alt+M, checks the card sits level with its text, the strong/light highlights, replying,
closing and reopening a card from the text, non-overlapping cards, resolve, delete, and the
.docx. `npm run e2e:collab` checks a comment and a reply cross between tabs.
- D21: a comment is a record in `comments` (range as two CRDT cursors, author, text, time,
  resolved, replies). Typing at a range's edges stays outside it, inside grows it; deleting
  all its text leaves the comment with an empty range. Comments overlap freely and merge
  per field between replicas.
- Layout: open comments highlight their text (light yellow, darker for the open card) where
  the text has no highlight of its own, through the existing highlight decorations.
- Editor: `addComment`, `comments`, `replyToComment`, `resolveComment`, `editComment`,
  `deleteComment`, `commentAtCaret`, `setActiveComment`, Ctrl+Alt+M (`onCommentRequest`).
  Demo: cards beside the page at their text's height (a drawer below 1320 px), updated in
  place so a reply being typed survives others' edits.
- Export: .docx `comments.xml` with ranges and references in the text; replies follow as
  paragraphs; resolved comments are left out.

## Known debt
- Images: wrapped text uses one side only (the wider one). No cropping, no "fix position on
  page" (positioned images always move with their paragraph), no image margins setting, no
  copy/paste of images inside the editor (pasted text drops them), no alt text in the PDF
  (needs tagged PDF), no rich picture of the image for screen readers in the editor (canvas).
  Images live inside the document, so many large photos make big snapshots (server message
  limit is 32 MB).
- Images: a slow live preview on very large images (adjustment sliders re-process the pixels
  in JS on every move).
- Every layout still rebuilds the outline (2.9 ms at 400k chars, native) and reads every
  container's marks for formats (2.3 ms), and pagination restacks every line: about 7 ms of a
  12.5 ms keystroke at 115 pages, growing with the document. Making them incremental needs
  Loro change events mapped to paragraphs.
- WASM bundle is 4.5 MB raw, 1.7 MB gzipped (mostly Loro; PDF writing and font subsetting
  about 1.2 MB), built with the `wasm-release` profile (opt-level "s" + LTO; "z" was smaller but
  much slower). Export could move to a separately loaded module.
- Emoji are monochrome (colour formats COLR/CBDT/SVG can't be drawn as paths yet, D8).
  Variable fonts are drawn at their default instance, so bold CJK/emoji from a variable font
  would draw regular (the CJK fonts used are static, so this doesn't happen today). Japanese
  documents with no kana get Chinese glyph forms. parley logs "No segmentation model for
  complex script: Chinese/Japanese" (word segmentation; line breaking is unaffected). Thai,
  Devanagari and other scripts still have no bundled font.
- Pagination restacks every line on every call.
- Clipboard: images and tables don't travel (a table's cells paste as paragraphs, images are
  dropped); pasted fonts are dropped (only bundled families exist) and pasted HTML colours
  must be hex or rgb(). Pasting HTML from other apps reads inline styles, not stylesheets
  (`<style>` rules, Word's classes), so some Word formatting is lost. No "paste without
  formatting" shortcut yet (Ctrl+Shift+V).
- Word movement: no word-wise drag selection after a double-click (it extends by
  characters).
- Export gaps: the .docx names fonts but doesn't embed them, so Word on a machine without Noto
  substitutes other fonts (and then breaks lines differently). Word also places the extra
  space of line spacing below each line while our layout splits it above and below, so 1.5 and
  double spacing look slightly different in Word. The PDF positions every glyph separately
  (larger content streams than run-based text), has no tagged structure (not PDF/UA), no
  outline/bookmarks and no hyperlinks.
- Formatting gaps: no font colour picker beyond 16 swatches plus the system picker.
- Styles: fixed looks (no "update Heading 1 to match", no custom styles); a paragraph can't
  set its space before/after to exactly 0 when its style has some (0 means "use the style's").
  No table of contents block yet. Headings inside table cells show in the outline and the
  PDF bookmarks too (Google Docs leaves them out). Only two font families (Noto Sans, Noto Serif). A family
  with no italic face (Noto Sans Arabic) draws upright: we never slant glyphs synthetically.
- Character formats: an underline or highlight on trailing spaces at a wrapped line's end is
  drawn over the space that hangs past the edge; underlines aren't skipped under descenders.
  Superscript/subscript don't make the line taller, so a large superscript can reach the line
  above. Highlight height is the font's ascent + descent, not the whole line. Link checks:
  none — any text is accepted as a URL; Ctrl+click opens only http(s)/mailto (bare domains get
  https). The PDF's link areas are the selection boxes of the link text (one per line).
- Lists: no custom marker styles, start numbers or "restart/continue numbering" commands; the
  marker stays at the left in right-to-left and centred/right-aligned items; a ticked item
  isn't greyed or struck through (needs strikethrough); .docx checklists are plain ☐/☑
  bullets (Word has no checklist); no indent ruler (indents only via buttons/keys/API).
  A marker that would reach its text pushes the first line right by a fixed gap instead of to
  a tab stop. Tab outside lists and tables still moves focus out of the editor (no tab stops).
- Undo steps are split by a heuristic: typing that continues where the last edit ended joins
  the step if within 1 s; anything else starts a new one. Google Docs also splits at words.
- A format chosen at a caret (Ctrl+B, then type) is remembered by position only; moving away
  and back to the same spot before typing still applies it.
- Two people giving the same last paragraph of a cell a format at the same time: last writer
  wins per property (plain map values), same as for marks.
- Arrow keys move in logical order; in right-to-left text Left/Right feel reversed. Visual
  movement for bidi text is still to do.
- Every edit repaints every visible page (2–3 at a time), changed or not.
- The editor injects its own stylesheet; apps can't theme the caret/selection yet.
- Table gaps: no column resizing (always equal widths), no merged cells, no nested tables, no
  header-row repeat, no "keep row together" option (rows always split when they don't fit), no
  cell-rectangle selection (selection inside a table is linear, row by row), no cell
  background/border styles.
- Access: tokens can't be revoked before they expire (keep them short-lived); a refused
  WebSocket looks like a network error in the browser, so the client keeps retrying and
  shows "Offline" rather than "Not allowed". No per-user identity: presence names are
  still whatever the client sends.
- Server storage calls are synchronous and run on the async runtime's threads while the room is
  locked. Fine for local SQLite; a networked store needs its own queue.
- Presence names and colours are whatever the client sends; nothing stops two people picking
  the same colour.
- Offline copy: the whole document is rewritten on every save (fine for text, slow with many
  large images); a change in the last ~300 ms before the browser kills a tab can be lost;
  the copy isn't encrypted at rest and isn't cleared on sign-out unless the app calls
  `LocalCopy.clear()`.
- Remote edits relayout the whole document, like local ones.
- Outline and `Document::text` rebuild from the whole document on every keystroke (O(n)). Cheap (arithmetic only), but could stop early
  once page breaks match the previous pass.

- Headers/footers: one for all pages (no different first page or odd/even), at a fixed 0.5 in
  from the edges; a header taller than the top margin overlaps the body (Google Docs pushes
  the body down). Page numbers always count from 1, in Arabic numerals. A shorter number sits
  in a box as wide as the widest one, so "Page 9 of 12" has a little extra space after 9.
  Clicking a header edits it directly (Google Docs asks for a double-click).
- Page setup: no custom paper sizes in the demo (the API takes any); the header/footer
  distances aren't settable yet.

- Comments: no suggestions (tracked changes) yet; resolved comments can't be browsed or
  reopened from the demo (the API can); no @mentions or notifications; Word gets replies as
  paragraphs, not threaded replies (`commentsExtended`), and no resolved state; the PDF has
  no comment annotations. Anyone who can edit can resolve or delete anyone's comment (no
  per-user permissions), and view-only users can't comment.

## Later
Footnotes, bidi caret movement, suggestions (tracked changes), accessibility mirror DOM.
