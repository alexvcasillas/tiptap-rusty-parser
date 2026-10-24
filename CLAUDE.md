# CLAUDE.md

## Interaction rules
- Be extremely concise; sacrifice grammar for concision.
- Always follow project info below.
- Commits: Conventional Commits spec.
- Commits: ONE LINERS.
- Performance IS A MUST.
- End each plan w/ concise list of unresolved questions (if any).
- Sacrifice grammar for concision.

## Project
Repo tiptap-studio, virtual workspace: `parser/` (crate tiptap-rusty-parser), `markdown/` (crate tiptap-rusty-markdown), `bindings/wasm/` (npm), `studio/` (desktop app).
tiptap-rusty-parser: Rust crate in `parser/`. Parse/walk/query/mutate Tiptap JSONContent (ProseMirror) docs.
- Schema-agnostic: accept any node/mark types, preserve unknown fields, faithful roundtrip.
- Query: predicate closures (find/find_all/walk).
- Mutation: in-place mutable tree (&mut).
- Pure lib, no bindings v1.
- Perf-first: avoid clones, borrow over copy, bench w/ criterion.

## markdown (tiptap-rusty-markdown)
- Markdown <-> JSONContent via pulldown-cmark; depends on parser. Keep pulldown/Markdown deps out of the parser crate.
- Import output must be schema-valid vs `markdown/tests/fixtures/gfm.schema.json`; export must stay idempotent (tests/roundtrip.rs).
- Goldens: `UPDATE_FIXTURES=1 cargo test -p tiptap-rusty-markdown`; bench: `cargo bench -p tiptap-rusty-markdown`.

## studio (tiptap-rusty-studio)
- GPUI desktop app in `studio/`. Depend only on `gpui-kit`; never add `gpui` directly (pinned snapshot).
- UI-only: document logic belongs in the parser crate. Missing capability → add it to the crate, not the app.
- All mutations via `DocumentState::edit` (Transform → Change log → invert for undo). No direct `&mut Node` from panels.
- Selection = index path `Vec<usize>`; drop it when it stops resolving.
- Unknown node types must render and be selectable.
- Run `cargo check -p tiptap-rusty-studio`, `cargo clippy -p tiptap-rusty-studio -- -D warnings` and `cargo fmt --all` before every commit.
- Headless smoke test on Linux: `xvfb-run -a ./target/debug/tiptap-studio` (needs mesa-vulkan-drivers for lavapipe).
