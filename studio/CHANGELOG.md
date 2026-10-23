# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/alexvcasillas/tiptap-rusty-parser/compare/studio-v0.1.0...studio-v0.1.1) - 2026-10-23

### Fixed

- address review findings (rebase no-op moves and splice offsets, loopback URL check, version date order, text leaves and hard breaks in preview, skip stapling zips)
- *(studio)* retry stapler on transient ticket-lookup network errors

### Other

- release-plz versions and tags the studio; release PR merge publishes crate, npm and desktop builds
