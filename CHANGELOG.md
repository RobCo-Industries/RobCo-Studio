# Changelog

All notable changes to this project are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A Cargo workspace with five crate shells: `studio-sys`, `studio-disk`, `studio-core`, `robco` and `robco-studio`.
- `robco --version` and `robco version`.
- RobCo-Core as a submodule at `external/robco-core`, and `studio-sys`, which compiles its C and wraps CRC-32, INI, `rimg_validate` and FAT16 card images.
- CI on Ubuntu, macOS and Windows: format, lints, tests, a licence check, a release build of `robco` and a Markdown link check.
- `studio-core::plan`, which reads a package's `flags.json` and lists the commands of a build: the startup code and source compiles with their three path maps, the link and `objcopy`, and the folders under `build/` those commands write into. It refuses a path field of `flags.json` that is not a plain relative path with `/`. It runs nothing.
- Seven spikes, K1 to K7, with a result page each, and an index in `docs/SPIKES.md`.

### Changed

- `external/robco-core` moves from `c6b6482` to `e004c73`, which holds the final specs in `docs/spec/`. No file that `studio-sys` compiles or includes changed.
- Spike K1 pins RobCo-Core `c6b6482`, where the racy canary case is fixed, and no longer retries any case.
