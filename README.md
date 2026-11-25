# RobCo Studio

RobCo Studio is the development tool for the RobCo Industries operating systems. It is a `robco` command and a window. It will build holotapes and apps for [Pip-OS](https://github.com/RobCo-Industries/Pip-OS) and [UOS](https://github.com/RobCo-Industries/UOS), run them in an emulator and write cards.

## Status

No feature works yet. This repository holds the skeleton of the first milestone:

- A Cargo workspace with five crates. Each one is an empty shell except for the version. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- `robco --version` and `robco version` print the version. No other command exists.
- `robco-studio` prints its version too. It opens no window yet.
- CI on Ubuntu, macOS and Windows: format, lints, tests, a licence check, a release build of `robco` and a link check of the Markdown files.

No RobCo-Core C is linked yet, and no window toolkit is chosen. Spikes decide both.

To build and test, see [CONTRIBUTING.md](CONTRIBUTING.md). Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).

The licence covers Studio's own code. It does not cover what you make with Studio: your projects, your art, your tapes and your card images are yours, under any licence you pick. The files that end up inside a tape live in [RobCo Reel](https://github.com/RobCo-Industries/RobCo-Reel) under MIT.

## Names

The names come from the Fallout games. All code, art, sound and text in this repository are original. This project is not affiliated with the owners of Fallout.
