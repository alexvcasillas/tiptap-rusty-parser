# Integrating with the Tiptap AI Toolkit

This crate is a **fast, schema-neutral engine** for the document operations
behind AI editing — flat ProseMirror positions, character-level diffing,
position-addressed apply/invert, and position mapping — usable from a browser
(via WASM) or a Rust server. It pairs with Tiptap's
[**AI Toolkit**](https://tiptap.dev/docs/content-ai/capabilities/ai-toolkit/overview)
so the operations an LLM emits run at native speed and produce **invertible**
patches (accept / reject), with selections that stay anchored across edits.

The AI Toolkit itself is proprietary; **no Toolkit code or schema lives here.**
This crate exposes stable, neutral data shapes, and the last-mile glue
(`tiptapEdit` ⇄ `PosEdit`, Tiptap Shorthand ⇄ serializer) is a thin translation
layer you write privately against these APIs.

---

## The neutral surface

Everything below is plain data (serde / JSON) at the boundary — the same shapes
in Rust and in the generated TypeScript (`.d.ts`).

| Capability | WASM (JS) | Rust |
|---|---|---|
| Resolve a flat position | `doc.resolve(pos)` → `ResolvedPos` | `Node::resolve` |
| Position ⇄ block/inline | `doc.posToInline(pos)`, `doc.inlineToPos(block, p)` | `Node::pos_to_inline` / `inline_to_pos` |
| Node boundaries / size | `doc.posBefore(path)`, `doc.posAfter(path)`, `doc.contentSize()` | `Node::pos_before` / `pos_after` / `content_size` |
| Granular diff (doc ↔ doc) | `a.diffWith(b, { text: "inline" \| "block" \| { smart: { replaceThreshold } } })` → `Change[]` | `Node::diff_with` |
| Position-addressed edit | `doc.applyPosEdits(edits)` → `Change[]` | `Node::apply_pos_edits` |
| Undo / reject a patch | `doc.invert(changes)` | `Node::invert` |
| Map a position/range through edits | `mapPosition(pos, edits, assoc?)`, `mapPositionRange(range, edits, assoc?)` | `PosMap::from_pos_edits(..).map(..)` |
| Schema-guard proposed content | `validateNode(schema, node)` → `Violation[]` | `Node::validate` |

`PosEdit` (the neutral op) is a tagged union:

```ts
type PosEdit =
  | { type: "insert"; pos: number; content: PosContent }
  | { type: "delete"; from: number; to: number }
  | { type: "replace"; from: number; to: number; content: PosContent }
  | { type: "addMark"; from: number; to: number; mark: Mark }
  | { type: "removeMark"; from: number; to: number; markType: string }
  | { type: "setBlockAttrs"; pos: number; attrs: Record<string, unknown> };

type PosContent =
  | { type: "text"; text: string; marks?: Mark[] }
  | { type: "nodes"; nodes: JSONContent[] };
```

Positions are **flat ProseMirror integer positions** — exactly what the Toolkit's
`tiptapEdit` operations and the editor's `state.selection` use.

---

## Worked flow: AI proposes a new document → review → apply

The Toolkit can have a model return a full proposed document. Diff it natively to
get a minimal, character-level change set you can present as suggestions, then
apply or reject:

```js
import { TiptapDoc } from "tiptap-rusty-parser";

const current  = TiptapDoc.fromJSON(editor.getJSON());
const proposed = TiptapDoc.fromJSON(aiProposedJSON);

// Native smart diff: character-level where edits are small, whole-node where
// they're sweeping. Returns a Change[] (each tagged with an `op`).
const changes = current.diffWith(proposed, {
  text: { smart: { replaceThreshold: 0.5 } },
});

// Apply to get the new doc…
current.applyChanges(changes);
editor.commands.setContent(current.toJSON());

// …or reject: invert relative to the pre-image and apply the undo.
const undo = preImage.invert(changes);
```

## Worked flow: position-addressed tool calls

When the model emits position-addressed operations (the `tiptapEdit` shape),
translate them to `PosEdit[]` and apply in one batch — you get the invertible
patch back for free:

```js
const doc = TiptapDoc.fromJSON(editor.getJSON());

// Your thin adapter maps the Toolkit's tiptapEdit ops to neutral PosEdits.
const edits = toolCall.operations.map(toPosEdit);

const patch = doc.applyPosEdits(edits); // Change[] — reproduces & inverts
editor.commands.setContent(doc.toJSON());

// Keep the user's selection anchored across the AI edit:
const sel = editor.state.selection;
const from = mapPosition(sel.from, edits, "left");
const to   = mapPosition(sel.to,   edits, "right");
```

Batches must be **disjoint** (overlapping spans error); edits apply
highest-position-first so the un-rebased positions stay valid.

## Schema-guard

Before inserting AI-proposed content, validate it against your editor schema and
reject anything that wouldn't fit — no half-applied mutations:

```js
const violations = validateNode(schema, proposedNode);
if (violations.length) reject(violations);
```

---

## Where it plugs into the Toolkit

Against the Toolkit's **documented public extension points**:

- **`tiptapEditHooks.beforeOperation`** — intercept each operation, run it through
  `applyPosEdits` on a mirrored `TiptapDoc` to pre-compute the invertible patch
  and validate proposed content (`validateNode`) before it touches the editor.
- **The diff utility / Review Changes** — replace the doc↔doc diff with
  `diffWith` (smart / inline / block) to produce the suggestion set.
- **Read-the-document / serialize** — `toJSON` / `fromJSON` and `textContent`
  feed the model; `resolve` / `posToInline` translate between the model's flat
  positions and your block-addressed logic.

The proprietary `tiptapEdit` ⇄ `PosEdit` and Tiptap Shorthand ⇄ serializer
mappings are a ~thin layer you keep private; this crate provides the stable
target shapes.

## Server-side (no browser)

The crate is a normal Rust dependency, so a **Server AI Toolkit** backend (or any
non-TS service) can run the same primitives natively — no Node/WASM needed:

```rust
use tiptap_rusty_parser::{Document, PosEdit, PosContent};

let mut doc = Document::from_json_str(req_body)?;
let patch = doc.root_mut().apply_pos_edits(&edits)?; // invertible Change list
let response = doc.to_json_str()?;
```

---

## Performance

Native throughput on the hot paths (see the crate README's full table; reproduce
with `cargo bench`):

| Operation | Time |
|---|---|
| `diffWith(inline)` — one-word edit in a ~10k-char paragraph | ~46 µs |
| `applyPosEdits` — 50 disjoint replaces across a 500-block doc | ~33 ms |
| `PosMap::map` — every position in a ~10k-unit doc through a 50-step map | ~1.8 ms |
| `resolve` — flat position → `ResolvedPos`, mid-doc | ~256 µs |

A runnable end-to-end demo lives at
[`bindings/wasm/examples/ai-toolkit-integration.mjs`](bindings/wasm/examples/ai-toolkit-integration.mjs).
