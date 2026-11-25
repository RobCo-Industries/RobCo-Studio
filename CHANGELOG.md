# Changelog

All notable changes to this project are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A Cargo workspace with five crate shells: `studio-sys`, `studio-disk`, `studio-core`, `robco` and `robco-studio`.
- `robco --version` and `robco version`.
- RobCo-Core as a submodule at `external/robco-core`, and `studio-sys`, which compiles its C and wraps CRC-32, INI, `rimg_validate` and FAT16 card images.
- CI on Ubuntu, macOS and Windows: format, lints, tests, a licence check, a release build of `robco` and a Markdown link check.
