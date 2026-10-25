# Decision log

Append new decisions at the bottom. To reverse one, add a new entry that supersedes it — don't edit
old entries.

## D1 — Rust for core, sync server and layout (2026-10-15)
Same crates compile to WASM (browser) and native (server, export), so layout and sync behave
identically everywhere. UI shell (toolbar, menus) stays in TypeScript.

## D2 — Use Loro as the CRDT (2026-10-15)
Rejected: writing our own (months of convergence work, interleaving and tie-break bugs) and Yrs
(weaker rich-text formatting model). Loro gives Peritext-style marks, run-length storage, undo,
and Rust + WASM support. Risk: younger ecosystem; pin versions and wrap it behind `opendox-model`
so it could be swapped.

## D3 — Text shaping in Rust with bundled fonts; we draw glyphs ourselves (2026-10-15)
Browser `measureText` differs across browsers/OSes, so page breaks would differ between users and
from PDF export. Costs accepted: bundled fonts only (no user system fonts), larger WASM, custom
glyph drawing. Shaping crate (parley vs cosmic-text) and glyph drawing method (Path2D outlines vs
glyph atlas vs WebGPU) are open — decided by spikes in M2 and M4.

## D4 — Monorepo, several published packages (2026-10-15)
Cargo workspace + npm workspaces in one repo, published as separate crates/packages
(model, layout, wasm, editor, server, export). Storage and auth are traits, not hardcoded.

## D5 — Repo stays in OneDrive; build output redirected (2026-10-15)
Owner's choice. Cargo `target-dir` points outside OneDrive via an untracked
`.cargo/config.toml`. `node_modules` will still sync; keep JS dependencies minimal.

## D6 — MIT license (2026-10-15)
Maximum adoption: companies can embed it without legal review friction. Rejected AGPL (blocks most
corporate use) and MIT/Apache-2.0 dual (owner preferred plain MIT).

## D7 — parley for shaping and line breaking (2026-10-15)
Spike: parley 0.12 vs cosmic-text 0.19, bundled Noto fonts only, six cases (plain wrap, overlong
word, mixed sizes, Arabic, CJK, ZWJ emoji + combining marks). Line breaks were identical in every
case. Chose parley because:
- 2x faster (3.6 ms vs 7.2 ms for a 2000-word paragraph, release build);
- line height comes from the fonts actually on the line (cosmic-text uses a fixed per-span value);
- inline boxes (needed for inline images), built-in cursor/selection/hit testing;
- same Linebender ecosystem as vello, a candidate for M4 glyph drawing.
Must build layouts with `quantize = false`: with it on, parley snaps metrics to whole units
(16.34pt lines become 17pt), which would drift page breaks. Risk: 0.x API churn — parley is
hidden behind opendox-layout's own types.

## D8 — Draw glyphs as cached Path2D outlines on canvas (2026-10-15)
Rust (skrifa) extracts each glyph's outline as SVG path data in font units; the renderer caches
one `Path2D` per glyph and fills it at the position layout computed. The browser's text engine is
never used. Measured in headless Chrome, 3 Letter pages / 7,184 glyphs: first draw 49 ms (builds
the path cache), redraw 4-5 ms at 100% and 200% zoom. Fast enough that a glyph atlas (more
complex, blurry when zoomed) isn't needed. Chrome and Edge screenshots were byte-identical.
Known limits: no hinting, so small text is slightly softer than native text; colour emoji
(COLR/CBDT) can't be drawn this way and need separate handling.

## D9 — Tables: body placeholder + column-keyed table containers (2026-10-18)
A table is one U+FFFC character in the body text, marked `table = <id>` (mark expand: none), plus
`tables[id] = { columns: MovableList<column id>, rows: MovableList<Map<column id, Text>> }`.
- Keeping the table's place as a body character means splitting paragraphs and typing around a
  table stay ordinary text edits, which are concurrency-safe. (A list-of-block-containers model
  loses text typed concurrently into the tail of a paragraph being split.)
- Cells keyed by column id, not index, so a column added or removed concurrently with a new row
  still lines up; missing cells read as empty and are created on first write.
- Editors use one "flat position" space: body chars, with each table replaced by its cells in
  reading order. The JS side stays number-based.
- Selection/delete rules: within one table across cells → clear those cells; spanning body and a
  table → remove the whole table. Backspace/Delete never cross a cell or table edge.
Known risk: two replicas creating the same missing cell concurrently (only possible after a
concurrent row + column insert) keep one container; the other's text in that cell is lost.
Rejected: tables as flat start/cell/end markers (needs repair after every concurrent edit).

## D10 — Sync: version handshake, then batched updates through a hub server (2026-10-18)
Clients talk to one server per document over a WebSocket (`/doc/{id}`), not peer to peer.
- On connect the client sends its Loro version; the server replies with what the client lacks
  plus its own version, and the client sends back what the server lacks. That one exchange
  covers first load, reconnect and offline edits alike, so there is no separate offline queue.
- Clients batch local edits (one message per ~30 ms) and never echo back what they received:
  the client tracks what the server has, merging in every remote edit it imports.
- The server keeps a live replica per open document, stores each new update as a row, and
  forwards only the newly applied part (never a client's resend) to the others.
- Storage is a snapshot plus updates after a watermark (the last update the snapshot contains).
  Compaction writes snapshot + watermark and deletes updates at or below it, in one
  transaction; an older watermark never replaces a newer one.
- Presence (name, colour, selection as encoded Loro cursors) is relayed, not stored. Cursors
  resolve in each replica, so a remote caret stays on its text through edits. Model anchors
  now work inside table cells too.
- A client that falls behind (outbox full) is disconnected rather than buffered without limit;
  the handshake on reconnect catches it up.
- Protocol and client state live in `opendox-sync`, with no I/O, so the browser (via wasm) and
  native tests run the same code. Server: axum + tokio, rusqlite (bundled SQLite).
Rejected: Loro's own awareness/ephemeral store (would expose Loro types past `opendox-model`);
sending every keystroke as its own message.

## D11 — Formatting as marks; paragraph formats on the line break; Loro undo (2026-10-19)
- Character formats (`bold`, `italic`, `size`, `font`) are Loro rich-text marks that expand
  after, so typing at the end of bold text stays bold. Text typed at the start of a paragraph
  copies the format of the character after it, as in Google Docs.
- Paragraph formats (alignment, line spacing, space before/after) are marks that never expand,
  on the character ending the paragraph: its `\n` (or a table placeholder). The last paragraph
  of a container has none, so its format sits beside the container (`body_tail` map, or
  `<column id>|p_*` keys in a table row). Inserting line breaks copies the paragraph's format to
  each new break; deleting breaks keeps the first paragraph's format.
  Rejected: marks over the paragraph's text (an empty paragraph has nothing to mark), and a
  separate list of paragraph records (would need re-linking after every concurrent split).
- Line spacing is a multiple of the fonts' natural line height (parley `MetricsRelative`), as in
  Google Docs. Space before is dropped at the top of a page.
- Undo/redo uses Loro's `UndoManager`, which undoes only this replica's edits and still works
  after others edited around them. Each step stores the selection from before it (Loro cursors)
  and undo restores it. The editor decides which edits may merge into one step (continued
  typing within 1 s) and forces a new step otherwise; `record_new_checkpoint` alone doesn't.
- Bundled Noto Sans Italic/Bold Italic and Noto Serif Regular/Bold/Italic/Bold Italic (same
  source and licence as the existing Noto files) so italics are real faces and there's a second
  family. Adds about 4.3 MB to the repo.

## D12 — Export: PDF from layout, .docx from the model (2026-10-21)
New crate `opendox-export`, depending on both `opendox-layout` and `opendox-model` (layout still
depends on neither).
- **PDF is drawn from layout output**, never re-laid out: each glyph is placed with its own
  text matrix at the exact position layout computed, so pages and line breaks match the editor
  and every browser. Fonts are embedded as subsets (`subsetter`) in CID-keyed Type 0 fonts whose
  codes are subset glyph ids (Identity-H), written with `pdf-writer` (both from Typst, pure
  Rust, so export also runs in the browser). Text extraction: layout now records which text
  each glyph draws (`GlyphRun::glyph_text`); a ToUnicode map covers ordinary glyphs, and
  characters drawn with several glyphs or with the missing-glyph box get an `ActualText` span,
  since a ToUnicode map is per glyph id and can't express those.
- **.docx is written from the document model**, not from layout: Word re-lays out text with its
  own engine, so the useful content is structure and formatting (runs, paragraph properties,
  tables, page setup). A small built-in ZIP writer (deflate via `miniz_oxide`) avoids a zip
  dependency. Output is deterministic (fixed timestamps), as is the PDF.
Rejected: printing through the browser (`window.print`), which would lay text out with the
browser's engine and break D3; and producing .docx from layout with absolute positioning,
which Word users couldn't edit.

## D13 — Images: placeholder in the paragraph, per-property record, layout-side wrapping (2026-10-21)
- **Model.** An image is one U+FFFC in its paragraph marked `image = <id>` (expand: none), plus
  `images[id]` = a Loro map with the file bytes, mime, pixel size and one key per property
  (size, rotation, wrap mode, x/y offset, recolor, brightness, contrast, transparency, alt).
  Same reasoning as D9: typing and splitting around an image stay plain text edits. One key
  per property means concurrent changes to different properties both survive. Deleting the
  placeholder keeps the record, so undo brings the image back. The bytes are stored in the
  document and synced like everything else; the browser re-encodes inserts as JPEG (opaque)
  or PNG (transparency) at ≤ 2000 px to keep that bounded. Because tables and images share
  U+FFFC, the paragraph-format lookup now asks which U+FFFC are tables instead of trusting
  the character.
- **Layout (still model-free).** `ParagraphInput.objects` carries each image's byte offset,
  size, rotation, mode and offset. Placeholders are shaped as zero-width spaces (same 3 UTF-8
  bytes) and draw nothing. In line → parley inline box sized to the rotated bounding box,
  shrunk to the line if wider. Break text → a band of its own before the line holding it.
  Behind / in front → placed at paragraph top + offset, no effect on text. Wrap text → lines
  beside the image get the widest gap left (Word's "largest side"; never both sides), lines
  with no gap ≥ 36pt move below it; pagination redoes layout until text settles (≤ 5 passes,
  one pass when no image wraps), with the wrap areas in the paragraph cache key. Images in
  table cells are always in line.
- **Look.** Recolor (grayscale/sepia/negative), then brightness/contrast, by one formula
  implemented in Rust (`opendox_export::adjust`, PDF) and TypeScript (renderer) — keep them
  identical. PDF embeds unadjusted JPEGs as-is and decodes the rest (`png`, `zune-jpeg`).
  .docx maps to Word pictures (inline / anchor with square, top-and-bottom, behind or none
  wrapping); sepia becomes a duotone, negative isn't expressible.
Rejected: images as their own blocks between paragraphs (in-line images and wrapping need
them inside paragraphs), storing images outside the document (needs a blob service; later,
if documents with many photos get too big to sync), wrapping text on both sides (parley
breaks one span per line).

## D14 — Lists as paragraph formats; numbering computed; markers drawn by layout (2026-10-25)
- **Model.** A list item is a paragraph whose D11 paragraph format carries `p_list` (bullet /
  number / check), `p_level` (0–8), `p_list_id` and `p_checked`, next to new `p_ind_start`,
  `p_ind_end` and `p_ind_first` indents. So Enter copies list membership to the new item and
  joining keeps the first item's, with no new merge rules, and each key is its own mark so
  concurrent changes to different keys both survive. Numbers are never stored: `list_markers`
  counts items per list id in reading order (a level's count resets after a shallower item),
  so concurrent inserts, deletes and level changes can't leave numbers stale. Applying a list
  right after an item of the same kind joins that list; anything else gets a fresh id (a hash
  of a replica-unique id). A list level adds its own indent (0.5 in per level, marker hanging
  0.25 in) on top of the paragraph's.
- **Layout (still model-free).** `ParagraphStyle` gained the three indents and
  `ParagraphInput.marker` takes the marker text and style; layout shapes the marker on its own,
  puts it on the first line's baseline at `indent_start + indent_first`, and moves the first
  line's text right if the marker would reach it. Indented paragraphs break lines through the
  same per-line loop as wrapping around images. `ParagraphLayout.marker` keeps its own text, so
  PDF export maps its glyphs to text without touching the paragraph's.
- **Fonts.** Bundled Noto Sans Symbols 2 (OFL, 1.2 MB) for ● ○ ■ ☐ ☑. It is given to markers
  only (`Editor::set_symbol_font`), never added to the text's family list: as the last
  fallback it would draw the missing-glyph box for characters no font has (emoji), and its deep
  descent made those lines taller, moving page breaks.
- **.docx.** One `abstractNum` + `num` per list (Word continues numbering across `num`s that
  share an `abstractNum`); a checklist gets one per ticked state, since bullets are per level.
Rejected: storing numbers (stale after concurrent edits); lists as their own containers
(needs re-linking after concurrent splits, like D11's rejected paragraph records); the editor
computing markers in TypeScript (PDF export needs them in Rust too).

## D15 — More character formats as marks; layout draws decorations (2026-10-25)
- **Model.** `underline`, `strike`, `color` and `highlight` (`#rrggbb`), `script` (`super` /
  `sub`) and `link` (URL) are rich-text marks like D11's, each its own key so concurrent
  changes merge per property. Links are configured `ExpandType::None`, so typing just after
  (or just before) a link never extends it; typing inside one does. Text typed at a
  paragraph's start copies the next character's format but never its link.
- **Layout (still model-free).** `TextStyle` carries colour, highlight, underline,
  strikethrough and a baseline shift. Colour, highlight and shift ride in parley's style
  "brush"; underline/strikethrough use parley's decorations, so their offset and thickness
  come from each font's own metrics. Lines return decoration rectangles; glyph runs return
  their colour. A style change splits a shaped run into pieces; each piece's glyph→text
  mapping is its slice of the whole run's (visual order), so PDF text extraction is unchanged.
  Super/subscript are the caller's choice of size and shift (wasm: 65 % size, raised 33 % /
  lowered 14 % of the text's size, as in Word); the shift moves glyphs and decorations only,
  never carets or line height.
- **Look decided above layout.** Link blue + underline, and grey + strikethrough for ticked
  checklist items, are applied where the wasm layer turns formats into `TextStyle`s, so the
  model stores only what the user chose and export can make its own mapping.
- **Exports.** PDF: run colours as fill colour, highlights before the text, lines after, link
  annotations over the link text's selection boxes. .docx: highlight as run shading
  (`w:shd`), since `w:highlight` allows only 16 named colours.
Rejected: drawing decorations in the renderer from glyph boxes (PDF export would need the same
logic again, and fonts' underline metrics would be lost); a link as an object holding text
(splits, joins and concurrent typing would need special rules marks already give).

## D16 — Paragraph styles by name, looks from one table (2026-10-25)
- A paragraph's style is one more D11 paragraph key, `p_style` = `title` / `subtitle` /
  `h1`…`h6` (absent for normal text). The model stores only the name; `StyleName::look()`
  holds Google Docs' default size, colour, slant and spacing for each, so every replica,
  the editor and both exports agree, and the looks can change later without migrating data.
- The look is a paragraph's *base*: text with its own size or colour keeps it. Spacing from
  the style applies where the paragraph's own is 0 (the stored default), so no "unset" state
  is needed in the model.
- Enter at the end of a heading or title gives normal text (Google Docs); inside one, both
  halves keep the style (D11 split rule).
- Bookmarks: PDF outline items nested by level (title 0 above headings 1–6), each opening
  its heading's page at its height. .docx: Word's built-in style ids (`Heading1`, `Title`…)
  with `w:outlineLvl`, so Word treats them as real headings.
Rejected: storing size/colour on each heading's text (a style change would need rewriting
marks over the text, and Word export couldn't tell a heading from big text).

## D17 — Page breaks, page setup, headers and footers (2026-10-25)
- **Page break** is a paragraph key, `p_break` (starts on a new page). Splitting keeps it on
  the first piece only; Ctrl+Enter is one raw insert plus the key under a single commit, so
  it's one undo step. Backspace at the start of such a paragraph removes the break first.
- **Page setup** lives in the document (`page` map, one key per size/margin), so everyone
  sees the same pages and concurrent changes to different margins merge. A combination that
  leaves under 0.5 in for text (possible after merging) reads as the default rather than
  laying text out in no room.
- **Headers and footers** are separate rich-text containers (`header`, `footer`), present
  when the `page` map's `header`/`footer` flag is set (so an empty one exists and removing
  one is one change that also clears its text). In the flat position space they are regions
  after the body: header from body end + 1, footer after it. Arrow keys, Ctrl+A, Ctrl+Home/
  End, selections and deletes stay inside a region.
- **Fields** (page number, page count) are the object placeholder marked `field`; the model
  never stores a number. Layout knows the pages, so it lays out the header and footer after
  the body and gives each field an in-line box sized for the widest number (Noto's digits
  share one width), then each page draws its own number in it. One paragraph layout serves
  every page; only the number glyphs differ.
- A header/footer paragraph is placed on every page; navigation needs one copy, so the
  paginator keeps a per-page index and a "margin page" (the page last clicked) that carets,
  selections and Up/Down use.
Rejected: substituting the number into the paragraph text per page (byte offsets would
differ per page and break caret mapping); a header per page in the model (Google Docs and
Word have one per section, edited once).

## D18 — Clipboard: a JSON clip, carried inside ordinary HTML (2026-10-25)
- Copy turns the selection into a `Clip` (paragraphs: text, character runs, paragraph format
  incl. style and list) in the model, so what's copied is exactly what's stored. The clipboard
  gets plain text and HTML; the HTML has normal tags and inline styles (so Google Docs, Word
  and mail clients keep the formatting) and the clip's JSON in a `data-opendox-clip`
  attribute, so pasting between opendox documents (even across browsers, where custom MIME
  types don't survive) loses nothing.
- Paste is one model change (one undo step): the text goes in with an exact default format
  (never the neighbours'), then each run's format and, for paragraphs the paste fills, the
  paragraph format. List ids are remapped to fresh lists so pasted items don't join or
  renumber lists already in the document.
- HTML from other apps is converted in the editor (DOM parsing is the browser's job), into
  the same JSON, keeping what the model can hold. `serde_json` was already compiled in by
  Loro, so the format costs no bundle size.
Rejected: a binary or Loro-snapshot clipboard (not readable by other apps, and pasting a
snapshot would import foreign history); per-character HTML spans (huge clipboards).

## D19 — Access by signed tokens; view-only sessions; a copy on the device (2026-10-25)
- Authorization is a trait (`Authorizer`, as D4 planned) asked once per WebSocket with the
  document id and a `token` query parameter (browsers can't set WebSocket headers). The sync
  server keeps no users: an application decides who may open what and issues HMAC-SHA256
  signed tokens naming the document (or `*`), view or edit, and an expiry. Signature checks
  are constant-time (RustCrypto `hmac`/`sha2`; no hand-rolled crypto).
- Safe by default: no secret means everyone can edit everything, so the server then refuses
  any address but localhost unless `--insecure` says that's intended.
- View-only is enforced on the server (edits refused) and mirrored in the client: a
  `ReadOnly` protocol message makes the sync client stop sending and the engine refuse local
  edits in its single `begin()` gate, which every edit already passes through.
- Offline edits survive closing the tab through a full snapshot in IndexedDB, merged into
  the engine before connecting; the D10 handshake already sends whatever the server lacks,
  so no separate queue is needed.
Rejected: user accounts and passwords in the sync server (that's the embedding app's job);
storing only unsent updates locally (needs tracking what the server acknowledged across
sessions; a snapshot is simpler and also gives offline reading).

## D20 — Emoji bundled, CJK fonts downloaded on demand (2026-10-25)
- Noto Emoji (monochrome, OFL, 2 MB) is bundled: emoji are common in any document, and its
  outlines go through the D8 path renderer unchanged. Colour emoji stay open.
- CJK fonts are too big for the repo (static Noto Sans SC 8.3 MB, JP 4.5, KR 4.6), so a
  script downloads them from the noto-cjk project and they're git-ignored. Google Fonts'
  variable versions were rejected: their default instance is Thin, and the renderer draws
  default-instance outlines.
- The library stays font-agnostic (D3): the engine reports `missing_chars()` after layout and
  the app decides what to fetch. The demo loads a CJK font only when that report has
  characters it covers, re-checking after each load (JP also covers Han).
Rejected: adding fallback fonts to the family list up front (a last-resort font's metrics
change line heights of uncovered text, D14); WOFF2 subsets (the font library reads OpenType
only).

## D21 — Comments as records with cursor ranges (2026-10-25)
- Each comment is a Loro map in `comments`: start and end as encoded CRDT cursors, author,
  text, time, `resolved`, and a list of replies — one key per field, so concurrent replies,
  edits and resolves merge. Rejected: a mark per comment (marks need their keys configured
  up front, and one key per comment id doesn't fit), and one shared `comment` mark (overlap
  would need set-valued marks).
- The start cursor holds the range's first character and the end cursor its last, so text
  typed just outside the range stays outside (Google Docs). When the end cursor's character
  is deleted, it resolves to where that character was, which is the range's end; once all
  the text is gone the range is empty but the comment stays.
- Highlighting is decided where formats become `TextStyle`s (as for links): commented
  stretches get a highlight colour unless they have their own, so layout, the renderer and
  the PDF need nothing new.
- .docx: comments are Word comments with range markup; threading (`commentsExtended`) and
  resolved state aren't written yet.

## Open
- Colour emoji rendering.
