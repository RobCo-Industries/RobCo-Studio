# Changelog

All notable changes to this project are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A Cargo workspace with five crate shells: `studio-sys`, `studio-disk`, `studio-core`, `robco` and `robco-studio`.
- `robco --version` and `robco version`.
- RobCo-Core as a submodule at `external/robco-core`, and `studio-sys`, which compiles its C and wraps CRC-32, INI, `rimg_validate` and FAT16 card images.
- CI on Ubuntu, macOS and Windows: format, lints, tests, a licence check, a release build of `robco` and a Markdown link check.
- `studio-core::plan`, which reads a package's `flags.json` and lists the commands of a build: the startup code and source compiles with their three path maps, the link and `objcopy`, and the folders under `build/` those commands write into. It refuses a path field of `flags.json` that is not a plain relative path with `/`. It runs nothing.
- `studio-core::resolve`, which reads the SDK index and a package's `sdk.json`, picks package versions and tools for a target, and reads, checks and writes `robco.lock`. It refuses an `sdk.json` whose fields differ from its index entry, and a lock that is stale for the target or the host. It writes the lock through a temporary file, so a failure leaves the old lock as it was. It downloads and installs nothing.
- `studio-core::install`, which downloads, checks and installs packages and tools into the cache. It hashes a download while it streams to a temporary file, checks a package archive and its `sdk.json` before it extracts anything, checks each file, refuses links that leave a tool archive, and moves the folder into the cache with its `.robco-archive` record in one rename. The caller passes the transport. Only a local one exists. It refuses a `pipos` package that lacks a file of "Layout of pipos" with `package: manifest`, before it extracts anything, and tells its caller each step as it begins and ends.
- `robco sdk list` and `robco sdk install`, with `--index` and `--json`, by [STUDIO_CLI.md](https://github.com/RobCo-Industries/RobCo-Core/blob/main/docs/spec/STUDIO_CLI.md). A failure leaves open the `step` it happened in, so a bad archive shows `install` started, not `verify`. They read a local index only: an `https://` index fails with `index: read`. Tests run the program against a local index and a cache in a temporary folder, and check its output against the spec's two examples.
- `studio-core::codes`, the table of the spec's error codes and the exit code of each. Every error of `manifest`, `resolve`, `install` and `boot` names its code.
- `studio-sys` compiles RobCo-Core's `src/lib/rimg_seal.c` and wraps `rimg_seal` and `rimg_pack`. A test runs every vector of RobCo-Core's `tests/vectors/rimg/vectors.ini` through them.
- Seven spikes, K1 to K7, with a result page each, and an index in `docs/SPIKES.md`.

### Changed

- A `project.title` in `robco.toml` with a control character, below U+0020 or U+007F, fails with `manifest: title`.
- A missing or bad `format` in `boot.json` fails with `package: format`.
- `robco.toml` unknown keys are reported in file order, and a `build.backend` other than `robco` counts as one.
- `external/robco-core` moves from `c6b6482` to `e004c73`, which holds the final specs in `docs/spec/`. No file that `studio-sys` compiles or includes changed.
- `external/robco-core` moves from `e004c73` to `ef5079f`, which holds the C sealer and packer and keeps the vector files' bytes on a Windows checkout. Of the files `studio-sys` compiles or includes, only `include/robco/rimg.h` changed: it gains the seal and pack declarations. The card image bytes did not change.
- Spike K1 pins RobCo-Core `c6b6482`, where the racy canary case is fixed, and no longer retries any case.
