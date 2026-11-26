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
//! - Each template path follows the path rules of a manifest path, and a
//!   few more that keep it one plain file name per component on every
//!   system: no `:`, no back slash, no component that starts with `-`, no
//!   character Windows refuses in a name, no name Windows keeps for a
//!   device such as `CON` or `NUL`, and no component that ends with a dot
//!   or a space, which Windows drops.
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

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use studio_core::install::{self, Cached};
use studio_core::manifest;
use studio_core::resolve::{TEMPLATES, Version, case_clash, sdk_json};

use crate::Failure;
use crate::lines::{Out, Val};

/// The `templates.json` `format` this tool reads.
const FORMAT: u64 = 1;

/// The paths of a project folder that only the tool writes, by "A project
/// folder" of `docs/spec/STUDIO_PROJECT.md`: a template that lists one, or
/// a path under one, is refused. Compared in lower case, so a case-blind
/// file system cannot map another spelling onto them.
const RESERVED: &[&str] = &["robco.lock", "build"];

/// The file every template holds at its root: it makes the folder a
/// project.
const PROJECT_MANIFEST: &str = "robco.toml";

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

/// A template of `templates.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Template {
    id: String,
    files: Vec<TemplateFile>,
}

/// A file of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TemplateFile {
    path: String,
    substitute: bool,
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
            "package: manifest",
            format!(
                "sdk.json is missing from the installed package {}",
                pkg.display()
            ),
        )
    })?;
    let sdk = sdk_json::parse(&sdk).map_err(|e| Failure::of(&e))?;
    if sdk.name != TEMPLATES {
        return Err(Failure::new(
            "package: manifest",
            format!(
                "{}: sdk.json names the package {}, not templates",
                pkg.display(),
                sdk.name
            ),
        ));
    }

    // 4. templates.json, and the files each template lists.
    let list = read_regular(
        &pkg.join("templates.json"),
        install::MAX_TEMPLATES_LIST,
        TEMPLATES_MANIFEST,
    )?
    .ok_or_else(|| {
        Failure::new(
            "templates: manifest",
            format!("templates.json is missing from {}", pkg.display()),
        )
    })?;
    let templates = parse_templates(&list)?;
    check_root(&pkg, &templates)?;
    for t in &templates {
        check_folder(&pkg, t)?;
    }

    // 5. The template asked for.
    let Some(t) = templates.iter().find(|t| t.id == req.template) else {
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
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
    let mut files = Vec::with_capacity(t.files.len());
    let mut total: u64 = 0;
    for f in &t.files {
        let path = file_path(&pkg, &t.id, &f.path);
        let bytes = read_regular(&path, install::MAX_TEMPLATE_FILE, TEMPLATES_MANIFEST)?
            .ok_or_else(|| {
                Failure::new(
                    "templates: manifest",
                    format!("{}/{}: gone while the command read it", t.id, f.path),
                )
            })?;
        total = total.saturating_add(bytes.len() as u64);
        if total > install::MAX_TEMPLATE_BYTES {
            return Err(Failure::new(
                "templates: manifest",
                format!(
                    "{}/: more than {} bytes in all",
                    t.id,
                    install::MAX_TEMPLATE_BYTES
                ),
            ));
        }
        let bytes = if f.substitute {
            substitute(&bytes, req.name.as_bytes()).map_err(|tok| {
                Failure::new("templates: token", format!("{}/{}: {tok}", t.id, f.path))
            })?
        } else {
            bytes
        };
        files.push((f.path.as_str(), bytes));
    }

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

fn manifest_error(detail: impl Into<String>) -> Failure {
    Failure::new(
        "templates: manifest",
        format!("templates.json: {}", detail.into()),
    )
}

/// The keys of each object of `templates.json`.
const TOP_KEYS: &[&str] = &["format", "templates"];
const TEMPLATE_KEYS: &[&str] = &["id", "title", "description", "kind", "files"];
const FILE_KEYS: &[&str] = &["path", "substitute"];

/// Step 4 for the text of `templates.json`, in the order of "Order of
/// checks": it parses, `format`, unknown fields, then each field in the
/// order of the tables. The files on disk are [`check_folder`]'s.
fn parse_templates(bytes: &[u8]) -> Result<Vec<Template>, Failure> {
    let text = std::str::from_utf8(bytes).map_err(|_| manifest_error("the file is not UTF-8"))?;
    let root: Value = serde_json::from_str(text)
        .map_err(|e| manifest_error(format!("the file is not JSON: {e}")))?;
    let o = root
        .as_object()
        .ok_or_else(|| manifest_error("the top value is not an object"))?;

    match o.get("format").map(Value::as_u64) {
        None => {
            return Err(Failure::new(
                "templates: format",
                "templates.json: format is missing",
            ));
        }
        Some(Some(n)) if n > FORMAT => {
            return Err(Failure::new(
                "templates: format",
                format!(
                    "templates.json has format {n} and this tool reads {FORMAT}: install a newer tool"
                ),
            ));
        }
        Some(Some(n)) if n >= 1 => {}
        Some(_) => {
            return Err(Failure::new(
                "templates: format",
                "templates.json: format is not a positive integer",
            ));
        }
    }

    unknown_fields(o)?;

    let list = match o.get("templates") {
        None => return Err(manifest_error("templates is missing")),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(manifest_error("templates is not an array")),
    };
    if list.is_empty() {
        return Err(manifest_error("templates is empty"));
    }
    let mut out: Vec<Template> = Vec::with_capacity(list.len());
    for (i, v) in list.iter().enumerate() {
        let at = format!("templates[{i}]");
        let t = template(&at, v)?;
        if let Some(prev) = out.last()
            && prev.id.as_bytes() >= t.id.as_bytes()
        {
            return Err(manifest_error(format!(
                "{at}.id {} {} {}: the list is sorted by id with no id twice",
                t.id,
                if prev.id == t.id {
                    "repeats"
                } else {
                    "comes after"
                },
                prev.id
            )));
        }
        out.push(t);
    }
    Ok(out)
}

/// Step 4 of "Order of checks": a field this spec does not define, at any
/// level, before any value is checked. A value of the wrong type is left
/// to step 5.
fn unknown_fields(o: &Map<String, Value>) -> Result<(), Failure> {
    let check = |at: &str, o: &Map<String, Value>, known: &[&str]| match o
        .keys()
        .find(|k| !known.contains(&k.as_str()))
    {
        Some(k) => Err(manifest_error(format!(
            "{at}{k} is not a field of this format"
        ))),
        None => Ok(()),
    };
    check("", o, TOP_KEYS)?;
    let Some(list) = o.get("templates").and_then(Value::as_array) else {
        return Ok(());
    };
    for (i, t) in list.iter().enumerate() {
        let Some(t) = t.as_object() else { continue };
        check(&format!("templates[{i}]."), t, TEMPLATE_KEYS)?;
        let Some(files) = t.get("files").and_then(Value::as_array) else {
            continue;
        };
        for (j, f) in files.iter().enumerate() {
            if let Some(f) = f.as_object() {
                check(&format!("templates[{i}].files[{j}]."), f, FILE_KEYS)?;
            }
        }
    }
    Ok(())
}

fn string<'a>(o: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a str, Failure> {
    match o.get(key) {
        None => Err(manifest_error(format!("{at}.{key} is missing"))),
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(manifest_error(format!("{at}.{key} is not a string"))),
    }
}

/// An ASCII text of 1 to `max` bytes, with no control character.
fn ascii_text(o: &Map<String, Value>, at: &str, key: &str, max: usize) -> Result<(), Failure> {
    let s = string(o, at, key)?;
    if s.is_empty() || s.len() > max || !s.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return Err(manifest_error(format!(
            "{at}.{key} is not ASCII text of 1 to {max} bytes"
        )));
    }
    Ok(())
}

fn is_template_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// One template, its fields in the order of the table.
fn template(at: &str, v: &Value) -> Result<Template, Failure> {
    let o = v
        .as_object()
        .ok_or_else(|| manifest_error(format!("{at} is not an object")))?;
    let id = string(o, at, "id")?;
    if !is_template_id(id) {
        return Err(manifest_error(format!(
            "{at}.id {id:?}: 1 to 32 of a to z, 0 to 9 and -, starting with a letter"
        )));
    }
    ascii_text(o, at, "title", 64)?;
    ascii_text(o, at, "description", 200)?;
    let kind = string(o, at, "kind")?;
    if kind != "holotape" {
        return Err(manifest_error(format!(
            "{at}.kind {kind:?}: this tool writes holotape only"
        )));
    }
    let list = match o.get("files") {
        None => return Err(manifest_error(format!("{at}.files is missing"))),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(manifest_error(format!("{at}.files is not an array"))),
    };
    let mut files: Vec<TemplateFile> = Vec::with_capacity(list.len());
    for (j, f) in list.iter().enumerate() {
        let fat = format!("{at}.files[{j}]");
        let fo = f
            .as_object()
            .ok_or_else(|| manifest_error(format!("{fat} is not an object")))?;
        let path = string(fo, &fat, "path")?;
        let substitute = match fo.get("substitute") {
            None => return Err(manifest_error(format!("{fat}.substitute is missing"))),
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(manifest_error(format!("{fat}.substitute is not a boolean"))),
        };
        check_template_path(&format!("{fat}.path"), path)?;
        check_not_reserved(&format!("{fat}.path"), path)?;
        if let Some(prev) = files.last()
            && prev.path.as_bytes() >= path.as_bytes()
        {
            return Err(manifest_error(format!(
                "{fat}.path {path}: the files are sorted by path with no path twice"
            )));
        }
        files.push(TemplateFile {
            path: path.to_owned(),
            substitute,
        });
    }
    // No two paths may differ only in case in any part, the folders above
    // them included, by the rule the installer keeps for archive entries.
    if let Some((a, b)) = case_clash(files.iter().map(|f| (f.path.as_str(), true))) {
        return Err(manifest_error(format!(
            "{at}.files: {b} and {a} of {id} differ only in case"
        )));
    }
    if !files.iter().any(|f| f.path == PROJECT_MANIFEST) {
        return Err(manifest_error(format!(
            "{at}.files: {id} lists no {PROJECT_MANIFEST} at its root"
        )));
    }
    Ok(Template {
        id: id.to_owned(),
        files,
    })
}

/// A path whose first component, in lower case, is a path of [`RESERVED`]:
/// the file itself or anything under it.
fn check_not_reserved(key: &str, path: &str) -> Result<(), Failure> {
    let first = path.split('/').next().unwrap_or(path).to_lowercase();
    if RESERVED.contains(&first.as_str()) {
        return Err(manifest_error(format!(
            "{key}: {path:?}: only the tool writes {first} in a project"
        )));
    }
    Ok(())
}

/// Names Windows keeps for devices, in any case and with any extension.
const DEVICES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// The path rules of a manifest path, then the rules that keep each
/// component one plain file name on every system, then no token.
fn check_template_path(key: &str, path: &str) -> Result<(), Failure> {
    manifest::check_relative_path(key, path).map_err(|e| manifest_error(e.detail()))?;
    let bad = |why: &str| Err(manifest_error(format!("{key}: {path:?}: {why}")));
    if let Some(c) = path
        .chars()
        .find(|c| c.is_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return bad(&format!("holds {c:?}, which a file name may not hold"));
    }
    for part in path.split('/') {
        if part.starts_with('-') {
            return bad("a component starts with -");
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return bad("a component ends with a dot or a space");
        }
        let stem = part.split('.').next().unwrap_or(part).trim_end();
        if DEVICES.contains(&stem.to_ascii_lowercase().as_str()) {
            return bad(&format!("{part} is a device name on Windows"));
        }
    }
    if !tokens(path.as_bytes()).is_empty() {
        return bad("a path holds no token");
    }
    Ok(())
}

/// Step 4 on disk: `<id>/` is a folder, each listed file is a regular file
/// there, and it holds nothing else. Nothing is followed through a link.
fn check_folder(pkg: &Path, t: &Template) -> Result<(), Failure> {
    let bad = |d: String| Failure::new("templates: manifest", d);
    let root = pkg.join(&t.id);
    match fs::symlink_metadata(&root) {
        Ok(md) if md.file_type().is_dir() => {}
        Ok(_) => return Err(bad(format!("{}/: not a folder", t.id))),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(bad(format!("{}/: the package does not hold it", t.id)));
        }
        Err(e) => return Err(read_failure(&root, &e)),
    }
    let mut held = BTreeSet::new();
    walk(&root, "", &t.id, &mut held)?;
    for f in &t.files {
        if !held.contains(&f.path) {
            return Err(bad(format!(
                "{}/{}: listed, and the package does not hold it",
                t.id, f.path
            )));
        }
    }
    if let Some(extra) = held.iter().find(|h| !t.files.iter().any(|f| &&f.path == h)) {
        return Err(bad(format!(
            "{}/{extra}: the package holds it, and files does not list it",
            t.id
        )));
    }
    Ok(())
}

/// "Layout of templates": the package root and its licences folder hold
/// only what [`install::templates_layout_allows`] allows, the same rule
/// the installer applies, and every licence file is there. A template
/// folder that `templates.json` does not list is refused here; a listed
/// one is checked by [`check_folder`].
fn check_root(pkg: &Path, templates: &[Template]) -> Result<(), Failure> {
    let layout_error = |d: String| Failure::new(TEMPLATES_MANIFEST, d);
    let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
    let entries = |dir: &Path, rel: &str| -> Result<Vec<(String, bool)>, Failure> {
        let mut v = Vec::new();
        for entry in fs::read_dir(dir).map_err(|e| read_failure(dir, &e))? {
            let entry = entry.map_err(|e| read_failure(dir, &e))?;
            let name = entry.file_name().into_string().map_err(|n| {
                layout_error(format!("{rel}{}: not a UTF-8 name", n.to_string_lossy()))
            })?;
            let kind = entry
                .file_type()
                .map_err(|e| read_failure(&entry.path(), &e))?;
            if !kind.is_dir() && !kind.is_file() {
                return Err(layout_error(format!(
                    "{rel}{name}: a link or not a regular file"
                )));
            }
            v.push((format!("{rel}{name}"), kind.is_dir()));
        }
        Ok(v)
    };
    let refuse = |name: &str| {
        layout_error(format!(
            "{name}: the layout of a templates package does not hold it"
        ))
    };
    let mut licences = false;
    for (name, folder) in entries(pkg, "")? {
        if !install::templates_layout_allows(&name, folder, &ids, true) {
            return Err(refuse(&name));
        }
        licences |= folder && name == install::TEMPLATES_LICENCES_DIR;
    }
    let dir = install::TEMPLATES_LICENCES_DIR;
    let mut held = BTreeSet::new();
    if licences {
        for (name, folder) in entries(&pkg.join(dir), &format!("{dir}/"))? {
            if !install::templates_layout_allows(&name, folder, &ids, true) {
                return Err(refuse(&name));
            }
            held.insert(name);
        }
    }
    for l in install::TEMPLATES_LICENCES {
        let p = format!("{dir}/{l}");
        if !held.contains(&p) {
            return Err(layout_error(format!(
                "{p}: a templates package must hold it"
            )));
        }
    }
    Ok(())
}

/// The regular files under `dir`, as `/` paths. A link, a name that is not
/// UTF-8 or any other kind of entry fails the package.
fn walk(dir: &Path, rel: &str, id: &str, held: &mut BTreeSet<String>) -> Result<(), Failure> {
    let entries = fs::read_dir(dir).map_err(|e| read_failure(dir, &e))?;
    for entry in entries {
        let entry = entry.map_err(|e| read_failure(dir, &e))?;
        let name = entry.file_name().into_string().map_err(|n| {
            Failure::new(
                "templates: manifest",
                format!("{id}/{rel}{}: not a UTF-8 name", n.to_string_lossy()),
            )
        })?;
        let path = format!("{rel}{name}");
        let kind = entry
            .file_type()
            .map_err(|e| read_failure(&entry.path(), &e))?;
        if kind.is_dir() {
            walk(&entry.path(), &format!("{path}/"), id, held)?;
        } else if kind.is_file() {
            held.insert(path);
        } else {
            return Err(Failure::new(
                "templates: manifest",
                format!("{id}/{path}: a link or not a regular file"),
            ));
        }
        if held.len() as u64 > install::MAX_TEMPLATE_FILES {
            return Err(Failure::new(
                "templates: manifest",
                format!("{id}/: more than {} files", install::MAX_TEMPLATE_FILES),
            ));
        }
    }
    Ok(())
}

/// The tokens of `b`, as ranges: `@`, one or more of `A` to `Z` and `_`,
/// then `@`, read from the left.
fn tokens(b: &[u8]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'@' {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_uppercase() || b[j] == b'_') {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b'@' {
                out.push(i..j + 1);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Replaces every `@NAME@` by `name` and changes nothing else. Any other
/// token is the error, as text.
fn substitute(b: &[u8], name: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(b.len());
    let mut at = 0;
    for r in tokens(b) {
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
    fn template_paths_stay_one_plain_name_per_component() {
        for ok in [
            "robco.toml",
            "src/main.c",
            "tests/golden/start/ready.png",
            "a-b/c.d",
        ] {
            assert!(check_template_path("k", ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/etc/x",
            "../x",
            "a/../x",
            "a/./x",
            "a//x",
            "a\\x",
            "C:x",
            "c:/x",
            "a:b",
            "src/ab:c",
            "-rf",
            "src/-x.c",
            "con",
            "src/NUL.c",
            "Com1.txt",
            "lpt9",
            "a./b",
            "a /b",
            "a\u{1}b",
            "a*b",
            "a?b",
            "src/@NAME@.c",
        ] {
            let e = check_template_path("k", bad).unwrap_err();
            assert_eq!(e.code, "templates: manifest", "{bad}");
        }
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

    #[test]
    fn reserved_paths_are_refused_in_any_case() {
        for bad in [
            "robco.lock",
            "ROBCO.LOCK",
            "Robco.Lock/x",
            "build/x",
            "BUILD/gen/a.c",
            "build",
        ] {
            let e = check_not_reserved("k", bad).unwrap_err();
            assert_eq!(e.code, "templates: manifest", "{bad}");
            assert!(e.detail.contains(bad), "{}", e.detail);
        }
        for ok in ["robco.toml", "builds/x", "src/build/x", "robco.lock.txt"] {
            assert!(check_not_reserved("k", ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn template_ids_follow_their_rule() {
        for ok in ["holotape-c", "a", "a1-"] {
            assert!(is_template_id(ok), "{ok}");
        }
        for bad in ["", "1a", "-a", "Holotape", "a_b", &"a".repeat(33)] {
            assert!(!is_template_id(bad), "{bad}");
        }
    }
}
