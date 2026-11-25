# Changelog

All notable changes to this project are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A Cargo workspace with five crate shells: `studio-sys`, `studio-disk`, `studio-core`, `robco` and `robco-studio`.
- `robco --version` and `robco version`.
- CI on Ubuntu, macOS and Windows: format, lints, tests, a licence check, a release build of `robco` and a Markdown link check.
- Seven spikes, K1 to K7, with a result page each, and an index in `docs/SPIKES.md`.

### Changed

- Spike K1 pins RobCo-Core `c6b6482`, where the racy canary case is fixed, and no longer retries any case.
