// SPDX-License-Identifier: GPL-3.0-or-later

//! The build script's reading of the RobCo-Core commit, on git folder
//! layouts made here from plain files, with no git program. Each case
//! checks the commit and every path the build script tells Cargo to watch.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test file; a failed expect is a failed test"
)]

#[path = "../build/core_git.rs"]
mod core_git;

use std::fs;
use std::path::{Path, PathBuf};

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const REF: &str = "refs/heads/main";

/// A fresh folder for one case, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("core_git")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn packed(commit: &str) -> String {
    format!("# pack-refs with: peeled fully-peeled sorted\n{commit} {REF}\n")
}

#[test]
fn detached_head() {
    let s = Scratch::new("detached");
    let git = s.0.join(".git");
    write(&git.join("HEAD"), &format!("{A}\n"));
    let r = core_git::read(&s.0);
    assert_eq!(r.commit.as_deref(), Some(A));
    assert_eq!(r.watched, [git.clone(), git.join("HEAD")]);
}

#[test]
fn loose_ref() {
    let s = Scratch::new("loose");
    let git = s.0.join(".git");
    write(&git.join("HEAD"), &format!("ref: {REF}\n"));
    write(&git.join(REF), &format!("{A}\n"));
    write(&git.join("packed-refs"), &packed(B));
    let r = core_git::read(&s.0);
    assert_eq!(r.commit.as_deref(), Some(A));
    assert_eq!(
        r.watched,
        [
            git.clone(),
            git.join("HEAD"),
            git.join(REF),
            git.join("packed-refs")
        ]
    );
}

#[test]
fn ref_only_in_packed_refs_then_loose() {
    let s = Scratch::new("packed");
    let git = s.0.join(".git");
    write(&git.join("HEAD"), &format!("ref: {REF}\n"));
    write(&git.join("packed-refs"), &packed(A));
    let r = core_git::read(&s.0);
    assert_eq!(r.commit.as_deref(), Some(A));
    let want = [
        git.clone(),
        git.join("HEAD"),
        git.join(REF),
        git.join("packed-refs"),
    ];
    // The loose path is watched while it does not exist yet.
    assert!(!git.join(REF).exists());
    assert_eq!(r.watched, want);
    // The branch moves and becomes loose: the watched path now exists.
    write(&git.join(REF), &format!("{B}\n"));
    let r = core_git::read(&s.0);
    assert_eq!(r.commit.as_deref(), Some(B));
    assert_eq!(r.watched, want);
}

#[test]
fn dot_git_file_points_elsewhere() {
    let s = Scratch::new("gitfile");
    let root = s.0.join("core");
    let dir = s.0.join("modules/core");
    write(&root.join(".git"), "gitdir: ../modules/core\n");
    write(&dir.join("HEAD"), &format!("ref: {REF}\n"));
    write(&dir.join("packed-refs"), &packed(A));
    let r = core_git::read(&root);
    assert_eq!(r.commit.as_deref(), Some(A));
    let dir = root.join("../modules/core");
    assert_eq!(
        r.watched,
        [
            root.join(".git"),
            dir.join("HEAD"),
            dir.join(REF),
            dir.join("packed-refs")
        ]
    );
}

#[test]
fn linked_worktree_with_commondir() {
    let s = Scratch::new("worktree");
    let root = s.0.join("wt");
    let common = s.0.join("main.git");
    write(&root.join(".git"), "gitdir: ../main.git/worktrees/wt\n");
    let dir = root.join("../main.git/worktrees/wt");
    write(&common.join("worktrees/wt/HEAD"), &format!("ref: {REF}\n"));
    write(&common.join("worktrees/wt/commondir"), "../..\n");
    write(&common.join("packed-refs"), &packed(A));
    let r = core_git::read(&root);
    assert_eq!(r.commit.as_deref(), Some(A));
    let c = dir.join("../..");
    assert_eq!(
        r.watched,
        [
            root.join(".git"),
            dir.join("HEAD"),
            dir.join("commondir"),
            dir.join(REF),
            c.join(REF),
            c.join("packed-refs"),
        ]
    );
    // The branch moves to a loose ref in the common folder.
    write(&common.join(REF), &format!("{B}\n"));
    assert_eq!(core_git::read(&root).commit.as_deref(), Some(B));
}

#[test]
fn no_dot_git() {
    let s = Scratch::new("none");
    let r = core_git::read(&s.0);
    assert_eq!(r.commit, None);
    assert_eq!(r.watched, [s.0.join(".git")]);
}
