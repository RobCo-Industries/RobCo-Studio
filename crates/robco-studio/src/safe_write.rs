// SPDX-License-Identifier: GPL-3.0-or-later

//! The one way Studio writes a file: a new temporary file in the same
//! folder, synced, then a rename over the destination. A saved document, a
//! theme file and `settings.toml` all go through [`write`].
//!
//! - The temporary file is created new and exclusive, so an existing file
//!   or link of that name is never opened. On Unix it is private, 0600,
//!   before any byte goes in.
//! - An existing destination keeps its permission bits. A read-only one is
//!   refused, since the rename would replace it in a writable folder.
//! - A missing destination gets the bits [`New`] asks for.
//! - A link at the destination is replaced by the new file, never written
//!   through and never followed. Its target is not touched. Settings and
//!   theme files are Studio's own, so a link there is replaced. A document
//!   resolves its link before it calls [`write`], so a linked source file
//!   keeps its link: see `document::write_safely`.
//! - On failure the temporary file is removed and the destination is
//!   untouched.
//!
//! Known limit: the result is a new file. It does not keep extended
//! attributes, access control lists, an owner or group that differ from the
//! person saving, or hard links to the old file.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The bits of a destination that did not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum New {
    /// A person's settings: 0600 on Unix.
    Private,
    /// What a plain create gives under the umask, as an editor does.
    Plain,
}

/// The name of a temporary file: short, fixed in shape, and apart from
/// every destination name. It starts with `.` and ends in `.tmp`, so the
/// theme listing never takes one left by a crash as a theme.
pub fn temp_name(pid: u32, n: u64) -> String {
    format!(".robco-{pid}-{n}.tmp")
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a file in `dir` under a fresh temporary name, new and exclusive.
/// On Unix its bits are `mode` from the start.
fn create_temp(dir: &Path, #[cfg(unix)] mode: u32) -> std::io::Result<(PathBuf, std::fs::File)> {
    let mut tries = 0;
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(temp_name(std::process::id(), n));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, mode);
        match opts.open(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists && tries < 16 => tries += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Write `bytes` as `path`. See the module docs for what holds.
pub fn write(path: &Path, bytes: &[u8], new: New) -> std::io::Result<()> {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_owned(),
        _ => PathBuf::from("."),
    };
    // `symlink_metadata` never follows a link: a link counts as absent and
    // the rename replaces it.
    let perms = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => None,
        Ok(m) if m.permissions().readonly() => {
            return Err(std::io::Error::new(
                ErrorKind::PermissionDenied,
                "the file is read-only",
            ));
        }
        Ok(m) => Some(m.permissions()),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let (tmp, mut file) = create_temp(
        &dir,
        #[cfg(unix)]
        0o600,
    )?;
    #[cfg(test)]
    tests::temp_opened(&tmp);
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| match perms {
            Some(p) => file.set_permissions(p),
            None => new_perms(&dir, new).and_then(|p| match p {
                Some(p) => file.set_permissions(p),
                None => Ok(()),
            }),
        });
    drop(file);
    let done = written.and_then(|()| std::fs::rename(&tmp, path));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// The bits for a new destination, or none to keep the temporary file's.
#[cfg(unix)]
fn new_perms(dir: &Path, new: New) -> std::io::Result<Option<std::fs::Permissions>> {
    match new {
        // The temporary file is already 0600.
        New::Private => Ok(None),
        New::Plain => plain_create_perms(dir).map(Some),
    }
}

/// Windows has no mode bits: the new file keeps its default attributes.
#[cfg(not(unix))]
fn new_perms(_dir: &Path, _new: New) -> std::io::Result<Option<std::fs::Permissions>> {
    Ok(None)
}

/// The bits a plain create gives a new file in `dir` under the umask. A
/// second, empty file is made the plain way, read and removed: this reads
/// the umask without `unsafe`.
#[cfg(unix)]
fn plain_create_perms(dir: &Path) -> std::io::Result<std::fs::Permissions> {
    let (probe, f) = create_temp(dir, 0o666)?;
    let perms = f.metadata().map(|m| m.permissions());
    drop(f);
    let removed = std::fs::remove_file(&probe);
    perms.and_then(|p| removed.map(|()| p))
}

/// Make `dir` and its missing parents. On Unix each folder made here is
/// private, 0700. An existing folder's bits are left alone.
pub fn make_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

#[cfg(test)]
pub(crate) mod tests;
