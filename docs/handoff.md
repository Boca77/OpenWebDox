# Handoff — what's next (2026-10-25)

Read `CLAUDE.md`, then `docs/roadmap.md` (milestones + Known debt) and `docs/decisions.md`
(D1–D21) before changing anything. This file is the short version: where things stand, what to
do next, and the traps.

## Where things stand

Milestones M0–M19 are done; **M20 (footnotes) is half built** — see "In progress" below.
M9 (images) is committed (`c62b55e`); M10–M20 may not be committed yet — check `git status`.
At the last stop everything was green: `cargo test --workspace`, clippy, fmt, and every
browser suite (e2e, lists, formats, pages, editing, comments, images, collab).

| Area | State |
|---|---|
| Model (`opendox-model`) | Loro CRDT: text, tables, char marks, paragraph formats, local undo |
| Layout (`opendox-layout`) | parley shaping, UAX #14 breaking, pagination, tables split across pages, alignment/justify, spacing |
| Sync (`opendox-sync`, `opendox-server`) | WebSocket hub, SQLite storage + compaction, offline merge, remote carets |
| Editor (`packages/editor`) | canvas renderer, IME, selection, formatting shortcuts, collaboration |
| Export (`opendox-export`) | print-exact PDF from layout; .docx from the model |

Images (M9, D13) are built: in line / wrap / break / behind / in front, resize, squish,
rotate, drag, recolor, adjustments, alt text, PDF and .docx export. `npm run e2e:images`.

Lists (M10, D14) are built: bullets, numbers, checklists, nine levels, indents, autoformat,
PDF and Word numbering. `npm run e2e:lists`.

Character formats (M11, D15) are built: underline, strikethrough, text colour, highlight,
superscript/subscript, links (toolbar, shortcuts, link bubble, PDF annotations, Word
hyperlinks). `npm run e2e:formats`.

Paragraph styles (M12, D16) are built: Title, Subtitle, Heading 1–6, style menu,
Ctrl+Alt+0–6, document outline in the demo, Word heading styles, PDF bookmarks.

Page layout (M13, D17) is built: Ctrl+Enter page breaks, page setup (size, orientation,
margins), header and footer with page number / page count fields. `npm run e2e:pages`.

Word-level editing (M14) and the rich clipboard (M15, D18) are built: Ctrl+Arrow,
Ctrl+Backspace/Delete, double/triple-click, copy/paste with formatting (to and from other
apps). `npm run e2e:editing`.

Access control and the offline copy (M16, D19) are built: signed tokens, view-only mode,
IndexedDB copy of shared documents. Run the server with `--secret` (or `OPENDOX_SECRET`) and
issue tokens with `--issue DOC view|edit SECONDS`.

Emoji and CJK (M18, D20) render: Noto Emoji bundled; CJK fonts via `npm run fetch-fonts`
(git-ignored), loaded on demand by the demo from `missing_chars()`.

Comments (M19, D21) are built: add, reply, resolve, delete, highlights, sidebar cards in the
demo, Word export. `npm run e2e:comments`.

Not built: suggestions (tracked changes), bidi caret movement, colour emoji. Footnotes are
in progress.

## In progress: M20 — Footnotes

Done (tested, nothing user-visible yet):
- **Model** (`crates/opendox-model/src/footnote.rs`, tests `tests/footnotes.rs`, 8 tests): a
  reference is the object placeholder marked `footnote = <id>` (mark `ExpandType::None`) in
  body or cell text; the note's text is a `LoroText` in the `footnotes` map under that id;
  the last paragraph's format lives in `footnote_tail` under `<id>|<key>`.
  `insert_footnote_at(outline, pos) -> id` (body/cells only, else `FootnoteOutsideBody`),
  `delete_footnote(outline, id)`. `TextRef::Footnote(id)`, `Region::Footnote`.
  `Outline::footnotes: Vec<FootnoteSlots { id, reference, slots }>` in number order (= order
  of references), each note a region after the footer; `footnote(id)`, `footnote_at(pos)`;
  `region_bounds` is per note, so selection/deletes never cross notes. Body references are
  found in the same delta pass as tables (`scan_body`), cell references by scanning cells
  whose text has a placeholder. Deleting a reference drops the note; undo restores both.
- **Layout** (`pagination.rs`, tests `tests/footnotes.rs`, 7 tests):
  `Paginator::layout_with_notes(engine, blocks, header, footer, &[NoteInput])`
  (`layout_document` now calls it with no notes). A reference is an object with
  `ObjectMode::Field(FieldKind::Note(n))`, sized to its number and drawn as a `PlacedLabel`
  in the style at its placeholder. Each note goes at the foot of the page holding its
  reference's line, under a rule a third of the width (`NOTE_SEPARATOR` = 12 pt of space);
  a line whose notes don't fit moves to the next page. Note paragraphs are numbered after
  the footer's (`note_paragraphs()`), their lines appended last in `placed_lines` with
  `Zone::Footnote`. `shape_label` now applies `baseline_shift` (so superscript labels and
  markers are raised; existing markers pass 0).
- wasm: only `region()` returns 3 for a footnote.

To do, in order:
1. **wasm `Editor::layout()`** (`crates/opendox-wasm/src/lib.rs` ~line 1370): for footnote
   slots use a 10 pt default size (Google Docs); for each `outline.footnotes[k]` find the slot
   holding `reference`, push an `ObjectInput { mode: Field(FieldKind::Note(k+1)), key:
   usize::MAX, .. }` at its byte plus a superscript `StyleSpan` over the placeholder
   (`SCRIPT_SCALE`, shift `size * 0.33`, as `style_spans` does); give each note's first slot
   a marker `"k+1"` in superscript style; build `NoteInput { paragraph: slot index, byte,
   paragraphs: note slots mapped through para }` and call `layout_with_notes`. Until this is
   done, a document with a footnote (none can be made from the UI yet) would lay out its
   note slots nowhere.
2. wasm API: `insert_footnote(pos) -> caret at the note's start` (fresh outline after the
   edit), `footnote_reference(pos)` (in a note → its reference, for jumping back),
   `footnote_near(pos)` (a reference at/just before the caret → its note), `delete_footnote`.
3. Editor/demo: Ctrl+Alt+F (Google Docs) and an Insert → Footnote toolbar button; clicking
   a reference number jumps to its note. Load the design-hub skill first (CLAUDE.md).
4. .docx: `word/footnotes.xml` with separator (-1) and continuation separator (0) notes,
   `<w:footnoteReference w:id=".."/>` runs (superscript), content-type override and a
   relationship from `document.xml`. PDF needs nothing (labels and rules are drawn already);
   check with `pdftotext`.
5. Tests: wasm unit test for layout with a footnote; `scripts/e2e-footnotes.mjs` +
   `npm run e2e:footnotes`; a collab check that a note reaches the other tab.
6. Docs: roadmap M20 section + Known debt (below), decision D22, CLAUDE.md command line.

Known limits so far (for the roadmap's Known debt): a note never continues onto the next
page (a note taller than the space below its line moves the line; one taller than a page
overlaps the body); notes referenced in a table row are added after the row piece is
placed, so they can push past the bottom margin; no footnote numbering styles (always
1, 2, 3…, never restarting per page); images in notes are laid out in line.

## How to run and verify

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run build && npm run serve        # demo at http://127.0.0.1:5174/demo/
npm run e2e                           # needs serve
npm run sync-server                   # separate terminal; rebuild it after model changes
npm run e2e:collab                    # needs serve + sync-server
```

Checking exports by hand (not in CI):
- PDF text: `pdftotext -enc UTF-8 file.pdf -` (poppler ships with Git for Windows).
- PDF render: `pip install --target <scratch dir> pymupdf`, then render with `pymupdf`.
- .docx: Microsoft Word is installed; drive it from PowerShell with
  `New-Object -ComObject Word.Application` (open read-only, inspect, `ExportAsFixedFormat` to
  PDF). Always `Quit()` in a `finally`.

## Next, in priority order

### 1. Export follow-ups (owner flagged export as important)
- Embed fonts in the .docx so Word shows Noto everywhere: obfuscated `.odttf` parts +
  `w:embedRegular/Bold/Italic/BoldItalic` in `fontTable.xml`. Costs ~0.6 MB per font face;
  embed only faces actually used.
- **Owner decision needed:** line spacing. Word puts the extra space of 1.5/double spacing
  below each line; our layout (parley half-leading) splits it above and below. Check what Google
  Docs does before changing. Changing layout moves lines in the editor too.
- Bundle size (5.5 MB raw, ~2.0 MB gzip): load export as a separate wasm module on first
  Download click; try `opt-level = "z"` + LTO for release wasm.
- Hyperlinks/bookmarks/tagged PDF come with links and headings.

### 2. Remaining performance debt (see Known debt)
M17 took a 115-page keystroke from 214 ms to 12.5 ms (only visible pages are painted; model
edits no longer scan the document). What's left is the per-layout outline/formats rebuild and
restack (~7 ms at 115 pages).

## Traps that cost time before

- **Shell + non-ASCII:** never round-trip source through PowerShell `Get-Content`/`Set-Content`
  (garbles UTF-8). In Git Bash heredocs, `\\n` inside inline Python got turned into real
  newlines once: for scripted multi-line edits, write the script to a file and run it.
- **Undo merging** is decided per edit by `History::before_edit(.., merge)`; Loro's
  `record_new_checkpoint` does *not* stop the next edit merging into the previous step.
- **Paragraph formats** live on the paragraph's `\n` (or table placeholder); the last paragraph
  of a container keeps its format beside it (`body_tail` map / `<column id>|p_*` row keys). Split
  copies, join keeps the first. Read D11 before touching insert/delete.
- **Paragraph indices** in layout = `Outline::slots` order (body and cell paragraphs in reading
  order). The wasm layer and PDF export both rely on this.
- The demo clears undo history after seeding the sample text (`clear_history`); do the same
  after any programmatic document setup.
- **Fallback fonts change line heights.** A character no bundled font has (emoji) is drawn
  with the missing-glyph box of the *last* family in the list, using that font's metrics.
  Adding Noto Sans Symbols 2 to the text's family list made such lines taller and moved page
  breaks, so it's only given to list markers (`set_symbol_font`).
- **Browser checks click at fixed coordinates** without scrolling: anything that makes the
  demo header taller (a toolbar wrapping to a second row at 1000 px) pushes targets off-screen
  and fails a dozen unrelated checks. Measure the header if many clicks fail at once.
- **Synthetic Enter doesn't submit forms.** A CDP `rawKeyDown` for Enter has no text, so the
  browser never fires the implicit form submit; send `keyDown` with `text: "\r"` when the
  focus is in a form field.
- **`len()` is the body only.** Positions run on into the header and footer; clamp with
  `end()`, and use `region_bounds()` for "start/end of document" behaviour.
- **Click counting is the view's.** Pointer events report `detail = 0`, so double/triple
  clicks are counted by time and distance; browser checks must pause ~600 ms between
  separate click gestures at one spot, or they become one multi-click.
- **Pages off screen aren't painted.** A test reading pixels of a page far below the window
  must scroll to it or call `view.renderer.paintAll()` first.
- After `git stash` experiments, rebuild (`npm run build`): `pkg/` keeps whatever was built
  last, and a demo calling a newer wasm API fails at startup with `window.opendox` undefined.
- `.cargo/config.toml` is local and untracked (target dir outside OneDrive, LLVM for wasm).
  CRLF warnings on commit are harmless.

## Open questions for the owner
1. Line-spacing convention (see 1 above).
2. Is ~0.6 MB per embedded font acceptable in .docx files?
3. Confirm the order: export follow-ups → performance.
