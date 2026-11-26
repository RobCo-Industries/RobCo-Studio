# Contributing

## What to install

- [rustup](https://rustup.rs). The file `rust-toolchain.toml` pins the compiler, and rustup fetches it on the first `cargo` command.
- git. RobCo-Core is a submodule at `external/robco-core`: clone with `--recurse-submodules`, or run `git submodule update --init`.
- A C compiler, for `studio-sys`: the Xcode command line tools on macOS, `build-essential` on Ubuntu, or the Visual Studio build tools with their clang component, "C++ Clang Compiler for Windows", on Windows. On Windows the build script finds the GNU-style `clang` itself. You set nothing by hand.
- Python 3, for one test of `studio-sys` that seals an image with RobCo-Core's `tools/rimg.py`.

When the window gets a toolkit, Ubuntu will need its development packages.

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

## What CI checks

Besides the commands above, CI runs these jobs:

- `pins` runs `scripts/check-pins.py`. It checks that the toolchain and every manifest name the same Rust release. It checks that `external/robco-core` is declared, checked out at the commit the repository records, and that this commit is on RobCo-Core's `main`. It fetches the whole of that branch first, because a shallow clone cannot tell. A failed fetch fails with "could not tell", never with "not on main". The script uses the repository's git configuration but never prompts for a password or passphrase. If a `url.<base>.insteadOf` rule rewrites the RobCo-Core URL, for example `https://github.com/` to SSH, the script does not fetch: it fails with "could not tell" and names the rewrite. On such a machine the full check is a CI check. The tests run git with no global or system configuration, so a local signing or hook setup does not change their result. The same job runs the script's tests, `python3 -m unittest scripts/test_check_pins.py`, and `scripts/check-workflows.py`, which fails when a workflow file repeats a key. Git can merge two jobs of one name without a conflict.
- `file-licences` runs `reuse lint`. It skips the submodule, which carries its own licence files.
- `release` builds `robco` for one target per system: musl on Linux, built against `musl-gcc`, the static C runtime on Windows, Apple silicon on macOS. It checks that the binaries are static, and that `robco --version` names the RobCo-Core commit of the submodule. `robco` does not call the C yet, so the job builds, checks and runs the tests of `studio-sys` for the same target, which do. The job is `.github/workflows/release-build.yml`, which the pre-release workflow calls too. It also writes each program's third-party notices with `tools/release/notices.py`. The release build maps the cargo home to `/cargo` and the workspace to `/robco` with `--remap-path-prefix`, and `tools/release/scan.py` fails the job when a program still names the runner's home, the workspace or the cargo home.
- The `prerelease` workflow gathers the three programs, their notices and `robco-SHA256SUMS`, the SHA-256 of each file, written by `tools/release/sums.sh`. A pull request that touches it, or a manual start, is a dry run that keeps the files as an artifact. Only a pushed tag `robco-<version>-pre.<n>`, where `<version>` is the workspace version, publishes a GitHub pre-release. `tools/release/check-tag.sh` checks the tag's form before the upload. Run the tools' tests with `python3 -m unittest discover -s tools/release`.
- `sys-card` writes a card image, and `sys-card-mtools` reads it with mtools in the CI container.
- `docs` checks the links in the Markdown files.

## Branches and pull requests

Work on a branch from `main`. Open one pull request per milestone. A pull request merges when every CI job passes.

## Licence boundary

Studio is GPL-3.0-or-later. Nothing that ends up inside a user's tape, app or card image lives in this repository: no runtime, no template, no code skeleton. Those live in [RobCo Reel](https://github.com/RobCo-Industries/RobCo-Reel) under MIT. If a file you add would be copied or linked into a user's work, it belongs there.

<!-- REUSE-IgnoreStart -->
Each source file starts with an SPDX line: `// SPDX-License-Identifier: GPL-3.0-or-later`. Contributions come in under the repository's licence. `REUSE.toml` covers files that cannot hold a comment, and CI runs `reuse lint` on every file.
<!-- REUSE-IgnoreEnd -->
