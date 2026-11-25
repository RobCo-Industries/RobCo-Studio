# Contributing

## What to install

- [rustup](https://rustup.rs). The file `rust-toolchain.toml` pins the compiler, and rustup fetches it on the first `cargo` command.
- git.

That is all for now. When the sys crate links RobCo-Core's C, you will also need a C compiler: the Xcode command line tools on macOS, `build-essential` on Ubuntu, or the Visual Studio build tools with their clang component on Windows. When the window gets a toolkit, Ubuntu will need its development packages.

To run the licence check on your machine, install [cargo-deny](https://github.com/EmbarkStudios/cargo-deny). CI runs it on every pull request.

## Build, test, format and lint

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny --locked check
```

CI runs the same commands on Ubuntu, macOS and Windows and fails on any warning.

## Rules the code follows

- The core never depends on a window or toolkit crate. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) shows which crate may depend on which.
- `unsafe` is denied in every crate except `studio-sys` and `studio-disk`. There, each `unsafe` block needs a `SAFETY` comment.
- `unwrap`, `expect`, `panic!`, `todo!` and `dbg!` are denied outside tests.
- `Cargo.lock` is committed. A new dependency needs a reason, and its licence must be on the allow list in `deny.toml`.
- The toolchain in `rust-toolchain.toml` and `rust-version` in `Cargo.toml` move together, in a pull request that does nothing else.

## Branches and pull requests

Work on a branch from `main`. Open one pull request per milestone. A pull request merges when every CI job passes.

## Licence boundary

Studio is GPL-3.0-or-later. Nothing that ends up inside a user's tape, app or card image lives in this repository: no runtime, no template, no code skeleton. Those live in [RobCo Reel](https://github.com/RobCo-Industries/RobCo-Reel) under MIT. If a file you add would be copied or linked into a user's work, it belongs there.

Each source file starts with an SPDX line: `// SPDX-License-Identifier: GPL-3.0-or-later`. Contributions come in under the repository's licence.
