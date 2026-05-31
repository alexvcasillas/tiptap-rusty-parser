// Runnable demo: pairing tiptap-rusty-parser with the Tiptap AI Toolkit's
// document operations. Shows the four native primitives an AI editing flow
// needs — smart diff, position-addressed apply, invertible reject, and
// selection mapping — plus the schema-guard.
//
//   npm i tiptap-rusty-parser
//   node bindings/wasm/examples/ai-toolkit-integration.mjs
//
// (Or, from a local build: `wasm-pack build bindings/wasm --target nodejs`
//  then import from the generated `pkg`.)

import {
  TiptapDoc,
  mapPosition,
  validateNode,
} from "tiptap-rusty-parser";

const assert = (cond, msg) => {
  if (!cond) throw new Error("assertion failed: " + msg);
};

// A tiny document: doc > paragraph("hello world").
const json = {
  type: "doc",
  content: [
    { type: "paragraph", content: [{ type: "text", text: "hello world" }] },
  ],
};

// ---------------------------------------------------------------------------
// 1. The model returns a full proposed document → native smart diff → suggestions
// ---------------------------------------------------------------------------
{
  const current = TiptapDoc.fromJSON(json);
  const proposed = TiptapDoc.fromJSON({
    type: "doc",
    content: [
      { type: "paragraph", content: [{ type: "text", text: "hello there" }] },
    ],
  });

  // Character-level where edits are small; whole-node past the threshold.
  const changes = current.diffWith(proposed, {
    text: { smart: { replaceThreshold: 0.5 } },
  });
  console.log("smart-diff changes:", JSON.stringify(changes));
  assert(changes.length > 0, "diff should be non-empty");

  current.applyChanges(changes);
  assert(current.textContent().includes("hello there"), "diff applied");
}

// ---------------------------------------------------------------------------
// 2. Position-addressed tool call (the `tiptapEdit` shape) → apply + invert
// ---------------------------------------------------------------------------
{
  const doc = TiptapDoc.fromJSON(json);
  const preImage = TiptapDoc.fromJSON(json); // snapshot for reject/undo

  // Replace "world" (flat [7, 12): 1 open token + scalars 6..11) with "there".
  const edits = [
    { type: "replace", from: 7, to: 12, content: { type: "text", text: "there" } },
  ];

  const patch = doc.applyPosEdits(edits); // invertible Change[]
  assert(doc.textContent() === "hello there", "pos-edit applied");

  // Reject: invert the patch relative to the pre-image and apply the undo.
  const undo = preImage.invert(patch);
  doc.applyChanges(undo);
  assert(doc.textContent() === "hello world", "reject restored original");
  console.log("apply + invert round-trips ✔");
}

// ---------------------------------------------------------------------------
// 3. Keep the user's selection anchored across an AI edit
// ---------------------------------------------------------------------------
{
  // A cursor at the end of "hello world" (flat pos 12).
  const cursor = 12;
  // The AI inserts "big " before "world" (flat pos 7).
  const edits = [
    { type: "insert", pos: 7, content: { type: "text", text: "big " } },
  ];
  const moved = mapPosition(cursor, edits, "right");
  assert(moved === 16, `cursor should shift +4, got ${moved}`);
  console.log("selection mapped", cursor, "→", moved);
}

// ---------------------------------------------------------------------------
// 4. Schema-guard: reject AI content that wouldn't fit the editor schema
// ---------------------------------------------------------------------------
{
  const schema = {
    nodes: { paragraph: { content: ["text"] }, text: {} },
  };
  const good = validateNode(schema, {
    type: "paragraph",
    content: [{ type: "text", text: "ok" }],
  });
  const bad = validateNode(schema, {
    type: "paragraph",
    content: [{ type: "widget" }],
  });
  assert(good.length === 0, "valid content passes the guard");
  assert(bad.length > 0, "invalid content is rejected");
  console.log("schema-guard: good=0 violations, bad=" + bad.length);
}

console.log("\nAll integration demos passed ✅");
