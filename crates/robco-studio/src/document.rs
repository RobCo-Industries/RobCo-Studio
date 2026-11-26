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
    /// The path is a folder, a pipe, a device or a socket.
    NotRegular,
    /// The file is longer than [`MAX_FILE`].
    TooLarge,
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
            LoadError::NotRegular => write!(
                f,
                "Studio opens regular files only, and this is not one. Nothing was opened."
            ),
            LoadError::TooLarge => write!(
                f,
                "This file is larger than {} MiB, the most Studio opens. Nothing was opened.",
                MAX_FILE / (1024 * 1024)
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

/// The largest file Studio reads, in bytes: 16 MiB.
pub const MAX_FILE: u64 = 16 * 1024 * 1024;

/// Read `path`, a regular file of at most `limit` bytes, links followed.
///
/// The file is opened once, and its kind and length come from that handle,
/// so it cannot change between the check and the read. On Unix the open
/// does not wait for a writer, so a named pipe is refused, not waited on.
pub fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, LoadError> {
    read_stamped(path, limit).map(|(b, _)| b)
}

/// What Studio knows of a file on disk to tell that another program
/// changed it: its length and modification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl Stamp {
    fn of(m: &std::fs::Metadata) -> Stamp {
        Stamp {
            len: m.len(),
            modified: m.modified().ok(),
        }
    }
}

/// The identity of a file: its canonical path, links followed. A file that
/// does not exist yet has none: its folder's canonical path joined with its
/// name stands in, or the path made absolute if the folder is missing too.
/// On a file system that folds case, as on macOS and Windows, the canonical
/// path carries the stored spelling, so two spellings of a name are one.
pub fn identity(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }
    let joined = match (path.parent(), path.file_name()) {
        (Some(d), Some(n)) => {
            let d = if d.as_os_str().is_empty() {
                Path::new(".")
            } else {
                d
            };
            std::fs::canonicalize(d).ok().map(|d| d.join(n))
        }
        _ => None,
    };
    joined
        .or_else(|| std::path::absolute(path).ok())
        .unwrap_or_else(|| path.to_owned())
}

/// [`read_regular`], with the stamp of the handle the bytes came from.
fn read_stamped(path: &Path, limit: u64) -> Result<(Vec<u8>, Stamp), LoadError> {
    use std::io::Read;
    let read = |e: std::io::Error| LoadError::Read(e.to_string());
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut opts, libc::O_NONBLOCK);
    let file = opts.open(path).map_err(read)?;
    let meta = file.metadata().map_err(read)?;
    if !meta.is_file() {
        return Err(LoadError::NotRegular);
    }
    if meta.len() > limit {
        return Err(LoadError::TooLarge);
    }
    // The file may grow after the check: read one byte past the limit to see.
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(read)?;
    if bytes.len() as u64 > limit {
        return Err(LoadError::TooLarge);
    }
    Ok((bytes, Stamp::of(&meta)))
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
    /// The file's identity when it was opened: see [`identity`].
    pub key: PathBuf,
    /// The path to show, relative to the project.
    pub shown: String,
    /// The text, LF only.
    pub text: String,
    saved: String,
    crlf: bool,
    stamp: Option<Stamp>,
    /// The bytes last read or written, to compare with the file on disk.
    disk: Vec<u8>,
    /// A selection, in characters, to apply before the next draw.
    pub pending_sel: Option<(usize, usize)>,
    /// The caret's line and column, from 1, after the last draw.
    pub caret: Option<(usize, usize)>,
}

impl Doc {
    /// Read `path`.
    pub fn open(id: u64, path: &Path, shown: String) -> Result<Doc, LoadError> {
        let (bytes, stamp) = read_stamped(path, MAX_FILE)?;
        let (text, crlf) = decode(&bytes)?;
        Ok(Doc {
            id,
            path: path.to_owned(),
            key: identity(path),
            stamp: Some(stamp),
            disk: bytes,
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
        // The rename would replace a read-only file in a writable folder:
        // keep the file's own say.
        if std::fs::metadata(&self.path).is_ok_and(|m| m.permissions().readonly()) {
            return Err(format!(
                "{} is read-only. Studio did not save it.",
                self.shown
            ));
        }
        let bytes = encode(&self.text, self.crlf);
        write_safely(&self.path, &bytes).map_err(|e| {
            format!(
                "Studio could not save {}: {e}. The file was left as it was.",
                self.shown
            )
        })?;
        self.saved = self.text.clone();
        self.stamp = std::fs::metadata(&self.path).ok().map(|m| Stamp::of(&m));
        self.disk = bytes;
        Ok(())
    }

    /// The file on disk is not the one last read or written: another
    /// program changed it, or removed it. A different length or time says
    /// changed at once; equal ones go on to compare the bytes, since a
    /// coarse clock can keep both. A file that cannot be read counts as
    /// changed.
    pub fn changed_on_disk(&self) -> bool {
        match std::fs::metadata(&self.path) {
            Ok(m) if Some(Stamp::of(&m)) != self.stamp => return true,
            Ok(_) => {}
            Err(_) => return true,
        }
        !read_regular(&self.path, MAX_FILE).is_ok_and(|b| b == self.disk)
    }
}

/// Write `bytes` to `path` so that the file is either the old one or the new
/// one, never cut short. A new document gets the bits a plain create gives,
/// as other editors do. See [`crate::safe_write`] for the rest.
///
/// Unlike Studio's own files, a document follows a link: the file it points
/// to is replaced and the link stays, so linked source files keep working. A
/// link loop fails here. A dangling link is not resolved, so the save
/// replaces the link itself. A link to a folder fails at the rename.
fn write_safely(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let target = match std::fs::canonicalize(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => path.to_owned(),
        Err(e) => return Err(e),
    };
    crate::safe_write::write(&target, bytes, crate::safe_write::New::Plain)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The document save path, for the shared write's tests.
    #[cfg(unix)]
    pub(crate) fn save_path(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        write_safely(path, bytes)
    }
    /// Make a named pipe with the `mkfifo` tool. False, with a printed
    /// line, when the tool is absent.
    #[cfg(unix)]
    pub(crate) fn mkfifo(path: &Path) -> bool {
        match std::process::Command::new("mkfifo").arg(path).status() {
            Ok(s) if s.success() => true,
            Ok(s) => panic!("mkfifo failed: {s}"),
            Err(e) => {
                eprintln!("skipped: no mkfifo tool: {e}");
                false
            }
        }
    }

    /// Run `f` on a thread; None if it takes longer than five seconds.
    pub(crate) fn within_time<T: Send + 'static>(
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
    }

    /// A named pipe with no writer is refused at once, not waited on.
    #[cfg(unix)]
    #[test]
    fn named_pipe_is_refused_without_waiting() {
        let d =
            Gone(std::env::temp_dir().join(format!("robco-studio-fifo-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&d.0);
        std::fs::create_dir_all(&d.0).unwrap();
        let f = d.0.join("pipe");
        if !mkfifo(&f) {
            return;
        }
        let r = within_time(move || Doc::open(1, &f, "pipe".into()).map(|d| d.text))
            .expect("opening a named pipe hung");
        assert_eq!(r.unwrap_err(), LoadError::NotRegular);
    }

    #[test]
    fn folder_is_refused() {
        let d = Gone(std::env::temp_dir().join(format!("robco-studio-dir-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&d.0);
        std::fs::create_dir_all(&d.0).unwrap();
        let r = within_time({
            let p = d.0.clone();
            move || Doc::open(1, &p, "d".into()).map(|d| d.text)
        })
        .expect("hung");
        assert!(r.is_err());
    }

    #[test]
    fn file_over_the_limit_is_refused() {
        let d = Gone(std::env::temp_dir().join(format!("robco-studio-big-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&d.0);
        std::fs::create_dir_all(&d.0).unwrap();
        let f = d.0.join("big.txt");
        let file = std::fs::File::create(&f).unwrap();
        file.set_len(MAX_FILE + 1).unwrap();
        assert_eq!(
            Doc::open(1, &f, "big".into()).unwrap_err(),
            LoadError::TooLarge
        );
        assert!(LoadError::TooLarge.to_string().contains("16 MiB"));
        file.set_len(MAX_FILE).unwrap();
        assert_eq!(read_regular(&f, MAX_FILE).unwrap().len() as u64, MAX_FILE);
    }

    /// Removes a test folder when the test ends, pass or fail.
    struct Gone(PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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

    #[test]
    fn same_length_and_time_still_seen() {
        struct Clean(PathBuf);
        impl Drop for Clean {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let d = scratch("stamp");
        let _clean = Clean(d.clone());
        let f = d.join("main.c");
        std::fs::write(&f, b"abc\n").unwrap();
        let mut doc = Doc::open(1, &f, "main.c".into()).unwrap();
        doc.text = "mine\n".into();
        assert!(!doc.changed_on_disk());
        let old = std::fs::metadata(&f).unwrap().modified().unwrap();
        std::fs::write(&f, b"xyz\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().modified().unwrap(), old);
        assert!(doc.changed_on_disk());
        // After a save the written bytes are the reference.
        doc.save().unwrap();
        assert!(!doc.changed_on_disk());
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

    /// A read-only file is not replaced by a save.
    #[test]
    fn read_only_file_is_not_saved() {
        let g = Gone(scratch("readonly"));
        let f = g.0.join("ro.c");
        std::fs::write(&f, b"old\n").unwrap();
        let mut doc = Doc::open(1, &f, "ro.c".into()).unwrap();
        let mut perms = std::fs::metadata(&f).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&f, perms).unwrap();
        doc.text = "new\n".into();
        let r = doc.save();
        assert!(r.is_err_and(|e| e.contains("read-only")));
        assert_eq!(std::fs::read(&f).unwrap(), b"old\n");
        assert!(doc.is_dirty());
    }
}
