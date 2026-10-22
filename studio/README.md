# tiptap-studio

Native desktop inspector, structural editor and differ for Tiptap/ProseMirror
JSON documents. Built on [GPUI](https://gpui.rs) through
[gpui-kit](https://gpui-kit.com) 0.8. Every document operation is delegated to
`tiptap-rusty-parser`; this crate is UI only and doubles as a live test bed for
the parser. If the UI needs something the crate cannot do, that is a crate
feature, not an app workaround.

```sh
cargo run -p tiptap-rusty-studio                 # bundled sample document
cargo run -p tiptap-rusty-studio -- path/to.json # open a file
cargo run -p tiptap-rusty-studio -- a.json b.json # A + B for Diff mode
```

Not published. Not part of the parser's CI matrix (needs native windowing libs).

## Layout

```
title bar   tiptap-studio · [Inspect | Diff] · file name · node stats
toolbar     Open · Save · Save as… · Undo · Redo · JSON · Schema
panes       tree (280) · preview (fluid) · inspector (320)

Diff mode
title bar   a.json → b.json · +n −n ~n · total changes
toolbar     … · Open B… · Swap (v1: disabled) · Apply all to A · Only changed · Sync scroll (v1: disabled)
panes       changes (300) · tree A (fluid) · tree B (fluid) · before/after (300)
keys        ↑/↓ step changes · ⏎ Take B · ⌫ Dismiss (again: restore) · ctrl/cmd-z undo
```

## Modules

| File | Purpose |
| --- | --- |
| `main.rs` | boots `gpui_kit::application()`, applies the theme, opens one `Workspace` window |
| `state.rs` | `DocumentState` entity: parser `Document` + selection path + expanded set + undo/redo. `edit()` runs a `Transform`, keeps the `Change` log, stores `invert()` for undo. Row summaries and type helpers |
| `tree.rs` | `TreePanel`: flattens visible nodes into rows on state change, renders with `uniform_list`; chevron toggles, click selects |
| `preview.rs` | `PreviewPanel`: read-only render; click a block to select it; unknown types render as a dashed amber block |
| `inspector.rs` | `InspectorPanel`: identity, attrs (value + remove), mark chips, structure actions, JSON peek |
| `diff.rs` | `DiffState` (doc B, `Document::diff` change list, per-path tints, recomputed on observe) and `DiffPanel` (change list, tinted trees A/B, before/after inspector). Take B = `rebase_change` + `Transform::apply_change` via `DocumentState::edit` |
| `workspace.rs` | title bar, Inspect/Diff `Mode`, toolbar, actions and key bindings, pane layout |
| `theme.rs` | visual tokens from the design canvas, applied over the gpui-component dark theme |
| `assets/sample.json` | demo document with headings, marks, link, list, quote, a custom `callout`, rule, code block |
| `assets/sample-v2.json` | edited copy of the sample, for Diff mode |

## Rules

- Depend only on `gpui-kit`. Never add `gpui` directly; it pins the `gpui-pre` snapshot.
- All mutations go through `DocumentState::edit`. Panels never hold `&mut Node`.
- Selection is an index path `Vec<usize>` (root = `[]`), the same addressing `Node::node_at` and every `Change` use. Dropped when it stops resolving.
- Unknown node and mark types must render and be selectable. Never match exhaustively on type.
- Perf: no per-frame clones once panels are stable. Known cost: `edit` clones the root once before the transaction so `invert` has a base.
- `cargo check`, `cargo clippy -p tiptap-rusty-studio -- -D warnings` and `cargo fmt --all` before every commit.
- Keyboard: arrows / Home / End / Esc navigate in the `Document` context (tree, preview); alt-↑/↓ move the node; Diff mode: ↑/↓ step changes, ⏎ Take B.

## Roadmap

- [x] Typed attribute editors (string → input, number → stepper, bool → switch, object/array → JSON textarea)
- [x] Add attribute; add mark with attrs (link `href`)
- [x] Wrap in…, split, join
- [x] Diff mode: second document, `Document::diff` change list, tinted rows in both trees, before/after inspector
- [x] Diff as a merge tool: Dismiss (⌫) leaves A's side and hides the change, Apply all skips dismissed, Save / Save as… writes the merged A (`<a>-merged.json` suggested)
- [ ] Diff: Swap A/B, sync scroll
- [x] Save (⌘S), Save as… (⇧⌘S)
- [ ] Open dialog for A, recent files
- [ ] Resizable panes via `DockArea`
- [ ] Schema panel surfacing `Violation`s
- [ ] WASM build as a web playground
