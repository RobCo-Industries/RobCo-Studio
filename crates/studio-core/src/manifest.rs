// SPDX-License-Identifier: GPL-3.0-or-later

//! The project manifest, `robco.toml`, format 1.
//!
//! The rules are the "robco.toml", "Kinds" and "Errors" sections of
//! `docs/spec/STUDIO_PROJECT.md` in RobCo-Core, with "Source order", and
//! "Order of checks" of `docs/spec/STUDIO_CLI.md`, read at commit
//! `e004c73`. The manifest is TOML 1.1, as the `toml` crate parses it.
//!
//! This module is pure: text in, a checked [`Manifest`] or a
//! [`ManifestError`] out. It reads no file. Three checks need the project
//! folder, so the build runs them with the helpers here:
//!
//! - [`expand_sources`] expands the source patterns against a list of the
//!   project's regular files, which the caller gathers.
//! - [`read_icon`] checks the icon file's bytes, which the caller reads.
//! - That a path names a file inside the root, and that no symbolic link
//!   leads outside it, is left to the build. This module checks only the
//!   spelling of each path.
//!
//! The parser is strict. An unknown key fails, and so does a value it
//! cannot read. Each error of the spec has its own variant, and
//! [`ManifestError::code`] gives the code as the spec spells it.

use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

/// The highest manifest `format` this module reads.
pub const FORMAT: i64 = 1;

/// The most source patterns `build.sources` may hold.
pub const MAX_SOURCE_PATTERNS: usize = 256;

/// The longest `project.title`, in bytes.
pub const MAX_TITLE: usize = 64;

/// The size of the META icon, in bytes.
pub const ICON_SIZE: usize = 128;

/// The sources used when `build.sources` is absent.
pub const DEFAULT_SOURCES: &str = "src/*.c";

/// A checked `robco.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The `format` of the file.
    pub format: i64,
    /// The `[project]` table.
    pub project: Project,
    /// The `[target]` table.
    pub target: Target,
    /// The `[build]` table, with its defaults filled in.
    pub build: Build,
}

/// The `[project]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The output stem and the header's `name`: 1 to 8 of `A-Z0-9_-`.
    pub name: String,
    /// The META title, empty when absent.
    pub title: String,
    /// The image version.
    pub version: Version,
    /// What the project builds.
    pub kind: Kind,
    /// The `ROBCO78` subtype.
    pub subtype: Subtype,
    /// The icon path, relative to the project root, when given.
    pub icon: Option<String>,
}

/// A `project.version`, `X.Y.Z`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    /// X, 0 to 65535.
    pub major: u16,
    /// Y, 0 to 255.
    pub minor: u8,
    /// Z, 0 to 255.
    pub patch: u8,
}

impl Version {
    /// The header's `image_version`: `(X << 16) | (Y << 8) | Z`.
    pub fn image_version(self) -> u32 {
        (u32::from(self.major) << 16) | (u32::from(self.minor) << 8) | u32::from(self.patch)
    }
}

/// A `target.abi`, `MAJOR.MINOR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abi {
    /// MAJOR. Always 1 for `pipos`.
    pub major: u16,
    /// MINOR, 0 to 65535.
    pub minor: u16,
}

impl Abi {
    /// The header's `abi_version`: `(MAJOR << 16) | MINOR`.
    pub fn abi_version(self) -> u32 {
        (u32::from(self.major) << 16) | u32::from(self.minor)
    }
}

/// A `project.kind` that format 1 builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A sealed `ROBCO78` image, `build/NAME.HTP`.
    Holotape,
}

/// A holotape's `project.subtype`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subtype {
    /// `game`, subtype 0.
    Game,
    /// `utility`, subtype 1.
    Utility,
}

impl Subtype {
    /// The header's `subtype` byte.
    pub fn value(self) -> u8 {
        match self {
            Subtype::Game => 0,
            Subtype::Utility => 1,
        }
    }
}

/// The `[target]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The operating system the output runs on.
    pub platform: Platform,
    /// The lowest ABI the project needs.
    pub abi: Abi,
}

/// A `target.platform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Pip-OS.
    Pipos,
}

/// The `[build]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// Who runs the build.
    pub backend: Backend,
    /// The source patterns, in the order the file gives them.
    pub sources: Vec<String>,
}

/// A `build.backend`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The tool compiles, links and seals itself.
    Robco,
}

/// A manifest error. Each variant is one code of the spec's "Errors" table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// Not TOML or not UTF-8. The detail holds the 1-based line.
    Syntax {
        /// The line of the error, 1-based.
        line: usize,
        /// The parser's message.
        message: String,
    },
    /// `format` is missing, not a positive integer, or too large.
    Format(String),
    /// A key or table the spec does not define, as a dotted name.
    UnknownKey(String),
    /// A key has the wrong TOML type. The dotted key and the type wanted.
    Type {
        /// The dotted key.
        key: String,
        /// The type the spec wants.
        expected: &'static str,
    },
    /// A required key is absent. The dotted key.
    Missing(String),
    /// `project.name` breaks a name rule. The reason.
    Name(String),
    /// `project.version` is not `X.Y.Z` or out of range.
    Version(String),
    /// `project.title` breaks a title rule. The reason.
    Title(String),
    /// `project.kind` is unknown or not built by this tool. The kind.
    Kind(String),
    /// `project.subtype` is missing for a holotape or not known.
    Subtype(String),
    /// `target.platform` is not `pipos`. The platform.
    Platform(String),
    /// `target.abi` is not `MAJOR.MINOR`, out of range or the wrong major.
    Abi(String),
    /// A path breaks a path rule. The dotted key and the reason.
    Path {
        /// The dotted key, with an index for an array item.
        key: String,
        /// The rule broken.
        reason: String,
    },
    /// The icon file is missing or breaks the icon rules.
    Icon(String),
    /// A pattern matches nothing, or matches a file that is not `.c`.
    Sources(String),
}

impl ManifestError {
    /// The code, as the spec spells it, such as `unknown key`.
    pub fn code(&self) -> &'static str {
        match self {
            ManifestError::Syntax { .. } => "syntax",
            ManifestError::Format(_) => "format",
            ManifestError::UnknownKey(_) => "unknown key",
            ManifestError::Type { .. } => "type",
            ManifestError::Missing(_) => "missing",
            ManifestError::Name(_) => "name",
            ManifestError::Version(_) => "version",
            ManifestError::Title(_) => "title",
            ManifestError::Kind(_) => "kind",
            ManifestError::Subtype(_) => "subtype",
            ManifestError::Platform(_) => "platform",
            ManifestError::Abi(_) => "abi",
            ManifestError::Path { .. } => "path",
            ManifestError::Icon(_) => "icon",
            ManifestError::Sources(_) => "sources",
        }
    }

    /// The detail that follows the code.
    pub fn detail(&self) -> String {
        match self {
            ManifestError::Syntax { line, message } => format!("line {line}: {message}"),
            ManifestError::Type { key, expected } => format!("{key}: expected {expected}"),
            ManifestError::Path { key, reason } => format!("{key}: {reason}"),
            ManifestError::Format(d)
            | ManifestError::UnknownKey(d)
            | ManifestError::Missing(d)
            | ManifestError::Name(d)
            | ManifestError::Version(d)
            | ManifestError::Title(d)
            | ManifestError::Kind(d)
            | ManifestError::Subtype(d)
            | ManifestError::Platform(d)
            | ManifestError::Abi(d)
            | ManifestError::Icon(d)
            | ManifestError::Sources(d) => d.clone(),
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "manifest: {}: {}", self.code(), self.detail())
    }
}

impl std::error::Error for ManifestError {}

impl crate::codes::ErrorCode for ManifestError {
    fn error_code(&self) -> &'static str {
        match self {
            ManifestError::Syntax { .. } => "manifest: syntax",
            ManifestError::Format(_) => "manifest: format",
            ManifestError::UnknownKey(_) => "manifest: unknown key",
            ManifestError::Type { .. } => "manifest: type",
            ManifestError::Missing(_) => "manifest: missing",
            ManifestError::Name(_) => "manifest: name",
            ManifestError::Version(_) => "manifest: version",
            ManifestError::Title(_) => "manifest: title",
            ManifestError::Kind(_) => "manifest: kind",
            ManifestError::Subtype(_) => "manifest: subtype",
            ManifestError::Platform(_) => "manifest: platform",
            ManifestError::Abi(_) => "manifest: abi",
            ManifestError::Path { .. } => "manifest: path",
            ManifestError::Icon(_) => "manifest: icon",
            ManifestError::Sources(_) => "manifest: sources",
        }
    }
}

/// The keys format 1 defines, per table, besides `format`, `project`,
/// `target` and `build` at the top. Anything else is unknown, the reserved
/// keys `build.runtime` and `[[asset]]` included.
const PROJECT_KEYS: &[&str] = &["name", "title", "version", "kind", "subtype", "icon"];
const TARGET_KEYS: &[&str] = &["platform", "abi"];
const BUILD_KEYS: &[&str] = &["backend", "sources"];

/// Parses and checks the bytes of a `robco.toml`.
///
/// Returns the first error found. The checks run in this order: syntax,
/// `format`, unknown keys, then each key in the order of the spec's tables.
pub fn parse(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    let text = std::str::from_utf8(bytes).map_err(|e| ManifestError::Syntax {
        line: line_of(bytes, e.valid_up_to()),
        message: "not UTF-8".into(),
    })?;
    let root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| ManifestError::Syntax {
            line: e.span().map_or(1, |s| line_of(bytes, s.start)),
            message: e.message().trim().to_string(),
        })?;

    let format = check_format(&root)?;
    check_unknown(text)?;

    let project = table(&root, "project")?;
    let target = table(&root, "target")?;
    let build = optional_table(&root, "build")?;

    let name = required_str(project, "project", "name")?;
    check_name(name)?;
    let title = optional_str(project, "project", "title")?.unwrap_or("");
    check_title(title)?;
    let version = parse_version(required_str(project, "project", "version")?)?;
    let kind = required_str(project, "project", "kind")?;
    let kind = match kind {
        "holotape" => Kind::Holotape,
        "rom" | "journal" | "terminal-content" | "site" => {
            return Err(ManifestError::Kind(format!(
                "{kind}: reserved, not built by format 1"
            )));
        }
        other => return Err(ManifestError::Kind(format!("{other}: unknown kind"))),
    };
    let subtype = match optional_str(project, "project", "subtype")? {
        None => return Err(ManifestError::Subtype("missing for a holotape".into())),
        Some("game") => Subtype::Game,
        Some("utility") => Subtype::Utility,
        Some(other) => {
            return Err(ManifestError::Subtype(format!(
                "{other}: not game or utility"
            )));
        }
    };
    let icon = optional_str(project, "project", "icon")?;
    if let Some(icon) = icon {
        check_path("project.icon", icon, false)?;
    }

    let platform = match required_str(target, "target", "platform")? {
        "pipos" => Platform::Pipos,
        other => return Err(ManifestError::Platform(other.to_string())),
    };
    let abi = parse_abi(required_str(target, "target", "abi")?)?;

    let (backend, sources) = match build {
        None => (Backend::Robco, vec![DEFAULT_SOURCES.to_string()]),
        Some(build) => {
            // A string other than `robco` failed at step 4, with the
            // unknown keys.
            optional_str(build, "build", "backend")?;
            let backend = Backend::Robco;
            (backend, check_sources(build)?)
        }
    };

    Ok(Manifest {
        format,
        project: Project {
            name: name.to_string(),
            title: title.to_string(),
            version,
            kind,
            subtype,
            icon: icon.map(str::to_string),
        },
        target: Target { platform, abi },
        build: Build { backend, sources },
    })
}

/// The 1-based line of a byte offset.
fn line_of(bytes: &[u8], offset: usize) -> usize {
    let end = offset.min(bytes.len());
    bytes[..end].iter().filter(|&&b| b == b'\n').count() + 1
}

fn check_format(root: &Table) -> Result<i64, ManifestError> {
    match root.get("format") {
        None => Err(ManifestError::Format("missing".into())),
        Some(Value::Integer(n)) if *n < 1 => Err(ManifestError::Format(format!(
            "{n} is not a positive integer"
        ))),
        Some(Value::Integer(n)) if *n > FORMAT => Err(ManifestError::Format(format!(
            "{n} is newer than this tool reads, {FORMAT}: install a newer tool"
        ))),
        Some(Value::Integer(n)) => Ok(*n),
        Some(_) => Err(ManifestError::Format("not a positive integer".into())),
    }
}

/// Step 4 of "Errors": the first unknown key or table in the order of the
/// file, where a `build.backend` other than `robco` counts as one at the
/// place of its key. The detail of that backend is `build.backend = "<value>"`,
/// the value written as a TOML basic string.
fn check_unknown(text: &str) -> Result<(), ManifestError> {
    use toml::de::{DeTable, DeValue};
    // The text parsed once already, so this parse cannot fail.
    let Ok(root) = DeTable::parse(text) else {
        return Ok(());
    };
    let mut found: Vec<(usize, String)> = Vec::new();
    for (key, value) in root.get_ref() {
        let name: &str = key.get_ref();
        let keys = match name {
            "format" => continue,
            "project" => PROJECT_KEYS,
            "target" => TARGET_KEYS,
            "build" => BUILD_KEYS,
            _ => {
                found.push((key.span().start, name.to_string()));
                continue;
            }
        };
        let DeValue::Table(table) = value.get_ref() else {
            continue;
        };
        for (k, v) in table {
            let inner: &str = k.get_ref();
            if !keys.contains(&inner) {
                found.push((k.span().start, format!("{name}.{inner}")));
            } else if (name, inner) == ("build", "backend")
                && let DeValue::String(backend) = v.get_ref()
                && backend != "robco"
            {
                found.push((
                    k.span().start,
                    format!("build.backend = {}", crate::resolve::lock::quote(backend)),
                ));
            }
        }
    }
    match found.into_iter().min_by_key(|(at, _)| *at) {
        Some((_, key)) => Err(ManifestError::UnknownKey(key)),
        None => Ok(()),
    }
}

fn optional_table<'a>(root: &'a Table, name: &str) -> Result<Option<&'a Table>, ManifestError> {
    match root.get(name) {
        None => Ok(None),
        Some(Value::Table(t)) => Ok(Some(t)),
        Some(_) => Err(ManifestError::Type {
            key: name.to_string(),
            expected: "table",
        }),
    }
}

fn table<'a>(root: &'a Table, name: &str) -> Result<&'a Table, ManifestError> {
    optional_table(root, name)?.ok_or_else(|| ManifestError::Missing(name.to_string()))
}

fn optional_str<'a>(
    t: &'a Table,
    table: &str,
    key: &str,
) -> Result<Option<&'a str>, ManifestError> {
    match t.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ManifestError::Type {
            key: format!("{table}.{key}"),
            expected: "string",
        }),
    }
}

fn required_str<'a>(t: &'a Table, table: &str, key: &str) -> Result<&'a str, ManifestError> {
    optional_str(t, table, key)?.ok_or_else(|| ManifestError::Missing(format!("{table}.{key}")))
}

fn check_name(name: &str) -> Result<(), ManifestError> {
    if name.is_empty() || name.len() > 8 {
        return Err(ManifestError::Name(format!(
            "{name:?}: must be 1 to 8 characters long"
        )));
    }
    if let Some(c) = name
        .chars()
        .find(|c| !(c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_' || *c == '-'))
    {
        return Err(ManifestError::Name(format!(
            "{name:?}: {c:?} is not one of A to Z, 0 to 9, _ and -"
        )));
    }
    Ok(())
}

fn check_title(title: &str) -> Result<(), ManifestError> {
    if !title.is_ascii() {
        return Err(ManifestError::Title("not ASCII".into()));
    }
    if title.contains('\0') {
        return Err(ManifestError::Title("holds a NUL".into()));
    }
    // STUDIO_PROJECT.md, table of TOML 1.1 forms: no key takes a control
    // character, so a title with `\e` fails `manifest: title`.
    if title.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(ManifestError::Title("holds a control character".into()));
    }
    if title.len() > MAX_TITLE {
        return Err(ManifestError::Title(format!(
            "{} bytes, longer than {MAX_TITLE}",
            title.len()
        )));
    }
    Ok(())
}

/// Reads one decimal part: digits only, leading zeros ignored, at most `max`.
fn decimal(part: &str, max: u32) -> Option<u32> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = part.trim_start_matches('0');
    let n = if digits.is_empty() {
        0
    } else {
        digits.parse::<u32>().ok()?
    };
    (n <= max).then_some(n)
}

/// Parses `X.Y.Z`: X from 0 to 65535, Y and Z from 0 to 255.
pub fn parse_version(text: &str) -> Result<Version, ManifestError> {
    let bad = || {
        ManifestError::Version(format!(
            "{text:?}: not X.Y.Z with X to 65535, Y and Z to 255"
        ))
    };
    let parts: Vec<&str> = text.split('.').collect();
    let [x, y, z] = parts.as_slice() else {
        return Err(bad());
    };
    let major = decimal(x, 65535)
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(bad)?;
    let minor = decimal(y, 255)
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(bad)?;
    let patch = decimal(z, 255)
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(bad)?;
    Ok(Version {
        major,
        minor,
        patch,
    })
}

/// Parses `MAJOR.MINOR` for `pipos`: MAJOR must be 1, MINOR to 65535.
pub fn parse_abi(text: &str) -> Result<Abi, ManifestError> {
    let bad = || ManifestError::Abi(format!("{text:?}: not MAJOR.MINOR, each 0 to 65535"));
    let parts: Vec<&str> = text.split('.').collect();
    let [major, minor] = parts.as_slice() else {
        return Err(bad());
    };
    let major = decimal(major, 65535)
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(bad)?;
    let minor = decimal(minor, 65535)
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(bad)?;
    if major != 1 {
        return Err(ManifestError::Abi(format!(
            "{text:?}: major {major}, pipos needs 1"
        )));
    }
    Ok(Abi { major, minor })
}

/// Checks the spelling of a manifest path. `pattern` allows `*` in the
/// last component. It does not look at the file system.
pub(crate) fn check_path(key: &str, path: &str, pattern: bool) -> Result<(), ManifestError> {
    let fail = |reason: &str| {
        Err(ManifestError::Path {
            key: key.to_string(),
            reason: format!("{path:?}: {reason}"),
        })
    };
    if path.starts_with('/') {
        return fail("starts with /");
    }
    let b = path.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return fail("starts with a drive letter");
    }
    if path.contains('\\') {
        return fail("uses \\, the separator is /");
    }
    if path.contains('\0') {
        return fail("holds a NUL");
    }
    let parts: Vec<&str> = path.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        match *part {
            "" => return fail("has an empty component"),
            "." => return fail("has a . component"),
            ".." => return fail("has a .. component"),
            _ => {}
        }
        if part.contains('*') && (!pattern || i + 1 != parts.len()) {
            return fail("* is allowed only in the last component of a source pattern");
        }
    }
    Ok(())
}

fn check_sources(build: &Table) -> Result<Vec<String>, ManifestError> {
    let array = match build.get("sources") {
        None => return Ok(vec![DEFAULT_SOURCES.to_string()]),
        Some(Value::Array(a)) => a,
        Some(_) => {
            return Err(ManifestError::Type {
                key: "build.sources".into(),
                expected: "array of strings",
            });
        }
    };
    if array.is_empty() || array.len() > MAX_SOURCE_PATTERNS {
        return Err(ManifestError::Sources(format!(
            "build.sources holds {} patterns, 1 to {MAX_SOURCE_PATTERNS} allowed",
            array.len()
        )));
    }
    let mut out = Vec::with_capacity(array.len());
    for (i, item) in array.iter().enumerate() {
        let key = format!("build.sources[{i}]");
        let Value::String(s) = item else {
            return Err(ManifestError::Type {
                key,
                expected: "string",
            });
        };
        check_path(&key, s, true)?;
        out.push(s.clone());
    }
    Ok(out)
}

/// Matches one path component against a component that may hold `*`.
fn component_matches(pattern: &str, name: &str) -> bool {
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or("");
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let pieces: Vec<&str> = pieces.collect();
    let Some((last, middle)) = pieces.split_last() else {
        return rest.is_empty();
    };
    for piece in middle {
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// True when a project path matches a source pattern.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    let (pdir, pname) = pattern.rsplit_once('/').unwrap_or(("", pattern));
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    pdir == dir && component_matches(pname, name)
}

/// The order of the source list, which is the link order.
///
/// `matches` holds, for each pattern in the order the manifest writes them,
/// the paths it expanded to. A pattern's own matches are sorted by byte
/// value, which leaves a pattern without `*` as its one path. The groups
/// are then appended in order, and a path already in the list keeps its
/// first place. This is the one place the rule lives.
pub fn order_sources(matches: Vec<Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for mut group in matches {
        group.sort_unstable();
        for path in group {
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// Expands the source patterns against the project's regular files.
///
/// `files` lists every regular file under the project root, as relative
/// paths with `/`. The caller gathers it; this function reads nothing.
/// The result is in the order of [`order_sources`]. A pattern that matches
/// nothing, or a match that does not end in `.c`, fails with `sources`.
/// Paths compare byte for byte, so a name that differs only in case
/// matches nothing.
pub fn expand_sources(patterns: &[String], files: &[&str]) -> Result<Vec<String>, ManifestError> {
    let mut groups = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        let mut group = Vec::new();
        for file in files.iter().filter(|f| pattern_matches(pattern, f)) {
            if !file.ends_with(".c") {
                return Err(ManifestError::Sources(format!(
                    "{pattern}: matches {file}, which is not a .c file"
                )));
            }
            group.push((*file).to_string());
        }
        if group.is_empty() {
            return Err(ManifestError::Sources(format!(
                "{pattern}: matches nothing"
            )));
        }
        groups.push(group);
    }
    Ok(order_sources(groups))
}

/// Applies the last path rule of the spec to one manifest path on disk:
/// "It names a file inside the project root. A symbolic link that leads
/// outside the root is refused."
///
/// `rel` is a manifest path, already checked by [`parse`], or a source
/// after [`expand_sources`], or a folder a source pattern names. The path
/// is resolved through every link, a link to a link included, and its
/// canonical form must lie under the canonical root, compared by
/// component. A root reached through a link is fine, since both sides are
/// canonical. A link whose target stays inside the root is allowed.
///
/// Returns the path under `root` to read, `None` when nothing is there,
/// a dangling link included, so the caller gives the error of a missing
/// file. A path that leads outside the root fails with `path` and `key`.
pub fn inside_root(root: &Path, key: &str, rel: &str) -> Result<Option<PathBuf>, ManifestError> {
    let outside = |reason: String| ManifestError::Path {
        key: key.to_owned(),
        reason,
    };
    let real_root = std::fs::canonicalize(root)
        .map_err(|e| outside(format!("{rel}: cannot resolve the project root: {e}")))?;
    let path = root.join(rel);
    match std::fs::canonicalize(&path) {
        Ok(real) if real.starts_with(&real_root) => Ok(Some(path)),
        Ok(_) => Err(outside(format!(
            "{rel}: a symbolic link leads outside the project root"
        ))),
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Ok(None),
        Err(e) => Err(outside(format!("{rel}: cannot resolve: {e}"))),
    }
}

/// Checks the icon file and returns its 128 META bytes.
///
/// `data` is the file's content, or `None` when the file is missing. The
/// rules are those of `rimg.py seal --icon-format auto`: exactly 128 bytes
/// is raw, any other length is a 32x32 PBM, `P1` or `P4`.
pub fn read_icon(path: &str, data: Option<&[u8]>) -> Result<[u8; ICON_SIZE], ManifestError> {
    let data = data.ok_or_else(|| ManifestError::Icon(format!("{path}: missing")))?;
    if let Ok(raw) = <[u8; ICON_SIZE]>::try_from(data) {
        return Ok(raw);
    }
    parse_pbm(data).map_err(|why| {
        ManifestError::Icon(format!(
            "{path}: {} bytes, so read as PBM: {why}",
            data.len()
        ))
    })
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

fn parse_pbm(data: &[u8]) -> Result<[u8; ICON_SIZE], String> {
    let magic = data.get(..2);
    if magic != Some(b"P1") && magic != Some(b"P4") {
        return Err("bad PBM header: expected P1 or P4".into());
    }
    let mut tokens: Vec<&[u8]> = Vec::new();
    let mut i = 0;
    while tokens.len() < 3 {
        while i < data.len() && (is_space(data[i]) || data[i] == b'#') {
            if data[i] == b'#' {
                while i < data.len() && data[i] != b'\r' && data[i] != b'\n' {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        let start = i;
        while i < data.len() && !is_space(data[i]) && data[i] != b'#' {
            i += 1;
        }
        if start == i {
            return Err("bad PBM header".into());
        }
        tokens.push(&data[start..i]);
    }
    let dim = |t: &[u8]| {
        std::str::from_utf8(t)
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| "bad PBM header".to_string())
    };
    let (w, h) = (dim(tokens[1])?, dim(tokens[2])?);
    if (w, h) != (32, 32) {
        return Err(format!("icon is {w}x{h}, expected 32x32"));
    }
    if tokens[0] == b"P4" {
        if i >= data.len() || !(is_space(data[i]) || data[i] == 0x0b || data[i] == 0x0c) {
            return Err("bad PBM header".into());
        }
        let raw = &data[i + 1..];
        return <[u8; ICON_SIZE]>::try_from(raw).map_err(|_| {
            format!(
                "PBM pixel data is {} bytes, expected {ICON_SIZE}",
                raw.len()
            )
        });
    }
    let mut out = [0u8; ICON_SIZE];
    let mut n = 0usize;
    let body = &data[i..];
    let mut j = 0;
    while j < body.len() {
        let c = body[j];
        if c == b'#' {
            while j < body.len() && body[j] != b'\r' && body[j] != b'\n' {
                j += 1;
            }
            continue;
        }
        match c {
            b'0' | b'1' => {
                if c == b'1' && n < 32 * 32 {
                    out[n / 8] |= 0x80 >> (n % 8);
                }
                n += 1;
            }
            b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c => {}
            other => {
                return Err(format!(
                    "bad character {:?} in PBM pixel data",
                    char::from(other)
                ));
            }
        }
        j += 1;
    }
    if n != 32 * 32 {
        return Err(format!("PBM has {n} pixels, expected 1024"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
