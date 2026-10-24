# tiptap-rusty-parser

A fast, schema-agnostic Rust crate to parse, query, edit, diff and validate
[Tiptap](https://tiptap.dev) / ProseMirror `JSONContent` documents.

- **Any schema.** Every node and mark type is accepted. A schema is optional and only
  used when you validate.
- **Faithful roundtrip.** Unknown node types and unknown JSON fields are kept and written
  back.
- **Fast.** Queries return borrows instead of clones, and edits happen in place.

It is the engine behind [Tiptap Studio](https://tiptap-studio.alexvcasillas.dev), and is also published for JavaScript as
[`@tiptap-studio/rusty-parser`](https://www.npmjs.com/package/@tiptap-studio/rusty-parser).

**Documentation:** [guide](https://tiptap-studio.alexvcasillas.dev/docs/parser) · [API reference](https://docs.rs/tiptap-rusty-parser)

## Install

```toml
[dependencies]
tiptap-rusty-parser = "0.6"
```

## Quick start

```rust
use tiptap_rusty_parser::{Document, Mark, Node, Query};

let mut doc = Document::from_json_str(
    r#"{"type":"doc","content":[
        {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"Notes"}]},
        {"type":"paragraph","content":[{"type":"text","text":"hello"}]}
    ]}"#,
)
.unwrap();

// Query with a closure, or with the query language.
assert_eq!(doc.by_type("paragraph").len(), 1);
let q = Query::parse("type == heading && attrs.level == 1").unwrap();
assert_eq!(q.find_paths(doc.root()), vec![vec![0]]);

// Bold every text node and append a paragraph.
let changed = doc.replace_all(
    |n| n.node_type.as_deref() == Some("text"),
    |n| {
        n.add_mark(Mark::new("bold"));
    },
);
assert_eq!(changed, 2);
doc.push_child(Node::element("paragraph").with_text("bye"));

assert_eq!(doc.text_content_with_separator("\n"), "Notes\nhello\nbye");
```

## What it does

| Area | Guide |
| --- | --- |
| Parse and serialize, with source maps from nodes to JSON byte spans | [Parsing](https://tiptap-studio.alexvcasillas.dev/docs/parser/parsing) |
| Find nodes with closures, selectors or a query language (`type == heading && attrs.level > 1`) | [Querying](https://tiptap-studio.alexvcasillas.dev/docs/parser/querying), [Query language](https://tiptap-studio.alexvcasillas.dev/docs/parser/query-language) |
| Address nodes by index path or by ProseMirror position, and map positions through edits | [Paths and positions](https://tiptap-studio.alexvcasillas.dev/docs/parser/paths-and-positions) |
| Edit marks, attributes, children and text; normalize; range and block commands | [Mutating](https://tiptap-studio.alexvcasillas.dev/docs/parser/mutating), [Range and block editing](https://tiptap-studio.alexvcasillas.dev/docs/parser/range-and-block-editing) |
| Record edits as an invertible change log for undo | [Transactions](https://tiptap-studio.alexvcasillas.dev/docs/parser/transactions) |
| Apply position-addressed edits, as AI tools produce them | [Position editing](https://tiptap-studio.alexvcasillas.dev/docs/parser/position-editing) |
| Diff two documents into a change list; compose, compact, map and cherry-pick changes | [Diffing](https://tiptap-studio.alexvcasillas.dev/docs/parser/diffing), [Change algebra](https://tiptap-studio.alexvcasillas.dev/docs/parser/change-algebra) |
| Validate against a schema with ProseMirror-style content expressions | [Schema](https://tiptap-studio.alexvcasillas.dev/docs/parser/schema) |
| Extract text, render HTML, build nodes in code | [Text](https://tiptap-studio.alexvcasillas.dev/docs/parser/text), [HTML](https://tiptap-studio.alexvcasillas.dev/docs/parser/html), [Building nodes](https://tiptap-studio.alexvcasillas.dev/docs/parser/building-nodes) |

Upgrading from 0.5: see the [migration guide](https://tiptap-studio.alexvcasillas.dev/docs/parser/migration).

## License

MIT
