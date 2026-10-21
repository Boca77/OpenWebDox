# opendox

Open-source collaborative document editor library (Google Docs class). Real-time sync,
paginated print-accurate layout. Layout quality is the product: treat it as the top priority.

## Architecture (decided — see docs/decisions.md before changing any of it)

- **Rust core, compiled to WASM for the browser and native for the server.** Same crates on both
  sides so layout and sync behave identically everywhere.
- **CRDT: Loro.** Do not write custom CRDT/sequence logic. Formatting uses Loro rich-text marks
  (Peritext-style ranges), never per-character attributes.
- **Text shaping in Rust with bundled fonts.** Never use browser `measureText`/`fillText` for
  layout or glyph placement. The renderer draws glyphs at the positions Rust computed.
- **Monorepo, many packages.** Cargo workspace in `crates/`, npm workspaces in `packages/`.

```
crates/
  opendox-model    document model on top of Loro (text, paragraphs, marks, positions)
  opendox-layout   shaping, line breaking, pagination, hit testing — no Loro dependency
  opendox-sync     sync protocol + client session state, no I/O (used by wasm and server)
  opendox-wasm     wasm-bindgen bindings for model + layout + sync
  opendox-server   WebSocket sync server (axum), storage trait, SQLite adapter
  opendox-export   PDF (from layout, print-exact) and .docx (from the model) export
packages/
  editor           TS: canvas renderer, input (hidden textarea, IME), selection, collaboration
demo/              dev harness page (plain ES modules, no bundler)
scripts/serve.mjs  zero-dependency static server for the demo
docs/
  roadmap.md       milestones and done-criteria — work milestone by milestone
  handoff.md       where things stand, what's next, known traps — read first
  decisions.md     decision log — append, never rewrite history
```

`opendox-layout` must not depend on Loro or `opendox-model`. It takes plain paragraphs + style runs
in and returns geometry out, so it can be tested and reused in isolation.

## Layout rules

- Internal unit is **points (pt)** as `f32`. Convert to device pixels only in the renderer
  (zoom × devicePixelRatio). Shape at the font's design units / a fixed reference size.
- Output is **glyph runs** with cluster → text-offset mapping, not per-character boxes.
- Line breaking follows UAX #14, never "split on spaces". Overlong words must break, not overflow.
- Text positions exposed to callers are **grapheme-cluster safe**: never place a cursor inside a
  grapheme. Be explicit about units (UTF-8 bytes, UTF-16 units, graphemes) in names and docs.
- Layout is cached **per paragraph**; pagination restacks cached paragraph heights.

## Conventions

- Rust edition 2021, stable toolchain. `cargo fmt` and `cargo clippy -- -D warnings` clean.
- No `unsafe` without a `// SAFETY:` comment and a reason in the PR.
- Public APIs get doc comments. Internal code: comment the why, not the what.
- Errors: `thiserror` in libraries; no `unwrap()`/`expect()` outside tests except on proven invariants
  (comment the invariant).

## Testing

- Every milestone ships with tests that prove its done-criteria (see roadmap).
- Model: property/fuzz tests that apply random concurrent edits on N replicas and assert they converge.
- Layout: golden tests (input text + width → expected line breaks / page breaks) using the bundled
  fonts only, so results are identical on every machine.

## Commands

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

npm install            # once
npm run build          # wasm-pack build of opendox-wasm + tsc for packages/editor
npm run serve          # demo at http://127.0.0.1:5174/demo/
npm run e2e            # browser interaction checks (needs serve running)
npm run e2e:images     # image checks: insert, resize, rotate, wrap, drag, panel, export (needs serve)
npm run sync-server    # sync server at ws://127.0.0.1:5175 (SQLite in the temp folder; --memory)
npm run e2e:collab     # two-tab collaboration checks (needs serve + sync-server running)
```

Collaborative demo: open `http://127.0.0.1:5174/demo/?doc=<any-id>` in two tabs.

Headless screenshots for visual checks:
`chrome.exe --headless=new --window-size=900,1150 --virtual-time-budget=10000 --screenshot=<png> http://127.0.0.1:5174/demo/`.
Virtual time freezes `performance.now()`, so timings shown in such screenshots read 0 ms.

## Environment notes

- The repo sits inside OneDrive. Cargo's build output is redirected outside OneDrive by a local,
  untracked `.cargo/config.toml` (`target-dir`). Never commit that file; never move `target/` back
  into the repo.
- Windows dev machine; shell commands in docs should work in PowerShell.
- Never round-trip source files through Windows PowerShell `Get-Content`/`Set-Content`: PS 5.1
  reads UTF-8 as ANSI and garbles every non-ASCII character (tests contain Arabic, CJK, emoji).
  Use the editor tools.
- Bundled fonts live in `fonts/` with their licenses (Noto: SIL OFL 1.1). Tests load them with
  `include_bytes!`; the library itself embeds no fonts.
- WASM builds need LLVM (clang + llvm-ar): Loro pulls in C code via `psm`. The local
  `.cargo/config.toml` sets `CC_wasm32_unknown_unknown` / `AR_wasm32_unknown_unknown` to the LLVM
  install. Check with `cargo build -p opendox-model --target wasm32-unknown-unknown`.
