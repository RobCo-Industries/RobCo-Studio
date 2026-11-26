// SPDX-License-Identifier: GPL-3.0-or-later

//! `robco new`, by "robco new" of RobCo-Core `docs/spec/STUDIO_CLI.md`, and
//! "The templates package", "templates.json" and "Tokens" of
//! `docs/spec/SDK_PACKAGE.md`.
//!
//! The program holds no template: every byte it writes comes from the
//! installed `templates` package. It makes the seven checks of the spec in
//! order and writes nothing until all have passed.
//!
//! A package comes from an index, so nothing in it may make the command
//! write outside the new project folder. Three rules hold that:
//!
//! - The package is checked whole by `studio_core::templates::validate`,
//!   the check the installer makes too: each template path follows the
//!   path rules of a manifest path, and a few more that keep it one plain
//!   file name per component on every system. Only the token rule of a
//!   file body stays here, since it needs the bytes.
//! - The package is read without following a link at the last component
//!   of a path: `templates.json`, `sdk.json` and each template file must
//!   be regular files, read up to a size limit, and a link under the
//!   template folder fails the package. Each file is opened with
//!   `O_NOFOLLOW` on Unix and `FILE_FLAG_OPEN_REPARSE_POINT` on Windows,
//!   and its type, size and bytes come from the open handle only.
//! - The project is written into a new folder that starts with a dot,
//!   beside the target, made by this run, so no link is in it. Each folder
//!   and file in it is created new. The folder is renamed to `NAME` at the
//!   end, and removed on any failure, so a failure leaves nothing.
//!
//! ## Limits
//!
//! Each check of a path and the act that follows are two steps, and
//! another process can change the path in between. What holds:
//!
//! - A package file swapped for a link after the check is not read: the
//!   open refuses a link at the last component.
//! - A folder of the package swapped for a link is still followed, by the
//!   open of a file under it and by the walk of the template folder. So a
//!   process that writes in the cache can make the command copy bytes from
//!   outside the package. Closing this needs each folder opened in turn
//!   from the package folder, each without following a link, as `openat`
//!   does on Unix. The cache belongs to the person who runs the command,
//!   and only a process that can already write there can do this; such a
//!   process can also change the package files themselves.
//! - The target is checked, then later removed if it was an empty folder
//!   and replaced by a rename. Removing a folder fails on a link and on a
//!   folder that is not empty, and a rename replaces a link itself, never
//!   what it points to. So a change in between makes the command fail or
//!   replace an empty folder or a link, and touches nothing outside.
//! - The temporary folder is made new, so it is never an older folder. A
//!   process that can write in the current folder can swap it or a folder
//!   in it for a link while files are written: a file is then created new
//!   outside the project, never over an existing file. Removing the
//!   temporary folder after a failure does not follow a link. Only a
//!   process that can already write in the person's own folder can do
//!   this, and closing it needs the same folder by folder opening.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use studio_core::install::{self, Cached};
use studio_core::manifest;
use studio_core::resolve::{TEMPLATES, Version, sdk_json};
use studio_core::templates::{self, Entry, Kind, Template};

use crate::Failure;
use crate::lines::{Out, Val};

/// The one token a template may hold.
const NAME_TOKEN: &[u8] = b"@NAME@";

/// What `robco new` was asked for.
pub struct Request<'a> {
    /// The template's `id`.
    pub template: &'a str,
    /// The project name, as given.
    pub name: &'a str,
    /// The current folder, where `NAME` is written.
    pub cwd: &'a Path,
}

/// Runs the command. The caller writes the error and the failed `result`
/// line from the returned failure.
pub fn run(out: &mut Out<'_>, cache: &Path, req: &Request<'_>) -> Result<(), Failure> {
    // 1. The name, as given.
    manifest::check_project_name(req.name).map_err(|e| Failure::new("new: name", e.detail()))?;

    // 2. The newest templates package in the cache.
    let pkg = newest_package(cache)?;

    // 3. Its sdk.json.
    let sdk = read_regular(
        &pkg.join("sdk.json"),
        install::MAX_MANIFEST,
        PACKAGE_MANIFEST,
    )?
    .ok_or_else(|| {
        Failure::new(
            PACKAGE_MANIFEST,
            format!(
                "sdk.json is missing from the installed package {}",
                pkg.display()
            ),
        )
    })?;
    let sdk = sdk_json::parse(&sdk).map_err(|e| Failure::of(&e))?;

    // 4. The whole package, by the check the installer makes too.
    let list = read_regular(
        &pkg.join(install::TEMPLATES_LIST),
        install::MAX_TEMPLATES_LIST,
        TEMPLATES_MANIFEST,
    )?;
    let entries = package_entries(&pkg, sdk.files.len())?;
    let list = templates::validate(&sdk, list.as_deref(), &entries, &templates::Limits::DEFAULT)
        .map_err(|e| invalid_package(&pkg, e))?;

    // 5. The template asked for.
    let Some(t) = list.iter().find(|t| t.id == req.template) else {
        let ids: Vec<&str> = list.iter().map(|t| t.id.as_str()).collect();
        return Err(Failure::new(
            "new: template",
            format!(
                "{} is not a template of templates {}: the package has {}",
                req.template,
                sdk.version,
                ids.join(", ")
            ),
        ));
    };

    // 6. Read every file, and check the tokens of each substitute file.
    let files = read_template(&pkg, t, req.name)?;

    // 7. The target folder.
    let target = req.cwd.join(req.name);
    let empty = target_state(&target, req.name)?;

    write_project(req.cwd, req.name, &files, empty)?;

    let mut written: Vec<String> = files.iter().map(|(p, _)| (*p).to_owned()).collect();
    written.sort();
    out.text(&format!(
        "wrote {} from {}, {} files",
        req.name,
        t.id,
        written.len()
    ));
    out.ok(
        "new",
        vec![
            ("template", Val::s(t.id.clone())),
            ("name", Val::s(req.name)),
            ("path", Val::s(req.name)),
            ("files", Val::Arr(written.into_iter().map(Val::s).collect())),
        ],
    );
    Ok(())
}

/// A package in the cache that [`templates::validate`] refuses. An older
/// tool may have installed it under a narrower check, and a reinstall
/// skips a finished install, so the line says how to replace it. No
/// command removes a package yet: the folder is deleted by hand.
fn invalid_package(pkg: &Path, e: templates::Error) -> Failure {
    if e.code == "templates: format" {
        return Failure::new(e.code, e.detail);
    }
    Failure::new(
        e.code,
        format!(
            "the installed package {p} is not valid: {d}. No command removes it yet: delete the folder {p}, then run robco sdk install templates",
            p = pkg.display(),
            d = e.detail
        ),
    )
}

/// Step 6: the bytes of each file of `t`, with `@NAME@` replaced in each
/// substitute file. Kept here, not in [`templates::validate`], because it
/// needs the bytes: the token rule of a file body. Each file is read up to
/// the size the check saw, so the limits it checked still hold.
fn read_template<'t>(
    pkg: &Path,
    t: &'t Template,
    name: &str,
) -> Result<Vec<(&'t str, Vec<u8>)>, Failure> {
    let mut files = Vec::with_capacity(t.files.len());
    for f in &t.files {
        let path = file_path(pkg, &t.id, &f.path);
        let changed = || {
            Failure::new(
                TEMPLATES_MANIFEST,
                format!("{}/{}: changed while the command read it", t.id, f.path),
            )
        };
        let bytes = read_regular(&path, f.size, TEMPLATES_MANIFEST)?.ok_or_else(changed)?;
        if bytes.len() as u64 != f.size {
            return Err(changed());
        }
        let bytes = if f.substitute {
            substitute(&bytes, name.as_bytes()).map_err(|tok| {
                Failure::new("templates: token", format!("{}/{}: {tok}", t.id, f.path))
            })?
        } else {
            bytes
        };
        files.push((f.path.as_str(), bytes));
    }
    Ok(files)
}

/// Every file and folder of the package folder, as `/` paths from its
/// root, without following a link. The record of the install is left out:
/// it is the cache's, not the package's. A name that is not UTF-8 fails.
/// Far more files than `sdk.json` lists fails at once, so a large tree is
/// not walked; a few more are left to the check, which names them.
fn package_entries(pkg: &Path, listed: usize) -> Result<Vec<Entry>, Failure> {
    let mut out = Vec::new();
    let mut files = 0usize;
    let mut stack = vec![(pkg.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| read_failure(&dir, &e))? {
            let entry = entry.map_err(|e| read_failure(&dir, &e))?;
            let name = entry.file_name().into_string().map_err(|n| {
                Failure::new(
                    TEMPLATES_MANIFEST,
                    format!("{rel}{}: not a UTF-8 name", n.to_string_lossy()),
                )
            })?;
            let path = format!("{rel}{name}");
            if path == install::RECORD {
                continue;
            }
            let kind = entry
                .file_type()
                .map_err(|e| read_failure(&entry.path(), &e))?;
            if kind.is_dir() {
                stack.push((entry.path(), format!("{path}/")));
                out.push(Entry::folder(path));
            } else if kind.is_file() {
                let md = entry
                    .metadata()
                    .map_err(|e| read_failure(&entry.path(), &e))?;
                files += 1;
                if files as u64 > (listed as u64).saturating_add(1 + install::MAX_TEMPLATE_FILES) {
                    return Err(Failure::new(
                        TEMPLATES_MANIFEST,
                        format!(
                            "{}: holds far more files than sdk.json lists",
                            pkg.display()
                        ),
                    ));
                }
                out.push(Entry::file(path, md.len()));
            } else {
                out.push(Entry {
                    path,
                    kind: Kind::Other,
                    size: 0,
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn read_failure(path: &Path, e: &std::io::Error) -> Failure {
    Failure::new("io: read", format!("{}: {}", path.display(), e.kind()))
}

fn write_failure(path: &Path, e: &std::io::Error) -> Failure {
    Failure::new("io: write", format!("{}: {}", path.display(), e.kind()))
}

/// Step 2: the highest version in `<cache>/packages/templates/` whose
/// folder holds a finished install. A folder whose name is not a version,
/// or whose install did not end, is no package.
fn newest_package(cache: &Path) -> Result<PathBuf, Failure> {
    let none = || {
        Failure::new(
            "templates: none",
            format!(
                "{} holds no templates package: run robco sdk install templates",
                cache.display()
            ),
        )
    };
    let dir = cache.join("packages").join(TEMPLATES);
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        // Only a folder that is truly absent, or a file where a folder
        // belongs, holds no package. A refusal is not an absence.
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Err(none());
        }
        Err(e) => return Err(read_failure(&dir, &e)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| read_failure(&dir, &e))?;
        names.push(entry.file_name());
    }
    let v = choose_newest(
        names.iter().filter_map(|n| n.to_str()),
        |v| match install::package_dir(cache, TEMPLATES, &v.to_string()) {
            None => Ok(false),
            Some(folder) => match install::cached(&folder).map_err(|e| Failure::of(&e))? {
                Cached::Holds(_) => Ok(true),
                Cached::Missing | Cached::Unfinished => Ok(false),
            },
        },
    )?
    .ok_or_else(none)?;
    install::package_dir(cache, TEMPLATES, &v.to_string())
        .ok_or_else(|| Failure::new("internal", "a cached version has no folder name"))
}

/// The version step 2 takes, from the folder names of
/// `<cache>/packages/templates/`. A name that is not `X.Y.Z` is ignored:
/// the spec gives the cache no rule for it. The versions are walked from
/// the highest, by the order of [`Version`], and `finished` tells whether
/// a version holds a finished install. The first that does is the answer,
/// and no lower one is read, so an old record that cannot be read never
/// hides a newer install. A record that cannot be read above every
/// finished install fails. The order of `names` does not matter.
fn choose_newest<'a>(
    names: impl IntoIterator<Item = &'a str>,
    mut finished: impl FnMut(Version) -> Result<bool, Failure>,
) -> Result<Option<Version>, Failure> {
    let mut versions: Vec<Version> = names.into_iter().filter_map(Version::parse).collect();
    versions.sort_unstable_by(|a, b| b.cmp(a));
    versions.dedup();
    for v in versions {
        if finished(v)? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

/// The code of a bad `sdk.json`, by "Reading sdk.json".
const PACKAGE_MANIFEST: &str = "package: manifest";

/// The code of a bad `templates.json` or template file, by "templates.json".
const TEMPLATES_MANIFEST: &str = "templates: manifest";

/// Reads a regular file of the package, up to `limit` bytes, without
/// following a link at its last component. `None` when nothing is there.
/// Something that is not a regular file, a link included, or a file over
/// the limit fails the package with `code`, the code of that file, naming
/// the path. A host that refuses is `io: read`, whatever the file.
///
/// The look at the path first only picks the error a person sees. What is
/// read is decided on the handle [`open_no_follow`] returns: its type, its
/// size and its bytes, so a file swapped for a link after the look is not
/// read through the link.
fn read_regular(path: &Path, limit: u64, code: &'static str) -> Result<Option<Vec<u8>>, Failure> {
    let bad = |why: &str| Failure::new(code, format!("{}: {why}", path.display()));
    match fs::symlink_metadata(path) {
        Ok(md) if md.file_type().is_file() => {}
        Ok(_) => return Err(bad("not a regular file")),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(read_failure(path, &e)),
    }
    let file = match open_no_follow(path) {
        Ok(f) => f,
        Err(e) if is_link_refused(&e) => return Err(bad("a link, not followed")),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(bad("gone while the command read it"));
        }
        Err(e) => return Err(read_failure(path, &e)),
    };
    let md = file.metadata().map_err(|e| read_failure(path, &e))?;
    if !md.is_file() || md.file_type().is_symlink() {
        return Err(bad("not a regular file"));
    }
    if md.len() > limit {
        return Err(bad(&format!("larger than {limit} bytes")));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| read_failure(path, &e))?;
    if bytes.len() as u64 > limit {
        return Err(bad(&format!("larger than {limit} bytes")));
    }
    Ok(Some(bytes))
}

/// Opens `path` to read, without following a link at its last component.
///
/// On Unix it passes `O_NOFOLLOW`, so a link fails the open with `ELOOP`,
/// and `O_NONBLOCK`, so a named pipe opens at once instead of waiting for a
/// writer. The caller then refuses the pipe by its type.
#[cfg(unix)]
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// Opens `path` to read, without following a link at its last component.
///
/// On Windows it passes `FILE_FLAG_OPEN_REPARSE_POINT`, so a symbolic link
/// or a junction opens as itself, not as its target. The caller then
/// refuses it by its type, read from the handle.
#[cfg(windows)]
fn open_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_FLAG_OPEN_REPARSE_POINT` of the Windows API.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Whether the open failed because the last component is a link.
#[cfg(unix)]
fn is_link_refused(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ELOOP)
}

/// Whether the open failed because the last component is a link. On
/// Windows a link opens as itself and is refused by its type.
#[cfg(windows)]
fn is_link_refused(_: &std::io::Error) -> bool {
    false
}

/// The path of a template file in the package, from its checked `/` path.
fn file_path(pkg: &Path, id: &str, rel: &str) -> PathBuf {
    let mut p = pkg.join(id);
    p.extend(rel.split('/'));
    p
}

/// Replaces every `@NAME@` by `name` and changes nothing else. Any other
/// token is the error, as text.
fn substitute(b: &[u8], name: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(b.len());
    let mut at = 0;
    for r in templates::tokens(b) {
        let tok = &b[r.clone()];
        if tok != NAME_TOKEN {
            return Err(format!(
                "holds the token {}, and only @NAME@ is known",
                String::from_utf8_lossy(tok)
            ));
        }
        out.extend_from_slice(&b[at..r.start]);
        out.extend_from_slice(name);
        at = r.end;
    }
    out.extend_from_slice(&b[at..]);
    Ok(out)
}

/// Step 7: `NAME` does not exist, or is an empty folder. Returns whether
/// it is an empty folder. A link, a file or a folder with anything in it
/// is `new: exists`.
fn target_state(target: &Path, name: &str) -> Result<bool, Failure> {
    let exists =
        |why: &str| Failure::new("new: exists", format!("{name} in the current folder {why}"));
    match fs::symlink_metadata(target) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(read_failure(target, &e)),
        Ok(md) if md.file_type().is_dir() => {
            let it = fs::read_dir(target).map_err(|e| read_failure(target, &e))?;
            match is_empty(it) {
                Ok(true) => Ok(true),
                Ok(false) => Err(exists("is a folder that is not empty")),
                Err(e) => Err(read_failure(target, &e)),
            }
        }
        Ok(_) => Err(exists("exists and is not a folder")),
    }
}

/// Whether a folder listing holds no entry, by its first item. An item the
/// system fails to read is an error, never an entry.
fn is_empty<T>(mut items: impl Iterator<Item = std::io::Result<T>>) -> std::io::Result<bool> {
    match items.next() {
        None => Ok(true),
        Some(Ok(_)) => Ok(false),
        Some(Err(e)) => Err(e),
    }
}

/// Makes a new folder `.NAME.robco-new-<pid>[-n]` in `cwd`. Made new, so
/// it is never a link or an older folder.
fn make_temp(cwd: &Path, name: &str) -> Result<PathBuf, Failure> {
    let pid = std::process::id();
    let mut last = None;
    for n in 0..100 {
        let tmp = if n == 0 {
            cwd.join(format!(".{name}.robco-new-{pid}"))
        } else {
            cwd.join(format!(".{name}.robco-new-{pid}-{n}"))
        };
        match fs::create_dir(&tmp) {
            Ok(()) => return Ok(tmp),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => last = Some((tmp, e)),
            Err(e) => return Err(write_failure(&tmp, &e)),
        }
    }
    match last {
        Some((tmp, e)) => Err(write_failure(&tmp, &e)),
        None => Err(Failure::new("internal", "no temporary folder was tried")),
    }
}

/// Writes the files into a new folder beside the target, then renames it to
/// `NAME`. Any failure removes the new folder and is `io: write`.
fn write_project(
    cwd: &Path,
    name: &str,
    files: &[(&str, Vec<u8>)],
    empty: bool,
) -> Result<(), Failure> {
    let tmp = make_temp(cwd, name)?;
    let target = cwd.join(name);
    let done = fill(&tmp, files).and_then(|()| {
        if empty {
            fs::remove_dir(&target).map_err(|e| write_failure(&target, &e))?;
        }
        fs::rename(&tmp, &target).map_err(|e| {
            if empty {
                // Put back the empty folder the person had.
                let _ = fs::create_dir(&target);
            }
            write_failure(&target, &e)
        })
    });
    if done.is_err() {
        // `remove_dir_all` does not follow links.
        let _ = fs::remove_dir_all(&tmp);
    }
    done
}

/// Writes each file under `root`, with its folders. Every folder and file
/// is created new, so nothing is written through a link.
fn fill(root: &Path, files: &[(&str, Vec<u8>)]) -> Result<(), Failure> {
    for (rel, bytes) in files {
        let parts: Vec<&str> = rel.split('/').collect();
        let Some((file, dirs)) = parts.split_last() else {
            return Err(Failure::new("internal", "an empty template path"));
        };
        let mut path = root.to_path_buf();
        for d in dirs {
            path.push(d);
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::AlreadyExists => match fs::symlink_metadata(&path)
                {
                    Ok(md) if md.file_type().is_dir() => {}
                    Ok(_) => {
                        return Err(Failure::new(
                            "io: write",
                            format!("{}: a link or not a folder, not followed", path.display()),
                        ));
                    }
                    Err(e) => return Err(write_failure(&path, &e)),
                },
                Err(e) => return Err(write_failure(&path, &e)),
            }
        }
        path.push(file);
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| write_failure(&path, &e))?;
        f.write_all(bytes)
            .and_then(|()| f.sync_all())
            .map_err(|e| write_failure(&path, &e))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests {
    #[test]
    fn is_empty_tells_an_error_from_an_entry() {
        use std::io::{Error, ErrorKind::PermissionDenied};
        let err = || Error::from(PermissionDenied);
        assert!(is_empty(Vec::<std::io::Result<()>>::new().into_iter()).unwrap());
        assert!(!is_empty(vec![Ok(())].into_iter()).unwrap());
        let e = is_empty(vec![Err::<(), _>(err())].into_iter()).unwrap_err();
        assert_eq!(e.kind(), PermissionDenied);
        let e = is_empty(vec![Err(err()), Ok(())].into_iter()).unwrap_err();
        assert_eq!(e.kind(), PermissionDenied);
    }

    use super::*;

    /// Every order of `items`.
    fn orders<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut out = Vec::new();
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let first = rest.remove(i);
            for mut o in orders(&rest) {
                o.insert(0, first.clone());
                out.push(o);
            }
        }
        out
    }

    #[test]
    fn choose_newest_does_not_depend_on_the_listing_order() {
        let v = |s| Version::parse(s).unwrap();
        // 1.0.0 cannot be read, 1.2.0 is unfinished, 1.10.0 is finished.
        let state = |x: Version| {
            if x == v("1.0.0") {
                Err(Failure::new("io: read", "1.0.0/.robco-archive"))
            } else {
                Ok(x == v("1.10.0") || x == v("1.9.0"))
            }
        };
        let names = ["1.0.0", "1.2.0", "1.10.0", "1.9.0", "notes", "1.03.0"];
        for o in orders(&names) {
            let got = choose_newest(o.iter().copied(), state).unwrap();
            assert_eq!(got, Some(v("1.10.0")), "{o:?}");
        }
        // An unreadable record above every finished install fails, in
        // every order; below one it is never read.
        for o in orders(&["1.0.0", "0.9.0", "x"]) {
            let e = choose_newest(o.iter().copied(), |x| {
                if x == v("1.0.0") {
                    Err(Failure::new("io: read", "r"))
                } else {
                    Ok(true)
                }
            })
            .unwrap_err();
            assert_eq!(e.code, "io: read", "{o:?}");
        }
        for o in orders(&["1.2.0", "notes"]) {
            assert_eq!(
                choose_newest(o.iter().copied(), |_| Ok(false)).unwrap(),
                None
            );
        }
    }

    #[test]
    fn only_name_is_replaced_and_nothing_else_changes() {
        let b = b"a @NAME@\r\nb@NAME@@NAME@ x@y.z @@ @a@ @NAME";
        assert_eq!(
            substitute(b, b"PIPES").unwrap(),
            b"a PIPES\r\nbPIPESPIPES x@y.z @@ @a@ @NAME".to_vec()
        );
    }

    #[test]
    fn another_token_is_refused_and_named() {
        let e = substitute(b"x @TITLE@ y", b"P").unwrap_err();
        assert!(e.contains("@TITLE@"), "{e}");
        assert!(substitute(b"@NAME_X@", b"P").is_err());
    }

    #[test]
    fn a_failure_part_way_leaves_nothing() {
        let cwd = std::env::temp_dir().join(format!("robco-unit-new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cwd);
        fs::create_dir_all(cwd.join("EMPTY")).unwrap();
        // The second file needs a folder where the first is a file.
        let files = [("a", b"1".to_vec()), ("a/b", b"2".to_vec())];
        for (name, empty) in [("PIPES", false), ("EMPTY", true)] {
            let e = write_project(&cwd, name, &files, empty).unwrap_err();
            assert_eq!(e.code, "io: write");
        }
        let mut left: Vec<String> = fs::read_dir(&cwd)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        // The empty folder the person had is kept.
        assert_eq!(left, vec!["EMPTY".to_owned()]);
        write_project(&cwd, "EMPTY", &[("a/b", b"2".to_vec())], true).unwrap();
        assert_eq!(fs::read(cwd.join("EMPTY/a/b")).unwrap(), b"2");
        let _ = fs::remove_dir_all(&cwd);
    }

    /// A fresh test folder under the system's temporary folder.
    #[cfg(unix)]
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("robco-unit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn the_opener_refuses_a_link_that_a_plain_open_follows() {
        let d = scratch("open-link");
        fs::write(d.join("real"), b"outside").unwrap();
        std::os::unix::fs::symlink(d.join("real"), d.join("link")).unwrap();
        assert!(File::open(d.join("link")).is_ok());
        let e = open_no_follow(&d.join("link")).unwrap_err();
        assert!(is_link_refused(&e), "{e}");
        assert!(open_no_follow(&d.join("real")).is_ok());
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn the_opener_returns_at_once_on_a_named_pipe() {
        use std::os::unix::fs::OpenOptionsExt;
        let d = scratch("open-fifo");
        let fifo = d.join("pipe");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        let (tx, rx) = std::sync::mpsc::channel();
        let at = fifo.clone();
        std::thread::spawn(move || {
            let _ = tx.send(open_no_follow(&at).map(|f| f.metadata().unwrap().is_file()));
        });
        let got = rx.recv_timeout(std::time::Duration::from_secs(10));
        // Unblock a thread still in the open, so the test process can end.
        let _ = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo);
        assert!(matches!(got, Ok(Ok(false))), "{got:?}");
        let _ = fs::remove_dir_all(&d);
    }
}
