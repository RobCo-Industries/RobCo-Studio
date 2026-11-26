// SPDX-License-Identifier: GPL-3.0-or-later

//! The one check of a `templates` package, by "The templates package",
//! "Layout of templates", "templates.json" and "Tokens" of RobCo-Core
//! `docs/spec/SDK_PACKAGE.md`.
//!
//! The installer calls [`validate`] with the entries of the archive before
//! it records an install as finished. `robco new` calls it with the entries
//! of the cached folder before it uses a template. One function, so a
//! package the installer keeps is a package `robco new` accepts.
//!
//! What needs the bytes of a template file is left to the caller that
//! reads them: the tokens of a file with `substitute` set to `true`, and
//! the hash of each file against `sdk.json`. Everything else is here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde_json::{Map, Value};

use crate::codes::ErrorCode;
use crate::install::{
    MAX_MANIFEST, MAX_TEMPLATE_BYTES, MAX_TEMPLATE_FILE, MAX_TEMPLATE_FILES, MAX_TEMPLATES_LIST,
    TEMPLATES_LICENCES, TEMPLATES_LICENCES_DIR, TEMPLATES_LIST, TEMPLATES_ROOT_FILES,
};
use crate::manifest;
use crate::resolve::{self, TEMPLATES, sdk_json::SdkJson};

/// The `templates.json` `format` this tool reads.
pub const FORMAT: u64 = 1;

/// The name of the package manifest at the package root.
const SDK_JSON: &str = "sdk.json";

/// The paths of a project folder that only the tool writes, by "A project
/// folder" of `docs/spec/STUDIO_PROJECT.md`: a template that lists one, or
/// a path under one, is refused. Compared in lower case, so a case-blind
/// file system cannot map another spelling onto them.
pub const RESERVED: &[&str] = &["robco.lock", "build"];

/// The file every template holds at its root: it makes the folder a
/// project.
pub const PROJECT_MANIFEST: &str = "robco.toml";

/// The kind of one entry of a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// Anything else: a link, a device, a pipe. Always refused.
    Other,
}

/// One entry of a package, from an archive or from a folder of the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The `/` path from the package root, with no trailing `/`.
    pub path: String,
    /// What the entry is.
    pub kind: Kind,
    /// Its size in bytes. Read only for a file.
    pub size: u64,
}

impl Entry {
    /// A file of `size` bytes.
    pub fn file(path: impl Into<String>, size: u64) -> Self {
        Self {
            path: path.into(),
            kind: Kind::File,
            size,
        }
    }

    /// A folder.
    pub fn folder(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind: Kind::Folder,
            size: 0,
        }
    }
}

/// The bounds a `templates` package keeps. The spec states none of them
/// yet: they are this tool's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes of `sdk.json`.
    pub manifest: u64,
    /// The most bytes of `templates.json`.
    pub list: u64,
    /// The most bytes of one template file.
    pub file: u64,
    /// The most files of one template.
    pub files: u64,
    /// The most bytes of one template, all its files together.
    pub bytes: u64,
}

impl Limits {
    /// The limits of the installer and of `robco new`.
    pub const DEFAULT: Self = Self {
        manifest: MAX_MANIFEST,
        list: MAX_TEMPLATES_LIST,
        file: MAX_TEMPLATE_FILE,
        files: MAX_TEMPLATE_FILES,
        bytes: MAX_TEMPLATE_BYTES,
    };
}

/// A template of a valid package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// The id, also its folder at the package root.
    pub id: String,
    /// The short name for lists.
    pub title: String,
    /// One sentence.
    pub description: String,
    /// Its files, sorted by path in byte order.
    pub files: Vec<TemplateFile>,
}

/// A file of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateFile {
    /// The `/` path inside the template folder and the new project.
    pub path: String,
    /// Whether the tokens of the file are replaced.
    pub substitute: bool,
    /// The size of the file in the package, in bytes.
    pub size: u64,
}

/// Why a package is not a valid `templates` package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// `templates: format`, `templates: manifest` or `package: manifest`.
    pub code: &'static str,
    /// Text for a person. It names the path or the field.
    pub detail: String,
}

impl Error {
    fn manifest(detail: impl Into<String>) -> Self {
        Self {
            code: "templates: manifest",
            detail: detail.into(),
        }
    }

    fn list(detail: impl Into<String>) -> Self {
        Self::manifest(format!("{TEMPLATES_LIST}: {}", detail.into()))
    }

    fn format(detail: impl Into<String>) -> Self {
        Self {
            code: "templates: format",
            detail: detail.into(),
        }
    }

    fn package(detail: impl Into<String>) -> Self {
        Self {
            code: "package: manifest",
            detail: detail.into(),
        }
    }
}

impl ErrorCode for Error {
    fn error_code(&self) -> &'static str {
        self.code
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for Error {}

/// The repository that builds the `templates` package.
pub const BUILT_FROM_REPO: &str = "RobCo-Reel";

/// The licence of every file of a `templates` package.
pub const LICENCE: &str = "MIT";

/// Holds `sdk.json` to the property table of "The templates package".
///
/// `sdk_json::parse` already holds `version` to `X.Y.Z`, refuses `abi`,
/// `tools` and `firmware`, requires the three formats, and refuses a format
/// it does not know. This checks the rest: `requires` is empty,
/// `built_from` is one `RobCo-Reel` entry, `formats` holds the three
/// formats and no other, and every file has an MIT licence and no other.
fn check_sdk(sdk: &SdkJson) -> Result<(), Error> {
    if let Some(r) = sdk.requires.first() {
        return Err(Error::package(format!(
            "{SDK_JSON}: requires names {}, and {TEMPLATES} requires nothing",
            r.name
        )));
    }
    match sdk.built_from.as_slice() {
        [b] if b.repo == BUILT_FROM_REPO => {}
        _ => {
            return Err(Error::package(format!(
                "{SDK_JSON}: built_from must be one {BUILT_FROM_REPO} entry and its commit"
            )));
        }
    }
    let want = resolve::required_formats(TEMPLATES);
    if let Some(f) = sdk.formats.keys().find(|f| !want.contains(&f.as_str())) {
        return Err(Error::package(format!(
            "{SDK_JSON}: formats.{f} is present, and {TEMPLATES} carries only {}",
            want.join(", ")
        )));
    }
    if let Some(l) = sdk.licences.iter().find(|l| l.spdx != LICENCE) {
        return Err(Error::package(format!(
            "{SDK_JSON}: licences holds {}, and every file of {TEMPLATES} is {LICENCE}",
            l.spdx
        )));
    }
    let covered = |path: &str| {
        sdk.licences
            .iter()
            .flat_map(|l| &l.paths)
            .any(|p| covers(p, path))
    };
    if let Some(f) = sdk.files.iter().find(|f| !covered(&f.path)) {
        return Err(Error::package(format!(
            "{SDK_JSON}: {}: no licences entry covers it",
            f.path
        )));
    }
    Ok(())
}

/// Whether the licence prefix `prefix` covers `path`. A prefix is a whole
/// path or whole folders: `""` covers every path, `a` and `a/` cover
/// `a/b`, and `a` does not cover `ab`.
fn covers(prefix: &str, path: &str) -> bool {
    if prefix.is_empty() || prefix == path {
        return true;
    }
    if prefix.ends_with('/') {
        return path.starts_with(prefix);
    }
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Checks a whole `templates` package and returns its templates.
///
/// `sdk` is its parsed `sdk.json`, `list` the bytes of `templates.json` or
/// `None` when the package holds none, and `entries` every file and folder
/// of the package, `sdk.json` included, however the caller found them. An
/// archive may leave its folders out; a folder of the cache lists them.
/// The record a finished install adds is no entry of the package.
///
/// It checks, in this order: `sdk.json` names the package, keeps the
/// property table of the spec, and keeps its limit; `templates.json` is there, keeps its limit, and has the form,
/// format, fields, sort orders, ids, paths, reserved names and
/// `robco.toml` of the spec; no entry is a link or other kind; no two
/// entries differ only in case; the root holds the layout and nothing else;
/// each listed template file is there, and a template folder holds only
/// its listed files and their folders; every file but `sdk.json` is in
/// `sdk.json` `files`, and every path there is a file of the package; the
/// size and count limits.
///
/// A token in a path is refused here. A token in the bytes of a file is
/// not: that needs the bytes, which the caller reads when it fills the
/// file.
pub fn validate(
    sdk: &SdkJson,
    list: Option<&[u8]>,
    entries: &[Entry],
    limits: &Limits,
) -> Result<Vec<Template>, Error> {
    if sdk.name != TEMPLATES {
        return Err(Error::package(format!(
            "{SDK_JSON} names the package {}, not {TEMPLATES}",
            sdk.name
        )));
    }
    check_sdk(sdk)?;
    let find = |p: &str| entries.iter().find(|e| e.path == p);
    match find(SDK_JSON) {
        Some(e) if e.kind == Kind::File && e.size > limits.manifest => {
            return Err(Error::package(format!(
                "{SDK_JSON}: {} bytes, over the limit of {} bytes",
                e.size, limits.manifest
            )));
        }
        Some(e) if e.kind == Kind::File => {}
        _ => {
            return Err(Error::package(format!(
                "{SDK_JSON} is not a file of the package"
            )));
        }
    }
    let (
        Some(bytes),
        Some(Entry {
            kind: Kind::File,
            size,
            ..
        }),
    ) = (list, find(TEMPLATES_LIST))
    else {
        return Err(Error::manifest(format!(
            "{TEMPLATES_LIST} is missing from the package"
        )));
    };
    if *size > limits.list || bytes.len() as u64 > limits.list {
        return Err(Error::list(format!(
            "over the limit of {} bytes",
            limits.list
        )));
    }
    let parsed = parse(bytes)?;
    let ids: Vec<&str> = parsed.iter().map(|t| t.id.as_str()).collect();

    if let Some(e) = entries.iter().find(|e| e.kind == Kind::Other) {
        return Err(Error::manifest(format!(
            "{}: a link or not a regular file",
            e.path
        )));
    }
    let clash = resolve::case_clash(
        entries
            .iter()
            .map(|e| (e.path.as_str(), e.kind == Kind::File)),
    );
    if let Some((a, b)) = clash {
        return Err(Error::manifest(format!(
            "{b}: {a} differs from a part of it only in case"
        )));
    }

    // Every file of every template, by its full path, and every folder
    // above one: what a template folder may hold.
    let mut listed: BTreeSet<String> = BTreeSet::new();
    let mut folders: BTreeSet<String> = BTreeSet::new();
    for t in &parsed {
        folders.insert(t.id.clone());
        for f in &t.files {
            let full = format!("{}/{}", t.id, f.path);
            let mut at = 0;
            while let Some(p) = full[at..].find('/') {
                folders.insert(full[..at + p].to_owned());
                at += p + 1;
            }
            listed.insert(full);
        }
    }

    // The layout of the root, then what each template folder holds.
    let is_file = |e: &Entry| e.kind == Kind::File;
    for e in entries {
        let folder = e.kind == Kind::Folder;
        if !layout_allows(&e.path, folder, &ids) {
            return Err(Error::manifest(format!(
                "{}: the layout of a templates package does not hold it",
                e.path
            )));
        }
        let under_id = e
            .path
            .split_once('/')
            .is_some_and(|(first, _)| ids.contains(&first));
        if !under_id {
            continue;
        }
        if folder && !folders.contains(&e.path) {
            return Err(Error::manifest(format!(
                "{}/: the package holds this folder, and files lists nothing in it",
                e.path
            )));
        }
        if is_file(e) && !listed.contains(&e.path) {
            return Err(Error::manifest(format!(
                "{}: the package holds it, and files does not list it",
                e.path
            )));
        }
    }
    for l in TEMPLATES_LICENCES {
        let p = format!("{TEMPLATES_LICENCES_DIR}/{l}");
        if !find(&p).is_some_and(is_file) {
            return Err(Error::manifest(format!(
                "{p}: a templates package must hold it"
            )));
        }
    }
    let present: BTreeMap<&str, u64> = entries
        .iter()
        .filter(|e| is_file(e))
        .map(|e| (e.path.as_str(), e.size))
        .collect();
    for t in &parsed {
        let prefix = format!("{}/", t.id);
        let held = entries
            .iter()
            .any(|e| e.path == t.id || e.path.starts_with(&prefix));
        if !held {
            return Err(Error::manifest(format!(
                "{}/: listed in {TEMPLATES_LIST}, and the package does not hold it",
                t.id
            )));
        }
        for f in &t.files {
            if !present.contains_key(format!("{prefix}{}", f.path).as_str()) {
                return Err(Error::manifest(format!(
                    "{prefix}{}: listed, and the package does not hold it",
                    f.path
                )));
            }
        }
    }

    // "Everything else: listed in sdk.json files, and nothing unlisted."
    let in_sdk: BTreeSet<&str> = sdk.files.iter().map(|f| f.path.as_str()).collect();
    if let Some(p) = present
        .keys()
        .find(|p| **p != SDK_JSON && !in_sdk.contains(*p))
    {
        return Err(Error::package(format!(
            "{p}: not listed in {SDK_JSON} files"
        )));
    }
    if let Some(p) = in_sdk.iter().find(|p| !present.contains_key(*p)) {
        return Err(Error::package(format!(
            "{p}: listed in {SDK_JSON} files, and the package does not hold it"
        )));
    }

    // The limits, from the sizes of the entries.
    let mut out = Vec::with_capacity(parsed.len());
    for t in parsed {
        if t.files.len() as u64 > limits.files {
            return Err(Error::manifest(format!(
                "{}/: more than {} files",
                t.id, limits.files
            )));
        }
        let mut total: u64 = 0;
        let mut files = Vec::with_capacity(t.files.len());
        for f in t.files {
            let full = format!("{}/{}", t.id, f.path);
            let size = present.get(full.as_str()).copied().unwrap_or(0);
            if size > limits.file {
                return Err(Error::manifest(format!(
                    "{full}: {size} bytes, over the limit of {} bytes for a template file",
                    limits.file
                )));
            }
            total = total.saturating_add(size);
            if total > limits.bytes {
                return Err(Error::manifest(format!(
                    "{}/: more than {} bytes in all",
                    t.id, limits.bytes
                )));
            }
            files.push(TemplateFile {
                path: f.path,
                substitute: f.substitute,
                size,
            });
        }
        out.push(Template {
            id: t.id,
            title: t.title,
            description: t.description,
            files,
        });
    }
    Ok(out)
}

/// "Layout of templates": whether a `templates` package may hold `path`, a
/// `/` path from its root, a folder when `folder` is true. The root holds
/// `sdk.json`, `templates.json`, `LICENSES/MIT.txt` and one folder per id
/// in `ids`, and nothing else. What lies under a template folder is
/// checked against its `files`.
fn layout_allows(path: &str, folder: bool, ids: &[&str]) -> bool {
    let (first, rest) = match path.split_once('/') {
        Some((f, r)) => (f, Some(r)),
        None => (path, None),
    };
    if ids.contains(&first) {
        return rest.is_some() || folder;
    }
    match rest {
        None if folder => first == TEMPLATES_LICENCES_DIR,
        None => TEMPLATES_ROOT_FILES.contains(&first),
        Some(r) => first == TEMPLATES_LICENCES_DIR && !folder && TEMPLATES_LICENCES.contains(&r),
    }
}

/// A template as `templates.json` gives it.
struct RawTemplate {
    id: String,
    title: String,
    description: String,
    files: Vec<RawFile>,
}

/// A file as `templates.json` gives it.
struct RawFile {
    path: String,
    substitute: bool,
}

/// The keys of each object of `templates.json`.
const TOP_KEYS: &[&str] = &["format", "templates"];
const TEMPLATE_KEYS: &[&str] = &["id", "title", "description", "kind", "files"];
const FILE_KEYS: &[&str] = &["path", "substitute"];

/// The text of `templates.json`, in the order of its table: it parses,
/// `format`, unknown fields, then each field.
fn parse(bytes: &[u8]) -> Result<Vec<RawTemplate>, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::list("the file is not UTF-8"))?;
    let root: Value = serde_json::from_str(text)
        .map_err(|e| Error::list(format!("the file is not JSON: {e}")))?;
    let o = root
        .as_object()
        .ok_or_else(|| Error::list("the top value is not an object"))?;

    match o.get("format").map(Value::as_u64) {
        None => return Err(Error::format("templates.json: format is missing")),
        Some(Some(n)) if n > FORMAT => {
            return Err(Error::format(format!(
                "templates.json has format {n} and this tool reads {FORMAT}: install a newer tool"
            )));
        }
        Some(Some(n)) if n >= 1 => {}
        Some(_) => {
            return Err(Error::format(
                "templates.json: format is not a positive integer",
            ));
        }
    }

    unknown_fields(o)?;

    let list = match o.get("templates") {
        None => return Err(Error::list("templates is missing")),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(Error::list("templates is not an array")),
    };
    if list.is_empty() {
        return Err(Error::list("templates is empty"));
    }
    let mut out: Vec<RawTemplate> = Vec::with_capacity(list.len());
    for (i, v) in list.iter().enumerate() {
        let at = format!("templates[{i}]");
        let t = template(&at, v)?;
        if let Some(prev) = out.last()
            && prev.id.as_bytes() >= t.id.as_bytes()
        {
            return Err(Error::list(format!(
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

/// A field this spec does not define, at any level, before any value is
/// checked. A value of the wrong type is left to the field's own check.
fn unknown_fields(o: &Map<String, Value>) -> Result<(), Error> {
    let check = |at: &str, o: &Map<String, Value>, known: &[&str]| match o
        .keys()
        .find(|k| !known.contains(&k.as_str()))
    {
        Some(k) => Err(Error::list(format!(
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

fn string<'a>(o: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a str, Error> {
    match o.get(key) {
        None => Err(Error::list(format!("{at}.{key} is missing"))),
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(Error::list(format!("{at}.{key} is not a string"))),
    }
}

/// An ASCII text of 1 to `max` bytes, with no control character.
fn ascii_text<'a>(
    o: &'a Map<String, Value>,
    at: &str,
    key: &str,
    max: usize,
) -> Result<&'a str, Error> {
    let s = string(o, at, key)?;
    if s.is_empty() || s.len() > max || !s.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return Err(Error::list(format!(
            "{at}.{key} is not ASCII text of 1 to {max} bytes"
        )));
    }
    Ok(s)
}

/// Whether `id` is a template id: 1 to 32 of `a` to `z`, `0` to `9` and
/// `-`, starting with a letter.
pub fn is_template_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// One template, its fields in the order of the table.
fn template(at: &str, v: &Value) -> Result<RawTemplate, Error> {
    let o = v
        .as_object()
        .ok_or_else(|| Error::list(format!("{at} is not an object")))?;
    let id = string(o, at, "id")?;
    if !is_template_id(id) {
        return Err(Error::list(format!(
            "{at}.id {id:?}: 1 to 32 of a to z, 0 to 9 and -, starting with a letter"
        )));
    }
    let title = ascii_text(o, at, "title", 64)?;
    let description = ascii_text(o, at, "description", 200)?;
    let kind = string(o, at, "kind")?;
    if kind != "holotape" {
        return Err(Error::list(format!(
            "{at}.kind {kind:?}: this tool writes holotape only"
        )));
    }
    let list = match o.get("files") {
        None => return Err(Error::list(format!("{at}.files is missing"))),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(Error::list(format!("{at}.files is not an array"))),
    };
    let mut files: Vec<RawFile> = Vec::with_capacity(list.len());
    for (j, f) in list.iter().enumerate() {
        let fat = format!("{at}.files[{j}]");
        let fo = f
            .as_object()
            .ok_or_else(|| Error::list(format!("{fat} is not an object")))?;
        let path = string(fo, &fat, "path")?;
        let substitute = match fo.get("substitute") {
            None => return Err(Error::list(format!("{fat}.substitute is missing"))),
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(Error::list(format!("{fat}.substitute is not a boolean"))),
        };
        check_path(&format!("{fat}.path"), path)?;
        check_not_reserved(&format!("{fat}.path"), path)?;
        if let Some(prev) = files.last()
            && prev.path.as_bytes() >= path.as_bytes()
        {
            return Err(Error::list(format!(
                "{fat}.path {path}: the files are sorted by path with no path twice"
            )));
        }
        files.push(RawFile {
            path: path.to_owned(),
            substitute,
        });
    }
    // No two paths may differ only in case in any part, the folders above
    // them included.
    if let Some((a, b)) = resolve::case_clash(files.iter().map(|f| (f.path.as_str(), true))) {
        return Err(Error::list(format!(
            "{at}.files: {b} and {a} of {id} differ only in case"
        )));
    }
    if !files.iter().any(|f| f.path == PROJECT_MANIFEST) {
        return Err(Error::list(format!(
            "{at}.files: {id} lists no {PROJECT_MANIFEST} at its root"
        )));
    }
    Ok(RawTemplate {
        id: id.to_owned(),
        title: title.to_owned(),
        description: description.to_owned(),
        files,
    })
}

/// A path whose first component, in lower case, is a path of [`RESERVED`]:
/// the file itself or anything under it.
fn check_not_reserved(key: &str, path: &str) -> Result<(), Error> {
    let first = path.split('/').next().unwrap_or(path).to_lowercase();
    if RESERVED.contains(&first.as_str()) {
        return Err(Error::list(format!(
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
/// component one plain file name on every system: no `:`, no back slash,
/// no component that starts with `-`, no character Windows refuses, no
/// device name, no component that ends with a dot or a space. Then no
/// token.
fn check_path(key: &str, path: &str) -> Result<(), Error> {
    manifest::check_relative_path(key, path).map_err(|e| Error::list(e.detail()))?;
    let bad = |why: &str| Err(Error::list(format!("{key}: {path:?}: {why}")));
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

/// The tokens of `b`, as ranges: `@`, one or more of `A` to `Z` and `_`,
/// then `@`, read from the left.
pub fn tokens(b: &[u8]) -> Vec<std::ops::Range<usize>> {
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

#[cfg(test)]
mod tests;
