# tiptap-rusty-parser (WASM)

WebAssembly bindings for [`tiptap-rusty-parser`](https://crates.io/crates/tiptap-rusty-parser) —
a fast, schema-agnostic parser/manipulator for Tiptap/ProseMirror `JSONContent`,
written in Rust and compiled to WASM.

```bash
npm install tiptap-rusty-parser
```

Built for the `bundler` target (Vite, webpack, Next, …).

## Usage

```js
import { TiptapDoc } from "tiptap-rusty-parser";

const doc = TiptapDoc.fromJSON({
  type: "doc",
  content: [
    { type: "heading", content: [{ type: "text", text: "Title" }] },
    { type: "paragraph", content: [{ type: "text", text: "Hello world" }] },
  ],
});

doc.textContent();            // "TitleHello world"
doc.wordCount();              // 3

// Locate nodes by index path, then mutate by path
const [headingPath] = doc.pathsByType("heading"); // e.g. [0]
doc.setAttr(headingPath, "level", 1);
doc.addMark([0, 0], "bold");                       // bold the heading text

// Validate (opt-in) against an allow-list schema
const schema = { nodes: { doc: { content: ["paragraph"] } } };
doc.isValid(schema);          // false (heading not allowed under doc)
doc.validate(schema);         // [{ path: [...], kind: ... }, ...]

const out = doc.toJSON();     // plain JSONContent object
```

## API (`TiptapDoc`)

The document tree stays owned by WASM; it's only serialized to JS at the
boundary. `path` arguments are plain `number[]` index paths (root = `[]`).

| Group | Methods |
|-------|---------|
| Lifecycle | `TiptapDoc.fromJSON(obj)`, `TiptapDoc.fromJSONString(str)`, `toJSON()`, `toJSONString()` |
| Read query | `byType(t)`, `firstByType(t)`, `byMark(t)`, `byAttr(key, value)` → node object(s) |
| Locate | `pathsByType(t)`, `pathsByMark(t)`, `pathsByAttr(key, value)` → `number[][]`; `nodeAt(path)`, `childCount(path)` |
| Mutate (by path) | `setAttr`, `removeAttr`, `setText`, `addMark`, `removeMark`, `pushChild`, `insertChild`, `removeChild` |
| Normalize | `normalize()`, `normalizeWith(options)` (merge adjacent text, drop empties) |
| Range edit (one block) | `insertText(path, pos, text, marks?)`, `deleteRange(path, range)`, `replaceRange(path, range, text, marks?)`, `addMarkRange`/`removeMarkRange`/`toggleMarkRange(path, range, …)` |
| Block edit (structure) | `setBlockType(path, type, attrs?)`, `splitBlock(path, at, depth)`, `splitBlockAt(path, pos, depth)`, `joinBlocks(parent, index)`, `wrap(path, type, attrs?)`, `wrapRange(parentPath, start, end, type, attrs?)`, `lift(path)` |
| Text | `textContent()`, `charCount()`, `wordCount()` |
| Validate | `validate(schema)`, `isValid(schema)` |
| Diff | `diff(other)` → `Change[]`; `applyChanges(changes)`; `invert(changes)` → reverse `Change[]` (undo) |
| Change algebra | `compose(a, b)`, `compact(changes)`, `mapPath(path, changes)` → `number[] \| null` (module functions) |
| Render | `toHTML()` → HTML string; `toHTMLWith(options)` (node/mark tag maps, unknown-policy, `selfClosing`, `spreadAttrs`, `textAlign`) |

Methods throw on malformed input or a missing path target.

### TypeScript

The package ships real types for the plain objects that cross the boundary —
`JSONContent`, `Mark`, `Change` (a tagged union), `Position`, `Range`,
`Violation`, `NormalizeOptions` — so you can annotate the values you build/read.

### Transactions / undo

For an invertible patch around a sequence of edits (including the block ops,
which mutate in place), snapshot first and diff:

```js
const before = TiptapDoc.fromJSON(doc.toJSON());
doc.splitBlock([0], 1, 0);
doc.wrap([1], "blockquote");
const patch = before.diff(doc);     // Change[] describing the edits
const undo  = before.invert(patch); // reverse patch
// later: doc.applyChanges(undo) to roll back
```

## License

MIT
