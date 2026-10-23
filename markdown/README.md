# tiptap-rusty-markdown

Fast Markdown ⇄ Tiptap / ProseMirror JSONContent conversion, built on
[`tiptap-rusty-parser`](https://crates.io/crates/tiptap-rusty-parser) and
[`pulldown-cmark`](https://crates.io/crates/pulldown-cmark).

```rust
use tiptap_rusty_markdown::{from_markdown, to_markdown};

let doc = from_markdown("# Hi\n\nSome **bold** and a [link](https://tiptap.dev).\n");
assert_eq!(doc.root().children()[0].node_type.as_deref(), Some("heading"));

let md = to_markdown(&doc);
assert_eq!(md, "# Hi\n\nSome **bold** and a [link](https://tiptap.dev).\n");
```

## Import: `from_markdown` / `from_markdown_with`

CommonMark plus GFM (tables, task lists, strikethrough) to Tiptap StarterKit names:

| Markdown | JSONContent |
| --- | --- |
| paragraph, `#`…`######`, `>` | `paragraph`, `heading{level}`, `blockquote` |
| `-` / `1.` lists | `bulletList` / `orderedList{start}` `> listItem` |
| code fence | `codeBlock{language}` |
| `---`, hard break | `horizontalRule`, `hardBreak` |
| `**` `*` `~~` `` ` `` | marks `bold` `italic` `strike` `code` |
| `[t](u "x")` | mark `link{href, title?}` |
| `![a](s "t")` | `image{src, alt, title?}` |
| GFM table | `table > tableRow > tableHeader/tableCell > paragraph` |
| GFM task list | `taskList > taskItem{checked}` |

Output is always schema-valid (list items start with a paragraph, containers
are never empty) and never panics.

`ParseOptions`:

| Field | Default | |
| --- | --- | --- |
| `gfm` | `true` | GFM tables, task lists, strikethrough |
| `html` | `HtmlPolicy::Text` | raw HTML kept as text, or `Drop` |
| `max_depth` | `128` | max JSON nesting of the result (`serde_json`'s limit), deeper structure is flattened, text kept |

## Export: `to_markdown` / `to_markdown_with`

Any `Node` tree to Markdown. Marks are merged across adjacent text nodes,
escaping keeps the output re-parsing to the same document, tables become GFM
pipe tables (HTML `<table>` fallback for complex cells), underline/sub/sup become
inline HTML.

`RenderOptions`:

| Field | Default | |
| --- | --- | --- |
| `unknown_node` | `Transparent` | unknown node types: render children, `Html` (via `to_html`), or `Skip` |
| `bullet` | `'-'` | `-`, `*` or `+` |
| `emphasis` | `'*'` | `*` or `_` |

Options are plain serde data (no closures), ready for WASM/FFI.

## Limitations

- GFM bare-URL autolinks (`https://…` without `<>`) stay text.
- Table column alignment is ignored.
- Underline/sub/sup and `<br>` in cells export as HTML, which reimports as text under `HtmlPolicy::Text`.
- Rare emphasis nesting between punctuation can re-parse differently.

## Performance

Single pass over the event stream, no intermediate AST. On a 1 MB document
(release, criterion): import ≈ 55 ms, export ≈ 9 ms.
`cargo bench -p tiptap-rusty-markdown`.

## License

MIT
