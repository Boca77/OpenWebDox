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

## Open
- Shaping crate — M2 spike.
- Glyph drawing method — M4 spike.
