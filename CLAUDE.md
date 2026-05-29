# CLAUDE.md

## Interaction rules
- Be extremely concise; sacrifice grammar for concision.
- Always follow project info below.
- Commits: Conventional Commits spec.
- Commits: ONE LINERS.
- Performance IS A MUST.
- End each plan w/ concise list of unresolved questions (if any).
- Sacrifice grammar for concision.

## Project
tiptap-rusty-parser: Rust crate. Parse/walk/query/mutate Tiptap JSONContent (ProseMirror) docs.
- Schema-agnostic: accept any node/mark types, preserve unknown fields, faithful roundtrip.
- Query: predicate closures (find/find_all/walk).
- Mutation: in-place mutable tree (&mut).
- Pure lib, no bindings v1.
- Perf-first: avoid clones, borrow over copy, bench w/ criterion.
