// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the SDK index.
//!
//! Implements RobCo-Core `docs/spec/SDK_PACKAGE.md`, "The index" and
//! "Reading the index", as read at RobCo-Core commit `e004c73`. Fetching a
//! URL is the caller's. This module reads bytes, and [`read_file`] reads a
//! local index.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value};

use super::{
    Abi, Error, PIPOS, Requirement, TEMPLATES, TOOLS, ToolPin, Version, is_host_name,
    is_package_name, is_sha256, is_tool_version, required_formats,
};

/// The index `format` this module reads. A larger one fails.
pub const FORMAT: u64 = 1;

/// A parsed index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    /// Every package version, in file order.
    pub packages: Vec<IndexPackage>,
    /// Every `gcc` and `qemu` version, in file order. Entries of other
    /// tools are skipped.
    pub tools: Vec<IndexTool>,
}

/// A `packages` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPackage {
    /// The package name.
    pub name: String,
    /// The version.
    pub version: Version,
    /// The ABI. Required for `pipos`, absent for `templates`.
    pub abi: Option<Abi>,
    /// Other packages this one needs, in file order.
    pub requires: Vec<Requirement>,
    /// The tools the package names: `gcc` then `qemu` for `pipos`, empty
    /// for any other package.
    pub tools: Vec<ToolPin>,
    /// Format name to format version.
    pub formats: BTreeMap<String, u64>,
    /// Where the archive is.
    pub url: String,
    /// The archive size in bytes.
    pub size: u64,
    /// The archive's SHA-256.
    pub sha256: String,
}

/// A `tools` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexTool {
    /// `gcc` or `qemu`.
    pub name: String,
    /// The upstream version, as written.
    pub version: String,
    /// Host name to archive.
    pub archives: BTreeMap<String, Archive>,
}

/// A tool archive object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// Where the archive is.
    pub url: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256.
    pub sha256: String,
    /// The folder inside the archive that holds `bin/`. Empty when `bin/` is
    /// at the top.
    pub root: String,
}

fn entry(at: &str, what: &str) -> Error {
    Error::IndexEntry(format!("{at} {what}"))
}

fn field<'a>(obj: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a Value, Error> {
    obj.get(key)
        .ok_or_else(|| entry(&format!("{at}.{key}"), "is missing"))
}

fn string<'a>(obj: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a str, Error> {
    field(obj, at, key)?
        .as_str()
        .ok_or_else(|| entry(&format!("{at}.{key}"), "is not a string"))
}

fn formed<'a>(
    obj: &'a Map<String, Value>,
    at: &str,
    key: &str,
    ok: fn(&str) -> bool,
) -> Result<&'a str, Error> {
    let s = string(obj, at, key)?;
    if ok(s) {
        Ok(s)
    } else {
        Err(entry(
            &format!("{at}.{key}"),
            &format!("= {s:?} breaks its form"),
        ))
    }
}

/// Reads a `url` and checks its form for an index from `source`.
fn url<'a>(obj: &'a Map<String, Value>, at: &str, source: IndexSource) -> Result<&'a str, Error> {
    let s = string(obj, at, "url")?;
    if is_url(s, source) {
        Ok(s)
    } else if source == IndexSource::Https && is_archive_path(s) {
        Err(entry(
            &format!("{at}.url"),
            &format!("= {s:?} is a relative path, and only a local index may hold one"),
        ))
    } else {
        Err(entry(
            &format!("{at}.url"),
            &format!("= {s:?} breaks its form"),
        ))
    }
}

fn top<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a Value, Error> {
    obj.get(key).ok_or_else(|| entry(key, "is missing"))
}

fn uint(obj: &Map<String, Value>, at: &str, key: &str) -> Result<u64, Error> {
    field(obj, at, key)?
        .as_u64()
        .ok_or_else(|| entry(&format!("{at}.{key}"), "is not a non-negative integer"))
}

fn object<'a>(v: &'a Value, at: &str) -> Result<&'a Map<String, Value>, Error> {
    v.as_object().ok_or_else(|| entry(at, "is not an object"))
}

fn array<'a>(v: &'a Value, at: &str) -> Result<&'a Vec<Value>, Error> {
    v.as_array().ok_or_else(|| entry(at, "is not an array"))
}

/// The kind of place an index came from. The caller knows it from the
/// `--index` location, by "Where the index is".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexSource {
    /// A local file.
    Local,
    /// A download from an `https://` URL.
    Https,
}

/// The form of a `url`: `https://`, or in a local index a relative path by
/// the path rules of "The archive".
pub fn is_url(s: &str, source: IndexSource) -> bool {
    match s.strip_prefix("https://") {
        Some(rest) => !rest.is_empty() && !s.chars().any(is_control),
        None => source == IndexSource::Local && is_archive_path(s),
    }
}

/// A character below U+0020, or U+007F. No system opens a path that holds
/// one, and no URL holds one.
fn is_control(c: char) -> bool {
    c < '\u{20}' || c == '\u{7f}'
}

/// The path rules of "The archive": relative, `/` as the separator, no
/// empty, `.` or `..` component, no drive letter. It also refuses every
/// character below U+0020 and U+007F. The spec does not name control
/// characters yet; this stricter rule will be written into it at its next
/// update.
pub fn is_archive_path(s: &str) -> bool {
    let b = s.as_bytes();
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    !s.is_empty()
        && !drive
        && !s.contains('\\')
        && !s.chars().any(is_control)
        && s.split('/').all(|c| !matches!(c, "" | "." | ".."))
}

/// Reads a `requires` array, shared with `sdk.json`. `err` builds the error
/// for a place and a reason. An absent array is empty.
pub(super) fn requires(
    v: Option<&Value>,
    at: &str,
    err: &dyn Fn(String) -> Error,
) -> Result<Vec<Requirement>, Error> {
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let here = format!("{at}requires");
    let items = v
        .as_array()
        .ok_or_else(|| err(format!("{here} is not an array")))?;
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let here = format!("{here}[{i}]");
        let obj = item
            .as_object()
            .ok_or_else(|| err(format!("{here} is not an object")))?;
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| is_package_name(s))
            .ok_or_else(|| err(format!("{here}.name is missing or not a package name")))?;
        let abi = obj
            .get("abi")
            .and_then(Value::as_str)
            .and_then(Abi::parse)
            .ok_or_else(|| err(format!("{here}.abi is missing or not MAJOR.MINOR")))?;
        out.push(Requirement {
            name: name.to_owned(),
            abi,
        });
    }
    Ok(out)
}

/// Reads the `abi` of a package, shared with `sdk.json`: required for
/// `pipos`, where it equals the first two parts of the version as text,
/// absent for `templates`, optional for any other package.
pub(super) fn abi(
    v: Option<&Value>,
    name: &str,
    version: Version,
    at: &str,
    err: &dyn Fn(String) -> Error,
) -> Result<Option<Abi>, Error> {
    let here = format!("{at}abi");
    let Some(v) = v else {
        if name == PIPOS {
            return Err(err(format!("{here} is missing, and {PIPOS} requires it")));
        }
        return Ok(None);
    };
    if name == TEMPLATES {
        return Err(err(format!(
            "{here} is present, and {TEMPLATES} must not carry it"
        )));
    }
    let abi = v
        .as_str()
        .and_then(Abi::parse)
        .ok_or_else(|| err(format!("{here} is not MAJOR.MINOR")))?;
    if name == PIPOS && (abi.major, abi.minor) != (version.major, version.minor) {
        return Err(err(format!(
            "{here} = \"{abi}\" does not equal the first two parts of {version}"
        )));
    }
    Ok(Some(abi))
}

/// Reads the `tools` of a package, shared with `sdk.json`: exactly `gcc`
/// then `qemu` for `pipos`, and absent for any other package.
pub(super) fn tool_pins(
    v: Option<&Value>,
    package: &str,
    at: &str,
    err: &dyn Fn(String) -> Error,
) -> Result<Vec<ToolPin>, Error> {
    let here = format!("{at}tools");
    let Some(v) = v else {
        if package == PIPOS {
            return Err(err(format!("{here} is missing")));
        }
        return Ok(Vec::new());
    };
    if package != PIPOS {
        return Err(err(format!(
            "{here} is present, and only {PIPOS} names tools"
        )));
    }
    let items = v
        .as_array()
        .ok_or_else(|| err(format!("{here} is not an array")))?;
    if items.len() != TOOLS.len() {
        return Err(err(format!("{here} must hold exactly gcc then qemu")));
    }
    let mut out = Vec::new();
    for (i, (item, want)) in items.iter().zip(TOOLS).enumerate() {
        let at = format!("{here}[{i}]");
        let obj = item
            .as_object()
            .ok_or_else(|| err(format!("{at} is not an object")))?;
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| err(format!("{at}.name is missing or not a string")))?;
        if name != want {
            return Err(err(format!(
                "{at}.name = {name:?}, and {here} must hold exactly gcc then qemu"
            )));
        }
        let version = obj
            .get("version")
            .and_then(Value::as_str)
            .filter(|s| is_tool_version(s))
            .ok_or_else(|| err(format!("{at}.version is missing or breaks its form")))?;
        out.push(ToolPin {
            name: name.to_owned(),
            version: version.to_owned(),
        });
    }
    Ok(out)
}

/// Reads `formats`, shared with `sdk.json`: an object of integers that
/// holds every name [`required_formats`] gives for the package `name`.
pub(super) fn formats(
    v: Option<&Value>,
    name: &str,
    at: &str,
    err: &dyn Fn(String) -> Error,
) -> Result<BTreeMap<String, u64>, Error> {
    let here = format!("{at}formats");
    let obj = v
        .ok_or_else(|| err(format!("{here} is missing")))?
        .as_object()
        .ok_or_else(|| err(format!("{here} is not an object")))?;
    let mut out = BTreeMap::new();
    for (k, v) in obj {
        // "One integer per format": a format version is a positive integer,
        // as `format` is. `0`, `-1` and `1.0` are malformed, not unknown, so
        // they fail here and are never passed over by rule 4.
        let n = v
            .as_u64()
            .filter(|n| *n >= 1)
            .ok_or_else(|| err(format!("{here}.{k} = {v} is not a positive integer")))?;
        out.insert(k.clone(), n);
    }
    if let Some(m) = required_formats(name)
        .iter()
        .find(|f| !out.contains_key(**f))
    {
        return Err(err(format!("{here}.{m} is missing, and {name} carries it")));
    }
    Ok(out)
}

fn package(v: &Value, at: &str, source: IndexSource) -> Result<IndexPackage, Error> {
    let obj = object(v, at)?;
    let dot = format!("{at}.");
    let name = formed(obj, at, "name", is_package_name)?.to_owned();
    let version = Version::parse(string(obj, at, "version")?).ok_or_else(|| {
        entry(
            &format!("{at}.version"),
            "is not X.Y.Z with no leading zero",
        )
    })?;
    let abi = abi(obj.get("abi"), &name, version, &dot, &Error::IndexEntry)?;
    let requires = requires(obj.get("requires"), &dot, &Error::IndexEntry)?;
    let tools = tool_pins(obj.get("tools"), &name, &dot, &Error::IndexEntry)?;
    let formats = formats(obj.get("formats"), &name, &dot, &Error::IndexEntry)?;
    let url = url(obj, at, source)?.to_owned();
    let size = uint(obj, at, "size")?;
    let sha256 = formed(obj, at, "sha256", is_sha256)?.to_owned();
    Ok(IndexPackage {
        name,
        version,
        abi,
        requires,
        tools,
        formats,
        url,
        size,
        sha256,
    })
}

/// Reads a `tools` entry. `None` when its `name` is a string other than
/// `gcc` and `qemu`: such an entry is skipped whole, and nothing else of it
/// is checked.
fn tool(v: &Value, at: &str, source: IndexSource) -> Result<Option<IndexTool>, Error> {
    let obj = object(v, at)?;
    let name = string(obj, at, "name")?;
    if !TOOLS.contains(&name) {
        return Ok(None);
    }
    let version = formed(obj, at, "version", is_tool_version)?.to_owned();
    let archives_at = format!("{at}.archives");
    let mut archives = BTreeMap::new();
    for (host, a) in object(field(obj, at, "archives")?, &archives_at)? {
        let here = format!("{archives_at}.{host}");
        if !is_host_name(host) {
            return Err(entry(&here, "is not a host name"));
        }
        let a = object(a, &here)?;
        archives.insert(
            host.clone(),
            Archive {
                url: url(a, &here, source)?.to_owned(),
                size: uint(a, &here, "size")?,
                sha256: formed(a, &here, "sha256", is_sha256)?.to_owned(),
                root: string(a, &here, "root")?.to_owned(),
            },
        );
    }
    Ok(Some(IndexTool {
        name: name.to_owned(),
        version,
        archives,
    }))
}

/// Reads a local index file. A file that is missing or cannot be read
/// gives `index: read`.
pub fn read_file(path: &Path) -> Result<Index, Error> {
    let bytes = std::fs::read(path)
        .map_err(|e| Error::IndexRead(format!("{}: {}", path.display(), e.kind())))?;
    parse(&bytes, IndexSource::Local)
}

/// Reads the bytes of an index, in the order of checks of "Reading the
/// index". Fields it does not know are ignored. `source` is where the bytes
/// came from: only a local index may hold a relative `url`.
pub fn parse(bytes: &[u8], source: IndexSource) -> Result<Index, Error> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| Error::IndexSyntax(format!("not UTF-8 at byte {}", e.valid_up_to())))?;
    let root: Value =
        serde_json::from_str(text).map_err(|e| Error::IndexSyntax(format!("not JSON: {e}")))?;
    let obj = root
        .as_object()
        .ok_or_else(|| Error::IndexSyntax("the top value is not an object".to_owned()))?;
    match obj.get("format") {
        None => return Err(Error::IndexFormat("format is missing".to_owned())),
        Some(v) => match v.as_u64() {
            Some(n) if n > FORMAT => {
                return Err(Error::IndexFormat(format!(
                    "the index has format {n} and this tool reads {FORMAT}: install a newer tool"
                )));
            }
            Some(n) if n >= 1 => {}
            _ => {
                return Err(Error::IndexFormat(
                    "format is not a positive integer".to_owned(),
                ));
            }
        },
    }

    let mut packages = Vec::new();
    for (i, v) in array(top(obj, "packages")?, "packages")?.iter().enumerate() {
        packages.push(package(v, &format!("packages[{i}]"), source)?);
    }
    let mut tools = Vec::new();
    for (i, v) in array(top(obj, "tools")?, "tools")?.iter().enumerate() {
        if let Some(t) = tool(v, &format!("tools[{i}]"), source)? {
            tools.push(t);
        }
    }

    let mut seen = BTreeSet::new();
    for p in &packages {
        if !seen.insert((p.name.as_str(), p.version)) {
            return Err(Error::IndexDuplicate {
                name: p.name.clone(),
                version: p.version.to_string(),
            });
        }
    }
    let mut seen = BTreeSet::new();
    for t in &tools {
        if !seen.insert((t.name.as_str(), t.version.as_str())) {
            return Err(Error::IndexDuplicate {
                name: t.name.clone(),
                version: t.version.clone(),
            });
        }
    }
    Ok(Index { packages, tools })
}
