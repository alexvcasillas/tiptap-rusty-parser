# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/alexvcasillas/tiptap-studio/compare/studio-v0.3.0...studio-v0.3.1) - 2026-10-24

### Other

- updated the following local packages: tiptap-rusty-parser

## [0.3.0](https://github.com/alexvcasillas/tiptap-studio/compare/studio-v0.2.1...studio-v0.3.0) - 2026-10-24

### Other

- *(parser)* [**breaking**] box Node/Mark attrs and extra maps, serde without flatten ([#66](https://github.com/alexvcasillas/tiptap-studio/pull/66))

## [0.2.1](https://github.com/alexvcasillas/tiptap-studio/compare/studio-v0.2.0...studio-v0.2.1) - 2026-10-23

### Other

- move the parser crate into parser/ and make the root a virtual workspace ([#61](https://github.com/alexvcasillas/tiptap-studio/pull/61))
- point repository links at the renamed alexvcasillas/tiptap-studio ([#59](https://github.com/alexvcasillas/tiptap-studio/pull/59))

## [0.2.0](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/studio-v0.1.2...studio-v0.2.0) - 2026-10-23

### Added

- *(studio)* Tiptap logo in the app and as the app icon ([#57](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/57))

### Fixed

- *(studio)* theme overlay reaches component tokens, Tiptap purple accent, Copy JSON button with Copied feedback ([#58](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/58))
- *(studio)* retry stapler validate in verify on transient ticket-lookup errors ([#55](https://github.com/alexvcasillas/tiptap-rusty-parser/pull/55))

## [0.1.2](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/studio-v0.1.1...studio-v0.1.2) - 2026-10-23

### Other

- updated the following local packages: tiptap-rusty-parser

## [0.1.1](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/studio-v0.1.0...studio-v0.1.1) - 2026-10-23

### Fixed

- address review findings (rebase no-op moves and splice offsets, loopback URL check, version date order, text leaves and hard breaks in preview, skip stapling zips)
- *(studio)* retry stapler on transient ticket-lookup network errors

### Other

- release-plz versions and tags the studio; release PR merge publishes crate, npm and desktop builds
