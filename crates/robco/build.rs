// SPDX-License-Identifier: GPL-3.0-or-later

//! Records the RobCo-Core commit that this build compiles, for
//! `robco --version`.
//!
//! RobCo-Core is the submodule at `external/robco-core`. The script reads
//! the commit from the submodule's git files, without running git, and
//! hands it to the program as `ROBCO_CORE_COMMIT`. Only the 40 hex digits
//! go into the program, never a path.
//!
//! In a source archive without git files the value is `unknown`, and the
//! build still succeeds.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    let core = manifest.join("../../external/robco-core");
    let dot_git = core.join(".git");
    // The folder too: a `.git` that appears keeps its old date, but the
    // folder's date changes.
    println!("cargo:rerun-if-changed={}", core.display());
    println!("cargo:rerun-if-changed={}", dot_git.display());
    let commit = match git_dir(&core, &dot_git) {
        Some(dir) => {
            let head = dir.join("HEAD");
            println!("cargo:rerun-if-changed={}", head.display());
            read_head(&dir, &head).unwrap_or_else(|| "unknown".to_owned())
        }
        None => "unknown".to_owned(),
    };
    println!("cargo:rustc-env=ROBCO_CORE_COMMIT={commit}");
}

/// The submodule's git folder: `.git` itself, or the folder its
/// `gitdir:` line names.
fn git_dir(core: &Path, dot_git: &Path) -> Option<PathBuf> {
    if dot_git.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let text = fs::read_to_string(dot_git).ok()?;
    let rel = text.trim().strip_prefix("gitdir:")?.trim();
    let dir = core.join(rel);
    dir.is_dir().then_some(dir)
}

/// The commit of `HEAD`: a detached commit, or the commit of the branch
/// it names, from its ref file or from `packed-refs`.
fn read_head(dir: &Path, head: &Path) -> Option<String> {
    let text = fs::read_to_string(head).ok()?;
    let text = text.trim();
    let Some(name) = text.strip_prefix("ref:").map(str::trim) else {
        return is_commit(text).then(|| text.to_owned());
    };
    // A linked worktree keeps its refs in the common folder.
    let common = fs::read_to_string(dir.join("commondir"))
        .ok()
        .map(|c| dir.join(c.trim()))
        .unwrap_or_else(|| dir.to_path_buf());
    for base in [dir, common.as_path()] {
        let file = base.join(name);
        if let Ok(c) = fs::read_to_string(&file) {
            println!("cargo:rerun-if-changed={}", file.display());
            let c = c.trim();
            return is_commit(c).then(|| c.to_owned());
        }
    }
    let packed = fs::read_to_string(common.join("packed-refs")).ok()?;
    packed.lines().find_map(|l| {
        let (c, n) = l.split_once(' ')?;
        (n == name && is_commit(c)).then(|| c.to_owned())
    })
}

fn is_commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
