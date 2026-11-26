// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the commit a git checkout has at `HEAD` from its git files,
//! without running git, and lists every file the answer depends on.
//!
//! `build.rs` and the test `tests/core_git.rs` both take this file with a
//! `#[path]` module, so the test runs the same code as the build.
//!
//! Cargo reruns a build script when a watched path changes. A watched path
//! that does not exist counts as changed on every build, so the script
//! reruns each time until it appears; a path that appears later is seen.

use std::fs;
use std::path::{Path, PathBuf};

/// What the reading found: the commit, if any, and the files it read or
/// would read.
pub struct CoreCommit {
    /// The 40 hex digits of `HEAD`, or `None` when the files do not say.
    pub commit: Option<String>,
    /// Every path whose change can change the answer, in reading order.
    pub watched: Vec<PathBuf>,
}

/// Reads the commit of the checkout at `root`.
pub fn read(root: &Path) -> CoreCommit {
    let mut watched = Vec::new();
    let commit = read_into(root, &mut watched);
    CoreCommit { commit, watched }
}

fn read_into(root: &Path, watched: &mut Vec<PathBuf>) -> Option<String> {
    // The `.git` entry decides the fallback, so it is watched even when
    // missing: a tree that gains one stops printing `unknown`.
    let dot_git = root.join(".git");
    watched.push(dot_git.clone());
    let dir = if dot_git.is_dir() {
        dot_git
    } else {
        // A `.git` file names the git folder on its `gitdir:` line.
        let text = fs::read_to_string(&dot_git).ok()?;
        let rel = text.trim().strip_prefix("gitdir:")?.trim();
        root.join(rel)
    };
    let head = dir.join("HEAD");
    watched.push(head.clone());
    // A linked worktree names its common folder in `commondir`.
    let commondir = dir.join("commondir");
    let common = match fs::read_to_string(&commondir) {
        Ok(c) => {
            watched.push(commondir);
            dir.join(c.trim())
        }
        Err(_) => dir.clone(),
    };
    let text = fs::read_to_string(&head).ok()?;
    let text = text.trim();
    let Some(name) = text.strip_prefix("ref:").map(str::trim) else {
        return is_commit(text).then(|| text.to_owned());
    };
    // Watch each loose ref path even when missing: a packed ref can become
    // loose. Then `packed-refs`, which a loose ref hides.
    let mut loose = vec![dir.join(name)];
    if common != dir {
        loose.push(common.join(name));
    }
    let packed = common.join("packed-refs");
    watched.extend(loose.iter().cloned());
    watched.push(packed.clone());
    for file in &loose {
        if let Ok(c) = fs::read_to_string(file) {
            let c = c.trim();
            return is_commit(c).then(|| c.to_owned());
        }
    }
    let packed = fs::read_to_string(packed).ok()?;
    packed.lines().find_map(|l| {
        let (c, n) = l.split_once(' ')?;
        (n.trim() == name && is_commit(c)).then(|| c.to_owned())
    })
}

fn is_commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
