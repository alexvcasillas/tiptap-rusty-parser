# Migration guide

## 0.5 → 0.6

`Node` / `Mark` maps are boxed to keep nodes small (`Node` 240 → 112 bytes,
`Mark` 168 → 40). Parse ≈ 2× faster, ≈ 55% less memory. JSON wire format and
the WASM/JS API are unchanged; only Rust code touching the fields directly
needs updates.

| Field | 0.5 | 0.6 |
| --- | --- | --- |
| `Node.attrs`, `Mark.attrs` | `Option<Map<String, Value>>` | `Option<Box<Map<String, Value>>>` |
| `Node.extra`, `Mark.extra` | `Map<String, Value>` | `Option<Box<Map<String, Value>>>` |

| 0.5 | 0.6 |
| --- | --- |
| `node.attrs.as_ref()` | `node.attrs()` (or `node.attrs.as_deref()`) |
| `node.attrs.get_or_insert_with(Map::new)` | `node.attrs_mut()` |
| `node.attrs = Some(map)` | `node.attrs = Some(Box::new(map))` |
| `node.attrs.iter().flatten()` | `node.attrs().into_iter().flatten()` |
| `node.extra.get(k)` | `node.extra_field(k)` |
| `node.extra.insert(k, v)` | `node.extra_mut().insert(k, v)` |
| `node.extra.remove(k)` | `node.remove_extra(k)` |
| `node.extra.iter()` | `node.extra().into_iter().flatten()` |
| `Node { extra: Map::new(), .. }` | `Node { extra: None, .. }` |
| `mark.attrs.as_ref()` | `mark.attrs()` |
| `mark.extra.insert(k, v)` | `mark.extra_mut().insert(k, v)` |

```rust
use serde_json::json;
use tiptap_rusty_parser::{Document, Mark};

let mut doc = Document::from_json_str(
    r#"{"type":"doc","x-id":7,"content":[{"type":"paragraph","attrs":{"a":1}}]}"#,
).unwrap();
assert_eq!(doc.extra_field("x-id"), Some(&json!(7)));
doc.extra_mut().insert("x-rev".into(), json!(2));
assert_eq!(doc.remove_extra("x-id"), Some(json!(7)));

let p = &mut doc.children_mut()[0];
assert_eq!(p.attrs().and_then(|m| m.get("a")), Some(&json!(1)));
p.attrs_mut().insert("b".into(), json!(2));

let link = Mark::new("link").attr("href", "https://tiptap.dev");
assert_eq!(link.attrs().and_then(|m| m.get("href")), Some(&json!("https://tiptap.dev")));
```

Behaviour notes:
- `extra: None` and `Some(empty)` compare equal; removing the last extra key sets `extra` to `None`.
- Functions taking `Option<Map>` (`set_block_type`, `wrap`, `wrap_range`, `Transform` methods) and `PosEdit` are unchanged.
