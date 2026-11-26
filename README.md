# RobCo Studio

RobCo Studio is the development tool for the RobCo Industries operating systems. It is a `robco` command and a window. It will build holotapes and apps for [Pip-OS](https://github.com/RobCo-Industries/Pip-OS) and [UOS](https://github.com/RobCo-Industries/UOS), run them in an emulator and write cards.

## Status

Nothing is usable yet. What exists:

- A Cargo workspace with six crates. `robco sdk list` and `robco sdk install` list and install SDK packages and tools from a local or an `https://` index, by [STUDIO_CLI.md](https://github.com/RobCo-Industries/RobCo-Core/blob/main/docs/spec/STUDIO_CLI.md). No other command exists yet. There is no default index: `--index` is required. `robco --version`, `robco version` and `robco-studio` print the version. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- CI on Ubuntu, macOS and Windows: format, lints, tests, a licence check, a release build of `robco` and a link check of the Markdown files.
- Seven spikes, throwaway code that answers one question each, with their results. No RobCo-Core C is linked into the crates yet. See [docs/SPIKES.md](docs/SPIKES.md).

To build and test, see [CONTRIBUTING.md](CONTRIBUTING.md). Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).

The licence covers Studio's own code. It does not cover what you make with Studio: your projects, your art, your tapes and your card images are yours, under any licence you pick. The files that end up inside a tape live in [RobCo Reel](https://github.com/RobCo-Industries/RobCo-Reel) under MIT.

## Names

The names come from the Fallout games. All code, art, sound and text in this repository are original. This project is not affiliated with the owners of Fallout.
