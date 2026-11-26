// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::store::tests::TempDir;
#[cfg(unix)]
use std::cell::RefCell;

// Only the Unix seam and tests read the temp file's bits.
#[cfg(unix)]
thread_local! {
    static TMP_MODES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

/// Test seam: record the temp file's bits as it is made, before the text
/// goes in.
pub(super) fn temp_opened(tmp: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(tmp) {
            TMP_MODES.with(|v| v.borrow_mut().push(m.permissions().mode() & 0o777));
        }
    }
    let _ = tmp;
}

/// The three kinds of file Studio writes, each through its own caller.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
enum Kind {
    Document,
    Theme,
    Settings,
}

#[cfg(unix)]
const KINDS: [Kind; 3] = [Kind::Document, Kind::Theme, Kind::Settings];

#[cfg(unix)]
impl Kind {
    fn file(self) -> &'static str {
        match self {
            Kind::Document => "main.c",
            Kind::Theme => "ember.toml",
            Kind::Settings => crate::store::SETTINGS,
        }
    }

    /// Write the kind's new content to `dir/file()` the way Studio does,
    /// and return those bytes.
    fn write(self, dir: &Path) -> (std::io::Result<()>, Vec<u8>) {
        let path = dir.join(self.file());
        match self {
            Kind::Document => {
                let bytes = b"int x;\n".to_vec();
                // A document opens from an existing file only; a first save
                // goes through the same function with the document's bits.
                if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
                    let mut doc = crate::document::Doc::open(1, &path, "main.c".into()).unwrap();
                    doc.text = "int x;\n".into();
                    let r = doc.save().map_err(std::io::Error::other);
                    (r, bytes)
                } else {
                    (write(&path, &bytes, New::Plain), bytes)
                }
            }
            Kind::Theme => {
                let spec = &crate::theme::SHIPPED[0];
                let r = crate::store::write_theme(dir, self.file(), spec)
                    .map_err(|n| std::io::Error::other(n.text));
                (r, crate::store::theme_text(spec).into_bytes())
            }
            Kind::Settings => {
                let s = crate::store::Settings::default();
                let r = s.write(dir);
                (r, s.text().unwrap().into_bytes())
            }
        }
    }
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn set_mode(p: &Path, m: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

/// What a plain create gives under this process's umask.
#[cfg(unix)]
fn plain_mode(dir: &Path) -> u32 {
    let p = dir.join("plain-probe");
    std::fs::write(&p, b"").unwrap();
    let m = mode(&p);
    std::fs::remove_file(&p).unwrap();
    m
}

/// Each kind against a missing destination and four modes: the mode after
/// the write and the content. A read-only destination is refused and kept.
#[cfg(unix)]
#[test]
fn modes_table() {
    let mut rows = 0;
    for kind in KINDS {
        for before in [None, Some(0o600), Some(0o644), Some(0o640), Some(0o444)] {
            let d = TempDir::new("modes");
            let plain = plain_mode(&d.0);
            let path = d.0.join(kind.file());
            if let Some(m) = before {
                std::fs::write(&path, b"old\n").unwrap();
                set_mode(&path, m);
            }
            let (r, new) = kind.write(&d.0);
            let row = format!(
                "{kind:?} {}",
                before.map_or("absent".into(), |m| format!("{m:o}"))
            );
            match before {
                Some(0o444) => {
                    assert!(r.is_err(), "{row}");
                    assert_eq!(std::fs::read(&path).unwrap(), b"old\n", "{row}");
                    assert_eq!(mode(&path), 0o444, "{row}");
                }
                _ => {
                    r.unwrap();
                    let want = match (before, kind) {
                        (Some(m), _) => m,
                        (None, Kind::Document) => plain,
                        (None, _) => 0o600,
                    };
                    assert_eq!(mode(&path), want, "{row}");
                    assert_eq!(std::fs::read(&path).unwrap(), new, "{row}");
                }
            }
            // Only the destination is left: no temporary file, no probe.
            assert_eq!(std::fs::read_dir(&d.0).unwrap().count(), 1, "{row}");
            rows += 1;
        }
    }
    assert_eq!(rows, 15);
}

#[cfg(unix)]
#[test]
fn temp_file_is_private_from_the_start() {
    let d = TempDir::new("private");
    let f = d.0.join("secret.c");
    std::fs::write(&f, b"old\n").unwrap();
    set_mode(&f, 0o644);
    for new in [New::Plain, New::Private] {
        TMP_MODES.with(|v| v.borrow_mut().clear());
        write(&f, b"new\n", new).unwrap();
        assert_eq!(TMP_MODES.with(|v| v.borrow().clone()), vec![0o600]);
    }
    assert_eq!(mode(&f), 0o644);
}

/// A link at a settings or theme file is replaced by a plain file with the
/// bits of a new one, and its target is untouched. A document keeps its
/// link: the target gets the new content and keeps its bits.
#[cfg(unix)]
#[test]
fn a_document_keeps_its_link_and_studio_files_do_not() {
    for kind in KINDS {
        let d = TempDir::new("link");
        let outside = d.0.join("outside.txt");
        std::fs::write(&outside, b"keep").unwrap();
        set_mode(&outside, 0o640);
        let path = d.0.join(kind.file());
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        let (r, new) = match kind {
            Kind::Document => {
                let mut doc = crate::document::Doc::open(1, &path, "main.c".into()).unwrap();
                doc.text = "int x;\n".into();
                (
                    doc.save().map_err(std::io::Error::other),
                    b"int x;\n".to_vec(),
                )
            }
            _ => kind.write(&d.0),
        };
        r.unwrap();
        if matches!(kind, Kind::Document) {
            assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
            assert_eq!(std::fs::read(&outside).unwrap(), new);
        } else {
            assert!(
                std::fs::symlink_metadata(&path).unwrap().is_file(),
                "{kind:?}"
            );
            assert_eq!(std::fs::read(&outside).unwrap(), b"keep", "{kind:?}");
            assert_eq!(std::fs::read(&path).unwrap(), new, "{kind:?}");
            assert_eq!(mode(&path), 0o600, "{kind:?}");
        }
        assert_eq!(mode(&outside), 0o640, "{kind:?}");
    }
}

/// A document's link loop fails and a dangling link is replaced.
#[cfg(unix)]
#[test]
fn document_link_guards() {
    let d = TempDir::new("guards");
    let a = d.0.join("a.c");
    let b = d.0.join("b.c");
    std::os::unix::fs::symlink(&b, &a).unwrap();
    std::os::unix::fs::symlink(&a, &b).unwrap();
    assert!(crate::document::tests::save_path(&a, b"x\n").is_err());
    let dangling = d.0.join("dangling.c");
    std::os::unix::fs::symlink(d.0.join("none.c"), &dangling).unwrap();
    crate::document::tests::save_path(&dangling, b"x\n").unwrap();
    assert!(std::fs::symlink_metadata(&dangling).unwrap().is_file());
    let folder = d.0.join("dir");
    std::fs::create_dir(&folder).unwrap();
    let to_dir = d.0.join("to-dir.c");
    std::os::unix::fs::symlink(&folder, &to_dir).unwrap();
    assert!(crate::document::tests::save_path(&to_dir, b"x\n").is_err());
    assert!(folder.is_dir());
}

/// A rename that fails leaves the destination as it was and no temporary
/// file behind.
#[cfg(unix)]
#[test]
fn failure_removes_the_temp_file() {
    let d = TempDir::new("fail");
    let f = d.0.join("main.c");
    std::fs::write(&f, b"old\n").unwrap();
    // A folder in the destination's place makes the rename fail after the
    // temporary file is written.
    let blocked = d.0.join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("inside"), b"").unwrap();
    assert!(write(&blocked, b"new\n", New::Private).is_err());
    assert!(write(&d.0.join("no/such/dir/f"), b"new\n", New::Private).is_err());
    assert_eq!(std::fs::read(&f).unwrap(), b"old\n");
    assert_eq!(std::fs::read_dir(&d.0).unwrap().count(), 2);
}

/// A read-only destination is refused on every system.
#[test]
fn read_only_is_refused() {
    let d = TempDir::new("ro");
    let f = d.0.join("settings.toml");
    std::fs::write(&f, b"old\n").unwrap();
    let mut p = std::fs::metadata(&f).unwrap().permissions();
    p.set_readonly(true);
    std::fs::set_permissions(&f, p).unwrap();
    assert!(write(&f, b"new\n", New::Private).is_err());
    assert_eq!(std::fs::read(&f).unwrap(), b"old\n");
    let mut p = std::fs::metadata(&f).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    p.set_readonly(false);
    std::fs::set_permissions(&f, p).unwrap();
}

/// Folders Studio makes are 0700; one that exists keeps its bits.
#[cfg(unix)]
#[test]
fn folders_studio_makes_are_private() {
    let d = TempDir::new("dirs");
    let settings = d.0.join("robco").join("studio");
    let themes = settings.join(crate::store::THEMES);
    crate::store::write_theme(&themes, "ember.toml", &crate::theme::SHIPPED[0]).unwrap();
    assert_eq!(mode(&d.0.join("robco")), 0o700);
    assert_eq!(mode(&settings), 0o700);
    assert_eq!(mode(&themes), 0o700);
    set_mode(&themes, 0o755);
    crate::store::Settings::default().write(&themes).unwrap();
    assert_eq!(mode(&themes), 0o755);
}
