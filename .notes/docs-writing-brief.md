# Brief for writing docs pages (docs site, PR 2)

Repo: /Users/alexvcasillas/code/libs/tiptap-rusty-parser (GitHub alexvcasillas/tiptap-studio).
The site is Fumadocs in `docs/`; pages are MDX in `docs/content/docs/<section>/`.
Read `.notes/docs-site.md` for the plan and `docs/content/docs/studio/*.mdx` for the
finished Studio section, which sets the tone and format.

## Accuracy (non-negotiable)

- Never write an API, type, method, argument, return shape, default, error variant or
  behaviour you have not read in the source. Source of truth: `parser/src`,
  `markdown/src`, `bindings/wasm/src`, plus their `tests/` for real usage.
- `parser/README.md`, `markdown/README.md`, `bindings/wasm/README.md`,
  `parser/INTEGRATION.md` and `parser/MIGRATION.md` are useful starting points but are
  NOT authoritative. Known stale claims in parser/README.md: install line says `"0.1"`
  (crate is 0.6.1), "no selector DSL" (the `Query` language exists), "rebasing out of
  scope" (`rebase_change` and `unmap_path` exist). Re-check everything you take from them.
- Current versions: tiptap-rusty-parser 0.6.1, tiptap-rusty-markdown 0.1.1, npm
  `@tiptap-studio/rusty-parser` 0.6.1 (follows the crate). All of `main` is released.
- If something is unclear in the source, leave it out rather than guess, and list it in
  your final report.

## Compiled Rust examples

Every ```rust fence on the site is compiled as a doctest by the `docs-check` crate.

- Register each page that contains Rust code in YOUR section module only (you will be
  told which): `pages! { ParserQuerying => "parser/querying.mdx", … }` in
  `docs-check/src/<module>.rs`. One unique CamelCase name per page.
- Blocks are doctests: write them like rustdoc examples. Use hidden lines for setup and
  for `?` support, exactly as in rustdoc:
  ```rust
  # use tiptap_rusty_parser::Document;
  let doc = Document::from_json_str(r#"{"type":"doc","content":[]}"#)?;
  assert_eq!(doc.root().child_count(), 0);
  # Ok::<(), tiptap_rusty_parser::ParseError>(())
  ```
  Hidden `# ` lines are stripped on the site. Prefer `assert_eq!` showing the result
  over comments, so the example proves itself.
- Available crates in doctests: `tiptap_rusty_parser`, `tiptap_rusty_markdown`,
  `serde_json`. Nothing else.
- Every code fence must have a language: `rust`, `toml`, `json`, `sh`, `ts`, `js`,
  `text`. An unlabelled fence is compiled as Rust and fails the guard test.
- A Rust block that genuinely cannot compile (a signature sketch) uses ```rust ignore.
  Keep these rare.
- Verify with your filter, e.g. `cargo test --doc -p docs-check -- parser_core::` and
  `cargo test -p docs-check --test coverage` (the coverage test may fail only because of
  another agent's in-progress pages; check its message names your files or not).
- Other agents are writing other sections at the same time and share the cargo target
  directory, so builds may wait on a lock. Do not edit files outside your scope.

## MDX format

- Frontmatter: `title` (short, sentence case) and `description` (one sentence).
- No `# H1` in the body (the title renders it). Use `##` and `###`.
- MDX: `{`, `}` and `<` in prose break the page. Put code, JSON and type names in
  backticks. Components available without imports: `Callout`, `Cards`, `Card`.
- Links between pages: absolute, e.g. `/docs/parser/querying`. API reference links go
  to docs.rs, e.g. `https://docs.rs/tiptap-rusty-parser/latest/tiptap_rusty_parser/struct.Query.html`.
- Each section has a `meta.json`: `{ "title": "…", "root": true, "pages": [ … ] }`.
  `"---Label---"` entries add sidebar separators. See `docs/content/docs/studio/meta.json`.

## Style

- Plain, direct, second person. Lead with what the reader can do, then how.
- No em-dashes anywhere. No parenthetical asides where a sentence works.
- Short sentences. One idea per sentence.
- No marketing words ("blazing", "powerful", "seamless").
- Tables for parallel reference material (methods, options, errors).
- Every page should be useful on its own: what it is, a minimal example, the details,
  the edge cases and errors.

## Done means

- Pages and `meta.json` written for your section, every Rust block compiles under
  your docs-check filter, every fence labelled.
- Do not run `pnpm build` or `pnpm dev`: the agents share `docs/.next`. The lead runs
  the site build once all sections are in.
- Final report: the pages written, anything you left out because the source was
  unclear, and any bug or inconsistency you found in the code or old READMEs.
- Do not commit or push.
