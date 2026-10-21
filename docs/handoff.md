# Handoff — what's next (2026-10-21)

Read `CLAUDE.md`, then `docs/roadmap.md` (milestones + Known debt) and `docs/decisions.md`
(D1–D12) before changing anything. This file is the short version: where things stand, what to
do next, and the traps.

## Where things stand

Milestones M0–M8 are done and pushed to `main` (last: `d14b95e`, M8 export).

| Area | State |
|---|---|
| Model (`opendox-model`) | Loro CRDT: text, tables, char marks, paragraph formats, local undo |
| Layout (`opendox-layout`) | parley shaping, UAX #14 breaking, pagination, tables split across pages, alignment/justify, spacing |
| Sync (`opendox-sync`, `opendox-server`) | WebSocket hub, SQLite storage + compaction, offline merge, remote carets |
| Editor (`packages/editor`) | canvas renderer, IME, selection, formatting shortcuts, collaboration |
| Export (`opendox-export`) | print-exact PDF from layout; .docx from the model |

Images (M9, D13) are built: in line / wrap / break / behind / in front, resize, squish,
rotate, drag, recolor, adjustments, alt text, PDF and .docx export. `npm run e2e:images`.

Not built: lists, headers/footers, footnotes, links, comments, auth.

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

### 1. Lists and numbering (biggest gap for real documents)
Suggested shape, following D11:
- Model: paragraph format keys on the paragraph's terminator, e.g. `p_list` (`bullet` /
  `number` / none), `p_level` (0–8), and a list id so separate lists number separately. Enter
  in a list item continues the list; Enter on an empty item ends it; Tab / Shift+Tab change level
  (Tab already means "next cell" inside tables, so check `tableInfo()` first).
- Layout: the editor computes the marker text ("•", "3.", "iii.") and indent per paragraph and
  passes them in; layout draws the marker in the indent and wraps text at the indent. Keep
  layout free of list semantics (CLAUDE.md: layout takes plain input).
- Export: PDF is automatic. .docx needs `word/numbering.xml` + `<w:numPr>` per paragraph.
- Tests: convergence property test with concurrent list toggles; numbering after concurrent
  inserts; golden layout tests for marker placement.

### 2. Export follow-ups (owner flagged export as important)
- Embed fonts in the .docx so Word shows Noto everywhere: obfuscated `.odttf` parts +
  `w:embedRegular/Bold/Italic/BoldItalic` in `fontTable.xml`. Costs ~0.6 MB per font face;
  embed only faces actually used.
- **Owner decision needed:** line spacing. Word puts the extra space of 1.5/double spacing
  below each line; our layout (parley half-leading) splits it above and below. Check what Google
  Docs does before changing. Changing layout moves lines in the editor too.
- Bundle size (5.5 MB raw, ~2.0 MB gzip): load export as a separate wasm module on first
  Download click; try `opt-level = "z"` + LTO for release wasm.
- Hyperlinks/bookmarks/tagged PDF come with links and headings.

### 3. Must-fix before anyone real uses it
- **Server has no auth** (anyone reaching it can edit any doc). D4 says auth is a trait.
- Offline edits live only in memory; closing the tab offline loses them (needs IndexedDB).

### 4. Performance debt (see Known debt)
Every keystroke is O(document): outline rebuild, `formats()` reading all marks, grapheme scans,
and redrawing every page. Fine at 3 pages, not at 100. Make these per-paragraph and draw only
visible pages before large-document work.

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
- `.cargo/config.toml` is local and untracked (target dir outside OneDrive, LLVM for wasm).
  CRLF warnings on commit are harmless.

## Open questions for the owner
1. Line-spacing convention (see 2 above).
2. Is ~0.6 MB per embedded font acceptable in .docx files?
3. Confirm the order: lists → export follow-ups → auth/offline → performance.
