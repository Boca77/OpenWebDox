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

## Open
- Colour emoji rendering.
