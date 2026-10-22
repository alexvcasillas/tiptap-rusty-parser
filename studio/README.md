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
```

Not published. Not part of the parser's CI matrix (needs native windowing libs).

## Layout

```
title bar   tiptap-studio · [Inspect | Diff] · file name · node stats
toolbar     Open · Save · Undo · Redo · JSON · Schema
panes       tree (280) · preview (fluid) · inspector (320)
```

## Modules

| File | Purpose |
| --- | --- |
| `main.rs` | boots `gpui_kit::application()`, applies the theme, opens one `Workspace` window |
| `state.rs` | `DocumentState` entity: parser `Document` + selection path + expanded set + undo/redo. `edit()` runs a `Transform`, keeps the `Change` log, stores `invert()` for undo. Row summaries and type helpers |
| `tree.rs` | `TreePanel`: flattens visible nodes into rows on state change, renders with `uniform_list`; chevron toggles, click selects |
| `preview.rs` | `PreviewPanel`: read-only render; click a block to select it; unknown types render as a dashed amber block |
| `inspector.rs` | `InspectorPanel`: identity, attrs (value + remove), mark chips, structure actions, JSON peek |
| `workspace.rs` | title bar, toolbar, actions and key bindings, three-pane layout |
| `theme.rs` | visual tokens from the design canvas, applied over the gpui-component dark theme |
| `assets/sample.json` | demo document with headings, marks, link, list, quote, a custom `callout`, rule, code block |

## Rules

- Depend only on `gpui-kit`. Never add `gpui` directly; it pins the `gpui-pre` snapshot.
- All mutations go through `DocumentState::edit`. Panels never hold `&mut Node`.
- Selection is an index path `Vec<usize>` (root = `[]`), the same addressing `Node::node_at` and every `Change` use. Dropped when it stops resolving.
- Unknown node and mark types must render and be selectable. Never match exhaustively on type.
- Perf: no per-frame clones once panels are stable. Known cost: `edit` clones the root once before the transaction so `invert` has a base.
- `cargo check -p tiptap-rusty-studio` and `cargo fmt --all` before every commit.

## Roadmap

- [ ] Typed attribute editors (string → input, number → stepper, bool → switch, object/array → JSON textarea)
- [ ] Add attribute; add mark with attrs (link `href`)
- [ ] Wrap in…, split, join
- [ ] Diff mode: second document, `Document::diff` change list, tinted rows in both trees, before/after inspector
- [ ] Open / save dialogs, recent files
- [ ] Resizable panes via `DockArea`
- [ ] Schema panel surfacing `Violation`s
- [ ] WASM build as a web playground
