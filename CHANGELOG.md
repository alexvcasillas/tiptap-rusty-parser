# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
