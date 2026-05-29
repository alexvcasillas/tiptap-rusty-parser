# tiptap-rusty-parser

Fast, schema-agnostic parser & manipulator for [Tiptap](https://tiptap.dev) /
ProseMirror `JSONContent` documents, in Rust.

- **Schema-agnostic** — any node/mark `type` accepted; unknown fields preserved
  for lossless roundtrip.
- **Query** via predicate closures — `find`, `find_all`, `walk`, `descendants`.
- **Mutate** in place — marks, attrs, children, text, bulk `replace_all`.
- **Build** ergonomically — `Node::element`, `Node::text`, `doc(..)`, `with_*`.

## Example

```rust
use tiptap_rusty_parser::{Document, Mark, Node};

let mut doc = Document::from_json_str(
    r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"hi"}]}]}"#,
)?;

// Bold every text node.
doc.replace_all(
    |n| n.node_type.as_deref() == Some("text"),
    |n| { n.add_mark(Mark::new("bold")); },
);

// Append a paragraph.
doc.push_child(Node::element("paragraph").with_text("bye"));

let json = doc.to_json_str()?;
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

## API

| Area     | Methods |
|----------|---------|
| Parse    | `Document::from_json_str` / `from_value` / `from_reader` |
| Serialize| `to_json_str` / `to_string_pretty` / `to_value` |
| Query    | `find`, `find_mut`, `find_all`, `find_all_mut`, `walk`, `walk_mut`, `descendants` |
| Marks    | `add_mark`, `remove_mark`, `toggle_mark`, `has_mark`, `get_mark`, `set_mark_attr`, `clear_marks` |
| Attrs    | `attr`, `set_attr`, `remove_attr`, `attrs_mut` |
| Children | `child`, `child_mut`, `child_count`, `children`, `push_child`, `insert_child`, `remove_child`, `replace_child`, `clear_children`, `retain_children` |
| Build    | `Node::element`, `Node::text`, `Node::text_with_marks`, `doc`, `with_attr`, `with_child`, `with_text`, `with_mark` |
| Bulk     | `replace_all` |

## Develop

```sh
cargo test     # unit + integration + doctests
cargo bench    # criterion baselines
```

## License

MIT
