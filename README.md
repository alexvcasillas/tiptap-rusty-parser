# tiptap-rusty-parser

Fast, schema-agnostic parser & manipulator for [Tiptap](https://tiptap.dev) /
ProseMirror `JSONContent` documents, in Rust.

- **Schema-agnostic** — any node/mark `type` is accepted; unknown JSON fields are
  preserved for lossless roundtrip.
- **Query** with predicate closures — `find`, `find_all`, `walk`, `descendants`.
- **Select** by type/mark/attr — `by_type`, `by_mark`, `by_attr`.
- **Address** by index path — `node_at`, `path_to`, `paths_to`.
- **Mutate** in place — marks, attrs, children, text, and bulk `replace_all`.
- **Extract text** — `text_content`, `char_count`, `word_count` (Unicode-aware).
- **Validate** (opt-in) — check against an allow-list `Schema` (Rust or JSON).
- **Build** ergonomically — `Node::element`, `Node::text`, `doc(..)`, `with_*` chaining.
- **JS/WASM** — `npm i tiptap-rusty-parser` for browser/bundler apps (see [JavaScript / WASM](#javascript--wasm)).
- **Fast** — borrow over copy, stack-based traversal (no recursion blowup), `lto`
  release profile, criterion benches.

---

## Table of contents

- [Install](#install)
- [Quick start](#quick-start)
- [Data model](#data-model)
- [Parsing & serializing](#parsing--serializing)
- [Querying](#querying)
- [Selectors](#selectors)
- [Node paths](#node-paths)
- [Mutating](#mutating)
  - [Marks](#marks)
  - [Attributes](#attributes)
  - [Children](#children)
  - [Text](#text)
  - [Bulk transforms](#bulk-transforms)
- [Text extraction](#text-extraction)
- [Schema validation](#schema-validation)
- [Building nodes](#building-nodes)
- [JavaScript / WASM](#javascript--wasm)
- [Error handling](#error-handling)
- [Performance](#performance)
- [Development](#development)
- [License](#license)

---

## Install

Add to `Cargo.toml`:

```toml
[dependencies]
tiptap-rusty-parser = "0.1"
```

Requires a recent stable Rust (edition 2021).

---

## Quick start

```rust
use tiptap_rusty_parser::{Document, Mark, Node};

fn main() -> Result<(), tiptap_rusty_parser::ParseError> {
    let mut doc = Document::from_json_str(
        r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"hi"}]}
        ]}"#,
    )?;

    // Bold every text node.
    doc.replace_all(
        |n| n.node_type.as_deref() == Some("text"),
        |n| { n.add_mark(Mark::new("bold")); },
    );

    // Append a new paragraph.
    doc.push_child(Node::element("paragraph").with_text("bye"));

    let json = doc.to_json_str()?;
    println!("{json}");
    Ok(())
}
```

---

## Data model

A Tiptap document is a tree of nodes. The crate mirrors Tiptap's `JSONContent`
shape directly.

```rust
pub struct Node {
    pub node_type: Option<String>,            // JSON "type", e.g. "doc", "paragraph", "text"
    pub attrs:     Option<Map<String, Value>>,// node attributes
    pub content:   Option<Vec<Node>>,         // child nodes
    pub marks:     Option<Vec<Mark>>,         // marks (bold, italic, link, …)
    pub text:      Option<String>,            // text payload (text nodes)
    pub extra:     Map<String, Value>,        // any unknown top-level fields, preserved
}

pub struct Mark {
    pub mark_type: String,                    // JSON "type", e.g. "bold"
    pub attrs:     Option<Map<String, Value>>,// mark attributes (e.g. link href)
    pub extra:     Map<String, Value>,        // unknown fields, preserved
}
```

`Map`/`Value` are re-exported from `serde_json`. The crate is built with the
`preserve_order` feature so attribute key order survives a roundtrip.

**Why everything is `Option`** — to faithfully distinguish *missing* from
*empty*. `content: None` serializes to no `content` key; `content: Some(vec![])`
serializes to `"content": []`. Unknown node types (custom Tiptap extensions) and
unknown fields land in `extra` and roundtrip untouched.

`Document` is a thin owning wrapper around the root `Node` and **derefs to it**,
so every `Node` method below is also callable directly on a `Document`.

---

## Parsing & serializing

```rust
use tiptap_rusty_parser::Document;
use serde_json::json;

// From a JSON string
let doc = Document::from_json_str(r#"{"type":"doc","content":[]}"#)?;

// From a serde_json::Value
let doc = Document::from_value(json!({ "type": "doc", "content": [] }))?;

// From any reader (file, socket, …)
let file = std::fs::File::open("doc.json")?;
let doc = Document::from_reader(file)?;

// Serialize
let compact = doc.to_json_str()?;       // String, compact
let pretty  = doc.to_string_pretty()?;  // String, indented
let value   = doc.to_value()?;          // serde_json::Value
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

Roundtrip is lossless — unknown node types, extra fields, and key order are all
preserved.

Access the root node explicitly when needed: `doc.root()`, `doc.root_mut()`,
`doc.into_root()`. Wrap an existing node with `Document::new(node)` or
`node.into()`.

---

## Querying

All traversal is **depth-first pre-order** (a node is visited before its
children). Selection is done with predicate closures — no selector DSL to learn.

```rust
use tiptap_rusty_parser::{Document, Node};

let doc = Document::from_json_str(r#"{
  "type":"doc","content":[
    {"type":"paragraph","content":[
      {"type":"text","text":"a","marks":[{"type":"bold"}]},
      {"type":"text","text":"b"}
    ]}
  ]}"#)?;

// First match (incl. the node itself)
let first_para: Option<&Node> = doc.find(|n| n.node_type.as_deref() == Some("paragraph"));

// All matches
let texts: Vec<&Node> = doc.find_all(|n| n.node_type.as_deref() == Some("text"));
assert_eq!(texts.len(), 2);

// Predicate can inspect anything: marks, attrs, text…
let bold = doc.find(|n| n.has_mark("bold")).unwrap();
assert_eq!(bold.get_text(), Some("a"));

// Lazy iterator over self + all descendants
let count = doc.descendants().count();

// Visit every node
let mut n = 0;
doc.walk(&mut |_| n += 1);
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

Mutable variants return `&mut` access:

```rust
# use tiptap_rusty_parser::{Document, Node};
# let mut doc = Document::from_json_str(r#"{"type":"doc","content":[{"type":"text","text":"x"}]}"#)?;
// Single mutable match
if let Some(node) = doc.find_mut(|n| n.node_type.as_deref() == Some("text")) {
    node.set_text("changed");
}

// All mutable matches (predicate passed by &mut)
let mut is_text = |n: &Node| n.node_type.as_deref() == Some("text");
for node in doc.root_mut().find_all_mut(&mut is_text) {
    node.add_mark(tiptap_rusty_parser::Mark::new("italic"));
}

// In-place visit
doc.walk_mut(&mut |n| { /* edit n */ });
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

| Method | Signature | Returns |
|--------|-----------|---------|
| `find` | `find(\|&Node\| -> bool)` | `Option<&Node>` |
| `find_mut` | `find_mut(\|&Node\| -> bool)` | `Option<&mut Node>` |
| `find_all` | `find_all(\|&Node\| -> bool)` | `Vec<&Node>` |
| `find_all_mut` | `find_all_mut(&mut \|&Node\| -> bool)` | `Vec<&mut Node>` |
| `walk` | `walk(&mut \|&Node\|)` | `()` |
| `walk_mut` | `walk_mut(&mut \|&mut Node\|)` | `()` |
| `descendants` | `descendants()` | `impl Iterator<Item = &Node>` |

---

## Selectors

Convenience wrappers over the closure API for the common cases — no closure to
write, and a friendlier surface for future CLI/FFI layers.

```rust
use tiptap_rusty_parser::Document;

let doc = Document::from_json_str(r#"{
  "type":"doc","content":[
    {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"Title"}]},
    {"type":"paragraph","content":[{"type":"text","text":"a","marks":[{"type":"bold"}]}]}
  ]}"#)?;

doc.by_type("paragraph");        // -> Vec<&Node>
doc.first_by_type("heading");    // -> Option<&Node>
doc.by_mark("bold");             // -> Vec<&Node> (nodes carrying the mark)
doc.by_attr("level", 1);         // -> Vec<&Node> (attr equals value)

// mutable
# let mut doc = doc;
for n in doc.root_mut().by_type_mut("paragraph") {
    n.set_attr("touched", true);
}
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

| Method | Returns |
|--------|---------|
| `by_type(t)` / `first_by_type(t)` / `by_type_mut(t)` | `Vec<&Node>` / `Option<&Node>` / `Vec<&mut Node>` |
| `by_mark(mark_type)` | `Vec<&Node>` |
| `by_attr(key, value)` | `Vec<&Node>` |

---

## Node paths

Address nodes by **index path** — a slice of child indices, root = `&[]`. In a
`doc → paragraph → text` tree the text node is at `&[0, 0]`. There are no parent
pointers; parent/sibling navigation is just path slicing.

```rust
use tiptap_rusty_parser::Document;

let mut doc = Document::from_json_str(r#"{
  "type":"doc","content":[
    {"type":"paragraph","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}
  ]}"#)?;

doc.node_at(&[0, 1]);                 // -> Option<&Node>  (the "b" text node)
doc.node_at_mut(&[0, 1]).unwrap().set_text("B");

let p = doc.path_to(|n| n.get_text() == Some("B")).unwrap(); // -> vec![0, 1]
let parent = doc.node_at(&p[..p.len() - 1]).unwrap();        // its paragraph

doc.paths_to(|n| n.node_type.as_deref() == Some("text"));    // every text location
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

| Method | Returns |
|--------|---------|
| `node_at(path)` / `node_at_mut(path)` | `Option<&Node>` / `Option<&mut Node>` |
| `path_to(pred)` | `Option<Vec<usize>>` (first match, pre-order) |
| `paths_to(pred)` | `Vec<Vec<usize>>` (all matches) |

---

## Mutating

Mutation is **in place** on a `&mut Node` / `&mut Document` — no copies, no
rebuild. Container fields auto-collapse to `None` when they become empty (e.g.
removing the last mark sets `marks` back to `None`), keeping output clean.

### Marks

```rust
use tiptap_rusty_parser::{Mark, Node};

let mut t = Node::text("hello");

t.add_mark(Mark::new("bold"));              // -> true (added)
t.add_mark(Mark::new("bold"));              // -> false (already present; deduped)
t.has_mark("bold");                          // -> true
t.get_mark("bold");                          // -> Option<&Mark>

t.toggle_mark(Mark::new("italic"));         // add if absent, remove if present
t.set_mark_attr("link", "href", "https://tiptap.dev"); // set attr on an existing mark
t.remove_mark("bold");                       // -> usize (count removed)
t.clear_marks();                             // drop all marks
```

### Attributes

```rust
use tiptap_rusty_parser::Node;
use serde_json::json;

let mut h = Node::element("heading");

h.set_attr("level", 2);          // -> previous value, if any
h.attr("level");                 // -> Option<&Value>  => Some(&json!(2))
h.attrs_mut().insert("class".into(), json!("title")); // raw map access
h.remove_attr("level");          // -> Option<Value>
```

### Children

```rust
use tiptap_rusty_parser::Node;

let mut p = Node::element("paragraph");

p.push_child(Node::text("a"));
p.push_child(Node::text("c"));
p.insert_child(1, Node::text("b"));      // index clamped to len

p.child_count();                          // -> 3
p.child(1);                               // -> Option<&Node>
p.child_mut(1);                           // -> Option<&mut Node>
p.children();                             // -> &[Node]
p.children_mut();                         // -> &mut Vec<Node> (creates if absent)

p.replace_child(0, Node::text("A"));      // -> Option<Node> (old)
p.remove_child(2);                        // -> Option<Node> (removed)
p.retain_children(|c| c.get_text() != Some("A")); // filter in place
p.clear_children();                       // remove all
```

### Text

```rust
# use tiptap_rusty_parser::Node;
let mut t = Node::text("old");
t.get_text();          // -> Some("old")
t.set_text("new");
```

### Bulk transforms

`replace_all` walks the whole subtree, applying a mutation to every node that
matches a predicate, and returns how many were changed.

```rust
use tiptap_rusty_parser::{Document, Mark};

let mut doc = Document::from_json_str(r#"{
  "type":"doc","content":[
    {"type":"paragraph","content":[{"type":"text","text":"x"}]},
    {"type":"paragraph","content":[{"type":"text","text":"y"}]}
  ]}"#)?;

let changed = doc.replace_all(
    |n| n.node_type.as_deref() == Some("text"),
    |n| { n.add_mark(Mark::new("bold")); },
);
assert_eq!(changed, 2);
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

---

## Text extraction

```rust
use tiptap_rusty_parser::Document;

let doc = Document::from_json_str(r#"{
  "type":"doc","content":[
    {"type":"paragraph","content":[{"type":"text","text":"Hello "},{"type":"text","text":"world"}]},
    {"type":"paragraph","content":[{"type":"text","text":"second line"}]}
  ]}"#)?;

doc.text_content();                       // "Hello worldsecond line"  (ProseMirror semantics)
doc.text_content_with_separator("\n\n");  // "Hello world\n\nsecond line"
doc.char_count();                         // Unicode scalar count of all text
doc.word_count();                         // 3  (Unicode word segmentation, block-aware)
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

`text_content` concatenates all descendant text with no separators (matches
ProseMirror's `node.textContent`). `text_content_with_separator(sep)` inserts
`sep` between adjacent block-level siblings (a node with `content` that isn't a
`text` node), so words don't merge across blocks. `word_count` uses
[`unicode-segmentation`](https://crates.io/crates/unicode-segmentation), so CJK
and complex scripts count correctly.

| Method | Returns |
|--------|---------|
| `text_content()` | `String` |
| `text_content_with_separator(sep)` | `String` |
| `char_count()` | `usize` |
| `word_count()` | `usize` |

---

## Schema validation

The crate is schema-*agnostic* by default — validation is **opt-in**. A `Schema`
is an allow-list of node types, marks, attributes, and child types. `validate`
collects **every** problem in one pass (empty result = valid); each `Violation`
carries the offending node's index path (see [Node paths](#node-paths)).

```rust
use tiptap_rusty_parser::{Document, Schema, NodeSpec, MarkSpec};

let schema = Schema::new()
    .node("doc", NodeSpec::new().content(["paragraph", "heading"]))
    .node("paragraph", NodeSpec::new().content(["text"]))
    .node("heading", NodeSpec::new().content(["text"])
        .attrs(["level"]).required_attrs(["level"]))
    .node("text", NodeSpec::new().marks(["bold", "italic"])) // marks live on text nodes
    .mark("bold", MarkSpec::new())
    .mark("italic", MarkSpec::new());

let doc = Document::from_json_str(
    r#"{"type":"doc","content":[{"type":"heading"}]}"#,
)?;

assert!(!doc.is_valid(&schema));
for v in doc.validate(&schema) {
    println!("{v}"); // e.g. `at [0]: missing required attribute `level``
}
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

Unset rules mean "anything goes": `NodeSpec::new()` allows any attrs/marks/children;
`content`/`marks`/`attrs` restrict only once set. `required_attrs` is always
enforced.

A schema can also be loaded from JSON:

```rust
use tiptap_rusty_parser::Schema;

let schema = Schema::from_json_str(r#"{
  "nodes": {
    "doc":       { "content": ["paragraph"] },
    "paragraph": { "content": ["text"] },
    "text":      { "marks": ["bold"] }
  },
  "marks": { "bold": {}, "link": { "attrs": ["href"], "required_attrs": ["href"] } }
}"#)?;
# let _ = schema;
# Ok::<(), tiptap_rusty_parser::ParseError>(())
```

`Violation::kind` is a `ViolationKind`: `MissingNodeType`, `UnknownNodeType`,
`DisallowedChild`, `UnknownMark`, `DisallowedMark`, `MissingAttr`, `UnknownAttr`.

| Method | Returns |
|--------|---------|
| `validate(&schema)` | `Vec<Violation>` (empty = valid) |
| `is_valid(&schema)` | `bool` |

---

## Building nodes

Constructors plus consuming `with_*` builder methods for fluent assembly.

```rust
use tiptap_rusty_parser::{doc, Mark, Node};

// Leaf constructors
let plain  = Node::text("hi");
let marked = Node::text_with_marks("bold!", [Mark::new("bold")]);

// Element builder
let para = Node::element("paragraph")
    .with_attr("textAlign", "center")
    .with_mark(Mark::new("bold"))
    .with_text("hello")                  // adds a text child
    .with_child(Node::text(" world"));   // adds an arbitrary child

// Mark builder
let link = Mark::new("link").attr("href", "https://tiptap.dev");

// doc(..) helper for the root
let document = doc([
    Node::element("heading").with_attr("level", 1).with_text("Title"),
    para,
]);
```

| Constructor / builder | Purpose |
|-----------------------|---------|
| `Node::element(type)` | new element node of `type` |
| `Node::text(s)` | new `text` node |
| `Node::text_with_marks(s, marks)` | text node with marks |
| `doc(children)` | a `doc` root node |
| `Mark::new(type)` / `.attr(k, v)` | construct a mark |
| `.with_attr(k, v)` | set an attr (chaining) |
| `.with_child(node)` / `.with_children(iter)` | append child/children |
| `.with_text(s)` | append a text child |
| `.with_mark(mark)` | add a mark |

---

## JavaScript / WASM

The crate ships WASM bindings on npm for browser/bundler apps:

```bash
npm install tiptap-rusty-parser
```

```js
import { TiptapDoc } from "tiptap-rusty-parser";

const doc = TiptapDoc.fromJSON({
  type: "doc",
  content: [{ type: "heading", content: [{ type: "text", text: "Title" }] }],
});

doc.textContent();               // "Title"
const [headingPath] = doc.pathsByType("heading"); // [0]
doc.setAttr(headingPath, "level", 1);
doc.addMark([0, 0], "bold");
doc.isValid({ nodes: { doc: { content: ["paragraph"] } } }); // false
const json = doc.toJSON();
```

An opaque `TiptapDoc` handle keeps the tree in WASM; queries return cloned
nodes or `number[]` index paths, and mutation is path-addressed. Full method
list in [`bindings/wasm/README.md`](bindings/wasm/README.md). Built for the
`bundler` target.

---

## Error handling

Parsing/serialization returns `Result<T, ParseError>`:

```rust
pub enum ParseError {
    Json(serde_json::Error), // invalid JSON / shape mismatch
    Io(std::io::Error),      // reader failure (from_reader)
}
```

`ParseError` implements `std::error::Error` (via `thiserror`) and `From` for both
underlying errors, so `?` works directly. A `Result<T>` alias is also exported.

---

## Performance

Borrow-first API, stack-based descendant iteration (no recursion blowup on deep
docs), `serde_json` for (de)serialization, and a release profile with
`lto = true` / `codegen-units = 1`.

Indicative criterion baselines on a synthetic doc of **500 paragraphs × 20 bold
text spans** (~10k text nodes, ~10.5k nodes total):

| Operation | Time |
|-----------|------|
| `parse` (from JSON string) | ~14 ms |
| `serialize` (to JSON string) | ~1.0 ms |
| `walk` (count all nodes) | ~29 µs |
| `find_all` (all text nodes) | ~108 µs |
| `replace_all` (add a mark to every text node) | ~5.0 ms |

Run `cargo bench` to reproduce on your hardware.

---

## Development

```sh
cargo test     # unit + integration + doctests
cargo clippy --all-targets -- -D warnings
cargo bench    # criterion baselines
```

---

## License

MIT
