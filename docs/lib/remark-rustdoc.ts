import type { Code, Root } from "mdast";

/**
 * Rust fences on these pages are compiled as doctests by the `docs-check`
 * crate, so they may carry rustdoc's hidden setup lines (`# use …`, `#`).
 * Strip them on the site, as rustdoc does, and unescape `##`.
 */
export function remarkRustdoc() {
  return (tree: Root) => {
    walk(tree, (node) => {
      if (node.lang !== "rust") return;
      node.value = node.value
        .split("\n")
        .filter((line) => {
          const t = line.trimStart();
          return !(t === "#" || t.startsWith("# "));
        })
        .map((line) => line.replace(/^(\s*)##/, "$1#"))
        .join("\n");
    });
  };
}

function walk(node: unknown, visit: (code: Code) => void) {
  const n = node as { type?: string; children?: unknown[] };
  if (n.type === "code") visit(n as Code);
  for (const child of n.children ?? []) walk(child, visit);
}
