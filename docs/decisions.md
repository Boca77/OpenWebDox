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

## Open
- Glyph drawing method — M4 spike.
