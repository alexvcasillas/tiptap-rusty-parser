# Docs site: spec and plan

Status: revision 2, approved 2026-10-24. PR 1 implemented on feat/docs-site.

## Goal

One documentation site for the whole repo (alexvcasillas/tiptap-studio): Tiptap
Studio (desktop app), `tiptap-rusty-parser`, `tiptap-rusty-markdown` and
`@tiptap-studio/rusty-parser` (WASM). The site is the **source of truth** for
long-form docs, and its Rust code is **compiled in CI**, so prose and examples
cannot drift silently. API reference stays on docs.rs.

## Decisions

- Fumadocs on Next.js in `docs/`, pnpm, own lockfile, outside the Cargo workspace.
- Vercel, default `*.vercel.app` subdomain; owner creates the project (root
  `docs/`) once it builds locally.
- Docs are the source of truth. READMEs become self-sufficient summaries (see
  "READMEs"), not copies.

## Stack

- Scaffold with `create-fumadocs-app` at its current release, then pin exact
  versions from the lockfile (expected: Next 16, fumadocs-core/ui/mdx 16,
  React >= 19.2). `packageManager: pnpm@<x>` and `engines.node >= 22` in
  `docs/package.json`; `.nvmrc` with 22.
- Search: Orama via the scaffold's server route (`app/api/search`), which runs
  on Vercel; verified against the production build. Static export not needed.
- Theme: dark default, accent `#7A52FF` (studio `theme.rs:58`), logo copied
  into `docs/public/` (no imports from outside `docs/`, so Turbopack and Vercel
  roots stay simple).
- Remark plugin: hide rustdoc hidden lines (`# ...` and bare `#`) in ```rust
  blocks on the site, so the same block is a full doctest and a clean example.
- `docs/.gitignore` (from the scaffold) ignores `.next/`, `.source`, `out/`.

## Compiled examples: `docs-check` crate

- New workspace member `docs-check/` (`publish = false`, `release = false` in
  release-plz) depending on the parser and markdown crates.
- Its `lib.rs` includes every MDX page that has Rust code under
  `#[cfg(doctest)]`: `#[doc = include_str!("../../docs/content/docs/parser/diffing.mdx")]`.
  rustdoc compiles every ```rust fence; JSX and frontmatter are plain text to it.
- Fence rules: every fence carries a language (unlabelled fences are Rust to
  rustdoc); non-compiling Rust uses ```rust ignore; JS/TS/shell fences are
  ignored by rustdoc.
- Runs in the existing `ci.yml` Rust job (`cargo test --doc -p docs-check`), so
  a page that breaks against the crate fails CI on any PR, docs or code.
- A guard test fails when an MDX file with ```rust fences is not listed in
  `lib.rs`, so new pages cannot skip compilation.
- `parser/MIGRATION.md` moves to `/docs/parser/migration` and is compiled the
  same way; the parser's own `include_str!("../MIGRATION.md")` doctest goes away.
- Markdown README doctests (`markdown/src/lib.rs:17`): the README keeps its
  examples and stays doctested (it is the crates.io page).

JS examples are not compiled in v1 (no harness exists for the WASM package);
every JS identifier is checked against `bindings/wasm/src/lib.rs`.

## Information architecture

```
/                         Landing: Studio showcase (screenshot slots), downloads, packages
/docs/studio/
  index                   What it is; install per OS: macOS dmg (Developer ID signed and
                          notarized in official releases; -unsigned builds need the
                          quarantine step), Windows zip (unsigned, SmartScreen), Linux
                          tarball (+ .desktop); SHA256SUMS; CLI: A [B] [--schema <file>|
                          starter-kit]
  tour                    Title bar, Inspect/Diff modes, toolbar, panes (cmd-1/cmd-2
                          collapse, resize), preview vs Source (cmd-3)
  inspect                 Tree, preview, inspector (attrs incl. typed editors, marks with
                          attrs, text), unknown node types, Copy JSON
  editing                 Insert chooser, wrap, lift, split, join, move, duplicate, delete,
                          copy/cut/paste, raw JSON edit (cmd-enter applies), undo/redo
  diff                    Open B…, change list, Take B, Dismiss/Restore (one toggle),
                          show dismissed, take all B in subtree, Apply all to A, Swap A/B,
                          sync scroll, only changed, saving (Save / Save as… suggests
                          <a>-merged.json)
  find                    Find box, query language, only matches, Enter / Shift-Enter
  schema                  StarterKit bundled, open/reload/unload/open another, Problems
                          drawer, F8 / Shift-F8, markers
  document-server         Profiles, token handling (keychain; opaque tokens opt-in; session
                          only when no keychain), JWT warnings, open as A/B, diff versions
  shortcuts               Hand-written table with context (Document / Diff / Find / JSON
                          editor) and platform variants; verified against an inventory of
                          KeyBinding::new plus input handlers (find.rs:398, inspector.rs:1569)
  files                   Open / drop / recent, unsaved guard, disk-change banner (Reload /
                          Keep mine / Dismiss), config files (layout.json, recent.json,
                          servers.json) and their per-OS locations as implemented
  troubleshooting         TIPTAP_STUDIO_PROFILE timings, unsigned build steps
/docs/parser/             Written from source + tests, not split from the README
  index, data-model, parsing (+ source maps), querying (closures, selectors), query-language,
  paths-and-positions (incl. PosMap, Assoc, scalar offsets, leaf rules), mutating,
  normalizing, range-and-block-editing, text, schema (+ content expressions), diffing
  (granularities, diff_text), transactions (failure semantics: Transform keeps prior
  steps, apply_inverting rolls back), position-editing (apply_pos_edits(_mapped)),
  change-algebra (compose, compact, map_path, unmap_path, rebase_change), html,
  building-nodes, errors (ParseError, ApplyError, RangeError, PosError, QueryError, …),
  performance, migration (0.5 → 0.6)
/docs/markdown/           index (install, quick start), import, export, options,
                          limitations, performance
/docs/javascript/         index (install, bundler setup, memory/disposal), TiptapDoc,
                          positions (contentSize, resolve, posBefore/After/InText,
                          posToInline, inlineToPos, applyPosEdits, mapPosition(Range)),
                          diffing (diff, diffWith), change-algebra (compose, compact,
                          mapPath), schema (validateNode), transactions, TypeScript
/docs/guides/ai-toolkit   from parser/INTEGRATION.md, checked against source
```

## Version policy

Single "latest", deployed from `main`. Pages describe released behaviour; anything
merged but not yet released is marked with an "Unreleased" callout naming the
package, removed when the release PR merges. Every package index page states the
version it documents. Versioned snapshots later, when a breaking release needs it.

## CI / deploy

- Rust doctests of the docs: in `ci.yml` (always runs), see `docs-check`.
- Site build: new workflow `.github/workflows/docs.yml`, `on.paths` = `docs/**` +
  the workflow file, `working-directory: docs`, pnpm via `pnpm/action-setup`,
  `pnpm install --frozen-lockfile`, lint (`next lint` or eslint as scaffolded),
  typecheck, `next build` from a clean checkout (no `.source`). Kept out of
  `ci.yml` because `on.paths` filters whole workflows. Not a required check.
- Vercel Git integration: previews per PR, production from `main`.

## READMEs (PR 3, after the site is live and links verified)

- Self-sufficient summaries, because crates.io, npm and GitHub render them and
  they ship inside the packages: what it is, install with the current version,
  one or two representative examples, key limitations, and links to the site.
- Fix the stale claims while doing so (install `"0.1"`, "no selector DSL",
  "rebasing out of scope").
- `parser/INTEGRATION.md` and `parser/MIGRATION.md` become one-line pointers to
  their site pages (keeps old links working).
- Root `README.md`: Studio showcase with screenshots (owner provides them).

## Delivery

1. PR 1: scaffold, pins, theme, search, remark plugin, `docs-check` crate with
   the guard test, `docs.yml`, IA skeleton, full Studio section.
2. PR 2: Parser, Markdown, JavaScript, Guides pages (all compiled via docs-check).
3. PR 3: READMEs, INTEGRATION/MIGRATION pointers, root README showcase.

## Risks

- rustdoc on MDX: JSX blocks are inert text, but an unlabelled fence becomes a
  Rust doctest. Mitigated by the fence rule plus a lint in the guard test.
- Fumadocs scaffold layout may change between versions; pin after scaffolding.
