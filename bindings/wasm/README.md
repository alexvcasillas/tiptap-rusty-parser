# @tiptap-studio/rusty-parser

Parse, query, edit, diff and validate [Tiptap](https://tiptap.dev) / ProseMirror
`JSONContent` from JavaScript. This is the Rust crate
[`tiptap-rusty-parser`](https://crates.io/crates/tiptap-rusty-parser) compiled to
WebAssembly; the document stays in WebAssembly memory and only crosses to JavaScript at
the boundary.

**Documentation:** [https://tiptap-studio.alexvcasillas.dev/docs/javascript](https://tiptap-studio.alexvcasillas.dev/docs/javascript)

```sh
npm install @tiptap-studio/rusty-parser
```

The package is built for bundlers (`wasm-pack --target bundler`) and imports its
`.wasm` file as a module: webpack 5 needs `experiments.asyncWebAssembly`, Vite needs
`vite-plugin-wasm`. See [bundler setup](https://tiptap-studio.alexvcasillas.dev/docs/javascript#bundler-setup).

## Example

```ts
import { TiptapDoc } from "@tiptap-studio/rusty-parser";

const doc = TiptapDoc.fromJSON({
  type: "doc",
  content: [
    { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "Title" }] },
    { type: "paragraph", content: [{ type: "text", text: "Hello world" }] },
  ],
});

doc.textContent(); // "TitleHello world"
doc.wordCount(); // 3

// Find nodes by index path, then edit by path.
const [headingPath] = doc.pathsByType("heading"); // [0]
doc.setAttr(headingPath, "level", 1);
doc.addMark([0, 0], "bold", undefined); // true

const json = doc.toJSON(); // a plain JSONContent object
doc.free();
```

## What's in it

- [`TiptapDoc`](https://tiptap-studio.alexvcasillas.dev/docs/javascript/tiptap-doc): read, query, edit and serialize a document.
- [Positions](https://tiptap-studio.alexvcasillas.dev/docs/javascript/positions): ProseMirror positions, position-addressed edits and position mapping.
- [Diffing](https://tiptap-studio.alexvcasillas.dev/docs/javascript/diffing) and [change algebra](https://tiptap-studio.alexvcasillas.dev/docs/javascript/change-algebra).
- [Schema validation](https://tiptap-studio.alexvcasillas.dev/docs/javascript/schema) and [transactions with undo](https://tiptap-studio.alexvcasillas.dev/docs/javascript/transactions).
- [TypeScript](https://tiptap-studio.alexvcasillas.dev/docs/javascript/typescript): the generated types and how errors surface.

For integrating with the Tiptap AI Toolkit, see the [guide](https://tiptap-studio.alexvcasillas.dev/docs/guides/ai-toolkit).

## License

MIT
