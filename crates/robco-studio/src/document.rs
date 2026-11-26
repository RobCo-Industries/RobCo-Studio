// SPDX-License-Identifier: GPL-3.0-or-later

//! An open file: its bytes on disk to editor text and back.
//!
//! The text holds LF only, never a CR. A file with CRLF on every line is read
//! as LF and written back as CRLF. A file that mixes the two, has a lone CR,
//! or is not UTF-8, is refused, so open then save gives the same bytes. Text
//! that enters by any other door goes through [`to_lf`].

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};

/// Why a file was not opened.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The file could not be read.
    Read(String),
    /// The file is not UTF-8.
    NotText {
        /// The offset of the first byte that is not UTF-8.
        valid_up_to: usize,
    },
    /// Some lines end in CRLF and others in LF.
    MixedLineEndings {
        /// Lines that end in CRLF.
        crlf: usize,
        /// Lines that end in LF.
        lf: usize,
    },
    /// A CR stands without an LF after it.
    LoneCr {
        /// How many.
        count: usize,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read(e) => write!(
                f,
                "Studio could not read the file: {e}. Nothing was opened."
            ),
            LoadError::NotText { valid_up_to } => write!(
                f,
                "This file is not UTF-8 text, from byte {valid_up_to}. Studio has no editor for it. Nothing was opened."
            ),
            LoadError::MixedLineEndings { crlf, lf } => write!(
                f,
                "This file mixes line endings, {crlf} CRLF and {lf} LF. Nothing was opened."
            ),
            LoadError::LoneCr { count } => write!(
                f,
                "This file has {count} CR without LF. Nothing was opened."
            ),
        }
    }
}

/// Text for the editor, and whether to write CRLF.
pub fn decode(bytes: &[u8]) -> Result<(String, bool), LoadError> {
    let s = std::str::from_utf8(bytes).map_err(|e| LoadError::NotText {
        valid_up_to: e.valid_up_to(),
    })?;
    let crlf = s.matches("\r\n").count();
    let lf = s.matches('\n').count() - crlf;
    let lone = s.matches('\r').count() - crlf;
    if lone > 0 {
        return Err(LoadError::LoneCr { count: lone });
    }
    match (crlf, lf) {
        (0, _) => Ok((s.to_owned(), false)),
        (_, 0) => Ok((s.replace("\r\n", "\n"), true)),
        _ => Err(LoadError::MixedLineEndings { crlf, lf }),
    }
}

/// The bytes to write for `text`.
pub fn encode(text: &str, crlf: bool) -> Vec<u8> {
    if crlf {
        text.replace('\n', "\r\n").into_bytes()
    } else {
        text.as_bytes().to_vec()
    }
}

/// Text from outside the file, such as a paste: CRLF and a lone CR become LF.
pub fn to_lf(s: &str) -> Cow<'_, str> {
    if s.contains('\r') {
        s.replace("\r\n", "\n").replace('\r', "\n").into()
    } else {
        s.into()
    }
}

/// One open file. Its text widget's state, undo history included, is keyed
/// by `id`, so two documents never share it.
#[derive(Debug)]
pub struct Doc {
    /// Unique for the life of the window.
    pub id: u64,
    /// The file, the only place Save writes.
    pub path: PathBuf,
    /// The path to show, relative to the project.
    pub shown: String,
    /// The text, LF only.
    pub text: String,
    saved: String,
    crlf: bool,
    /// A selection, in characters, to apply before the next draw.
    pub pending_sel: Option<(usize, usize)>,
    /// The caret's line and column, from 1, after the last draw.
    pub caret: Option<(usize, usize)>,
}

impl Doc {
    /// Read `path`.
    pub fn open(id: u64, path: &Path, shown: String) -> Result<Doc, LoadError> {
        let bytes = std::fs::read(path).map_err(|e| LoadError::Read(e.to_string()))?;
        let (text, crlf) = decode(&bytes)?;
        Ok(Doc {
            id,
            path: path.to_owned(),
            shown,
            saved: text.clone(),
            text,
            crlf,
            pending_sel: None,
            caret: None,
        })
    }

    /// The text differs from the file as last opened or saved.
    pub fn is_dirty(&self) -> bool {
        self.text != self.saved
    }

    /// The file name, without folders.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(|| self.shown.clone(), |n| n.to_string_lossy().into_owned())
    }

    /// Write the text to the document's own file.
    pub fn save(&mut self) -> Result<(), String> {
        write_safely(&self.path, &encode(&self.text, self.crlf)).map_err(|e| {
            format!(
                "Studio could not save {}: {e}. The file was left as it was.",
                self.shown
            )
        })?;
        self.saved = self.text.clone();
        Ok(())
    }
}

/// Write `bytes` to `path` so that the file is either the old one or the new
/// one, never cut short.
///
/// The bytes go to a new temporary file in the same folder, synced, then
/// renamed over the file. A link is followed: the file it points to is
/// replaced and the link stays. The new file keeps the old file's permission
/// bits. The temporary file is created new under a name no one has, so an
/// existing file of that name is never opened. On failure the temporary file
/// is removed and the file is not touched. `std::fs::rename` replaces an
/// existing file on Windows too.
fn write_safely(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::{ErrorKind, Write};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let target = match std::fs::canonicalize(path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => path.to_owned(),
        Err(e) => return Err(e),
    };
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_owned(),
        _ => PathBuf::from("."),
    };
    let name = target
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let perms = std::fs::metadata(&target).ok().map(|m| m.permissions());
    let mut tries = 0;
    let (tmp, mut file) = loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(".{name}.save-{}.{n}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == ErrorKind::AlreadyExists && tries < 16 => tries += 1,
            Err(e) => return Err(e),
        }
    };
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| match &perms {
            Some(p) => file.set_permissions(p.clone()),
            None => Ok(()),
        });
    drop(file);
    let done = written.and_then(|()| std::fs::rename(&tmp, &target));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_round_trip() {
        let (t, crlf) = decode(b"a\r\nb\r\n").unwrap_or_default();
        assert!(crlf);
        assert_eq!(t, "a\nb\n");
        assert_eq!(encode(&t, crlf), b"a\r\nb\r\n");
    }

    #[test]
    fn refusals() {
        assert!(matches!(
            decode(b"a\r\nb\n"),
            Err(LoadError::MixedLineEndings { .. })
        ));
        assert!(matches!(
            decode(b"a\rb"),
            Err(LoadError::LoneCr { count: 1 })
        ));
        assert!(matches!(
            decode(&[0xff, 0]),
            Err(LoadError::NotText { valid_up_to: 0 })
        ));
    }

    #[test]
    fn paste_is_lf() {
        assert_eq!(to_lf("a\r\nb\rc"), "a\nb\nc");
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("robco-doc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn failed_save_keeps_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("fail");
        let f = d.join("main.c");
        std::fs::write(&f, b"old\n").unwrap();
        let mut doc = Doc::open(1, &f, "main.c".into()).unwrap();
        doc.text = "new\n".into();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).unwrap();
        let r = doc.save();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(r.is_err());
        assert!(doc.is_dirty());
        assert_eq!(std::fs::read(&f).unwrap(), b"old\n");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_link_and_mode() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("link");
        let real = d.join("real.c");
        let link = d.join("link.c");
        std::fs::write(&real, b"old\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut doc = Doc::open(1, &link, "link.c".into()).unwrap();
        doc.text = "new\n".into();
        doc.save().unwrap();
        assert!(!doc.is_dirty());
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&real).unwrap(), b"new\n");
        let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
