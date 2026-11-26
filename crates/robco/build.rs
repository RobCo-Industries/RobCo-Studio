// SPDX-License-Identifier: GPL-3.0-or-later

//! Records the RobCo-Core commit that this build compiles, for
//! `robco --version`.
//!
//! RobCo-Core is the submodule at `external/robco-core`. The script reads
//! the commit from the submodule's git files, without running git, and
//! hands it to the program as `ROBCO_CORE_COMMIT`. Only the 40 hex digits
//! go into the program, never a path. It tells Cargo every file the
//! answer depends on, so a moved branch reruns it.
//!
//! In a source archive without git files the value is `unknown`, and the
//! build still succeeds.

use std::env;
use std::path::PathBuf;

#[path = "build/core_git.rs"]
mod core_git;

fn main() {
    let manifest = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    let found = core_git::read(&manifest.join("../../external/robco-core"));
    for path in &found.watched {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let commit = found.commit.unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=ROBCO_CORE_COMMIT={commit}");
}
