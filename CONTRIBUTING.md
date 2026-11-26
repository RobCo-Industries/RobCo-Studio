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
- `release` builds `robco` for one target per system: musl on Linux, built against `musl-gcc`, the static C runtime on Windows, Apple silicon on macOS. It checks that the binaries are static. `robco` does not call the C yet, so the job builds, checks and runs the tests of `studio-sys` for the same target, which do.
- `sys-card` writes a card image, and `sys-card-mtools` reads it with mtools in the CI container.
- `agreement` runs `tools/agreement.sh` in the CI container. It builds Pip-OS's `sdk/examples/tape`, at the commit the `pipos` package comes from, by Make and by `robco build` with the xPack compiler. It writes `robco.toml` from the Makefile's own values and runs the checks of "A project that also builds with a Makefile" in [STUDIO_PROJECT.md](https://github.com/RobCo-Industries/RobCo-Core/blob/main/docs/spec/STUDIO_PROJECT.md#a-project-that-also-builds-with-a-makefile): sources, ABI, compiler release, then the two tapes' SHA-256. Then it seals one raw image with `rimg.py` and with the C sealer and compares the bytes. A failure names the first byte that differs and its header field. The same job runs the script again with the container's own compiler for Make and fails unless that run fails. The hashes go up as the `agreement-hashes` artifact.
- `docs` checks the links in the Markdown files.

To run the agreement check on your machine, build a `pipos` package from a Pip-OS checkout with `make sdk-package`, pack your compiler's `bin/` as a `.tar.gz`, and give the script the inputs its header lists. The compiler the package names must be the one Make runs. Relative paths work: the script makes them absolute and stops with the variable's name when one is missing. `tools/test_agreement.sh` shows that the sealer check fails on a one-byte change; the `agreement` job runs it first.

## Branches and pull requests

Work on a branch from `main`. Open one pull request per milestone. A pull request merges when every CI job passes.

## Licence boundary

Studio is GPL-3.0-or-later. Nothing that ends up inside a user's tape, app or card image lives in this repository: no runtime, no template, no code skeleton. Those live in [RobCo Reel](https://github.com/RobCo-Industries/RobCo-Reel) under MIT. If a file you add would be copied or linked into a user's work, it belongs there.

<!-- REUSE-IgnoreStart -->
Each source file starts with an SPDX line: `// SPDX-License-Identifier: GPL-3.0-or-later`. Contributions come in under the repository's licence. `REUSE.toml` covers files that cannot hold a comment, and CI runs `reuse lint` on every file.
<!-- REUSE-IgnoreEnd -->
