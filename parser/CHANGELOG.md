# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.2](https://github.com/alexvcasillas/tiptap-studio/compare/v0.5.1...v0.5.2) - 2026-10-23

### Added

- *(wasm)* publish to npm as @tiptap-studio/rusty-parser ([#63](https://github.com/alexvcasillas/tiptap-studio/pull/63))

### Other

- *(parser)* fix wasm links after the move into parser/ ([#62](https://github.com/alexvcasillas/tiptap-studio/pull/62))
- move the parser crate into parser/ and make the root a virtual workspace ([#61](https://github.com/alexvcasillas/tiptap-studio/pull/61))

## [0.5.1](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.5.0...v0.5.1) - 2026-10-23

### Other

- keep the parser's v{{ version }} tag and release names ([#53](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/53))

## [0.5.0](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/tiptap-rusty-parser-v0.4.4...tiptap-rusty-parser-v0.5.0) - 2026-10-23

### Added

- *(source-map)* map node paths to JSON byte spans and back
- *(studio)* read-only Tiptap Document Server browser with keychain-only tokens, open as A/B and diff versions
- *(studio)* dropping a schema file loads it as the schema instead of opening it as A or B
- *(studio)* schema validation with StarterKit schema, tree/inspector markers, Problems drawer and F8 stepping
- *(schema)* locate offending child and mark in Violation, report unknown children once
- *(studio)* open files sent by the OS (Finder, open -a, Dock drop) at launch and through the unsaved guard
- *(studio)* resizable, collapsible panes (cmd-1/cmd-2) with layout and window bounds persisted to layout.json
- *(studio)* diff change list in document order, sync scroll, Swap A/B, take B for subtree
- *(studio)* find box with query language, live matches, counter, next/prev, tree and preview highlights
- *(query)* small query language compiled to a zero-alloc node predicate (Query::parse, matches, find_paths)
- *(studio)* insert node chooser, enter/duplicate/copy/cut/paste keys and raw JSON edit
- *(studio)* open A via dialog, drop and recent files, unsaved-changes guard, parse errors as UI state, reload on change
- *(studio)* diff as merge tool, Dismiss hides a change, Save and Save as write the merged A
- *(studio)* Keep A resolves a change, survives edits, toggles back, and Apply all skips kept changes
- *(studio)* diff mode with change list, tinted A/B trees, before/after inspector and undoable Take B
- *(change-ops)* rebase_change to cherry-pick a mid-list diff Change onto its base
- *(change-ops)* unmap_path, inverse of map_path for live-index change paths
- *(transform)* apply_change to cherry-pick a diff Change into a transaction
- *(studio)* typed attr editors, add attr, text editing, editable mark attrs, add mark with attrs
- *(studio)* arrow-key document navigation and zero-padding preview frames with aligned list markers
- *(studio)* wrap-in chooser, split, join next, alt-arrow moves, disabled structure actions
- *(studio)* GPUI desktop inspector scaffold on gpui-kit 0.8

### Fixed

- address review findings (rebase no-op moves and splice offsets, loopback URL check, version date order, text leaves and hard breaks in preview, skip stapling zips)
- *(studio)* toolbar fits at 1280 px: shrinkable find box, short schema label, icon toggle for Only matches, window min size
- *(studio)* from_doc and replace_doc know the Document Server origin field
- *(studio)* ignore a saved window position that no connected display shows
- *(studio)* scope alt-arrow node moves to the Document context so they never fire while typing

### Other

- detect the studio version from its git tags so release-plz bumps it ([#52](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/52))
- release-plz versions and tags the studio; release PR merge publishes crate, npm and desktop builds
- Revert "build(studio): sign and notarize macOS releases locally via release.sh; CI builds only Windows/Linux"
- *(studio)* sign and notarize macOS releases locally via release.sh; CI builds only Windows/Linux
- bump Cargo.lock to tiptap-rusty-parser 0.4.4
- Merge remote-tracking branch 'origin/main' into feat/gpui-studio
- *(diff)* skip Myers when the length gap already exceeds its edit budget
- *(diff)* use as_chunks in Fx hasher for clippy 1.99
- *(diff)* linear move settle for reordered equal siblings, one Move per relocated node
- *(bench)* wrap+undo bench on one tree, no per-iteration clone/drop
- *(diff)* Myers LCS over interned child ids for long lists, O(n) move pairing
- *(transform)* record inverse incrementally (finish_with_inverse, apply_inverting, rollback); scope structural diffs to the touched subtree
- commit Cargo.lock and build releases with --locked
- *(studio)* document the Document Server browser and its security model
- *(diff)* regression tests for absent vs empty content/attrs/marks through transform, invert and apply
- *(studio)* Swap replaces A through DocumentState::replace_doc
- *(studio)* windows_subsystem attribute replaces Windows linker flags in release workflow
- *(studio)* release, secrets, verification and unsigned-build guide
- *(studio)* release workflow for macOS universal, Windows and Linux, replaces studio-mac.yml
- *(studio)* macOS .app bundle, dmg, signing and notarization scripts, Linux tarball, placeholder icon
- *(studio)* tick phase 2 roadmap and document key bindings
- *(studio)* macOS build workflow with downloadable artifact
- studio rules in CLAUDE.md

## [0.4.4](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.4.3...v0.4.4) - 2026-10-22

### Other

- *(diff)* fix exponential Move cascade on reordered equal siblings ([#47](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/47))

## [0.4.3](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.4.2...v0.4.3) - 2026-05-31

### Added

- *(wasm)* neutral AI-Toolkit integration surface + INTEGRATION.md ([#44](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/44))

## [0.4.2](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.4.1...v0.4.2) - 2026-05-31

### Added

- *(pos-map)* flat-position mapping (PosMap) for carrying positions through edits ([#42](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/42))

## [0.4.1](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.4.0...v0.4.1) - 2026-05-31

### Added

- *(pos-edit)* position-addressed apply/invert (PosEdit + inline Transform builders) ([#40](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/40))

## [0.4.0](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.8...v0.4.0) - 2026-05-31

### Added

- *(diff)* inline character-level text diff (diff_with + Change::SpliceText) ([#38](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/38))

## [0.3.8](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.7...v0.3.8) - 2026-05-30

### Added

- *(pos)* flat ProseMirror position model ([#36](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/36))

## [0.3.7](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.6...v0.3.7) - 2026-05-30

### Added

- *(wasm)* block ops, change algebra, and TypeScript types ([#34](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/34))

The Rust library is unchanged from `0.3.6`; this is a **paired patch release** so
the npm package (versioned off the crate) ships the new WASM bindings and the
crate and npm versions stay in sync.

## [0.3.6](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.5...v0.3.6) - 2026-05-30

### Other

- property-based tests (proptest) for core invariants ([#32](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/32))

## [0.3.5](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.4...v0.3.5) - 2026-05-30

### Added

- *(change-ops)* compose, compact, and map_path over change lists ([#30](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/30))

## [0.3.4](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.3...v0.3.4) - 2026-05-30

### Added

- *(block)* structural editing — split/join/wrap/lift/set_block_type ([#28](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/28))

## [0.3.3](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.2...v0.3.3) - 2026-05-30

### Other

- *(ci)* semver-checks job, runnable examples, docs.rs metadata ([#26](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/26))

## [0.3.2](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.1...v0.3.2) - 2026-05-30

### Added

- *(transform)* transaction API recording an invertible Change log ([#24](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/24))

## [0.3.1](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.3.0...v0.3.1) - 2026-05-30

### Added

- *(range)* inline range editing commands on a block ([#22](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/22))

## [0.3.0](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.2.2...v0.3.0) - 2026-05-30

### Added

- *(diff)* detect moves, emit Change::Move instead of remove+insert ([#20](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/20))

## [0.2.2](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.2.1...v0.2.2) - 2026-05-30

### Added

- normalize() canonicalizes trees (merge adjacent text, drop empties) ([#18](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/18))

## [0.2.1](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.2.0...v0.2.1) - 2026-05-30

### Added

- render JSONContent to HTML (to_html)

### Fixed

- *(html)* whitelist text-align values and clarify escaping/security docs

### Other

- document HTML rendering

## [0.2.0](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.1.5...v0.2.0) - 2026-05-30

### Added

- content-expression schema validation (cardinality, ordering, groups)

## [0.1.5](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.1.4...v0.1.5) - 2026-05-30

### Added

- invert change lists for diff-based undo

### Other

- *(diff)* drop minimality claim from invert

## [0.1.4](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.1.3...v0.1.4) - 2026-05-30

### Added

- *(wasm)* expose diff and applyChanges on TiptapDoc
- structural diff and apply for JSONContent trees

### Fixed

- *(diff)* validate insert index and preserve empty-vs-absent container shapes

### Other

- document structural diffing

## [0.1.3](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.1.2...v0.1.3) - 2026-05-30

### Other

- pin npm to 11.5.1 instead of latest for reproducible releases
- Merge branch 'main' into claude/tiptap-json-parser-rust-l5DMR
- Update GitHub Sponsors username in FUNDING.yml

## [0.1.2](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.1.1...v0.1.2) - 2026-05-29

### Added

- add opt-in schema validation

### Other

- document schema validation

## [0.1.1](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/v0.1.0...v0.1.1) - 2026-05-29

### Added

- add selectors, node paths, and text utilities

### Other

- document selectors, node paths, and text extraction
