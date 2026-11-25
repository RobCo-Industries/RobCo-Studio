// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the SDK index.
//!
//! Implements RobCo-Core `docs/spec/SDK_PACKAGE.md`, "The index" and
//! "Reading the index", as read at RobCo-Core commit `a7acab5`. Reading a
//! file or a URL is the caller's: this module reads text.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::{Abi, Error, PIPOS, Requirement, TOOLS, ToolPin, Version, sha256};

/// The index `format` this module reads. A larger one fails.
pub const FORMAT: u64 = 1;

/// A parsed index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    /// Every package version, in file order.
    pub packages: Vec<IndexPackage>,
    /// Every tool version, in file order.
    pub tools: Vec<IndexTool>,
}

/// A `packages` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPackage {
    /// The package name.
    pub name: String,
    /// The version.
    pub version: Version,
    /// The ABI. Required for `pipos`, and then equal to the first two parts
    /// of the version.
    pub abi: Option<Abi>,
    /// Other packages this one needs.
    pub requires: Vec<Requirement>,
    /// The tools the package names: `gcc` then `qemu` for `pipos`, empty
    /// for any other package.
    pub tools: Vec<ToolPin>,
    /// Where the archive is.
    pub url: String,
    /// The archive size in bytes.
    pub size: u64,
    /// The archive's SHA-256, lower case.
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
    /// Its SHA-256, lower case.
    pub sha256: String,
    /// The folder inside the archive that holds `bin/`. Empty when `bin/` is
    /// at the top.
    pub root: String,
}

fn entry(at: &str, what: &str) -> Error {
    Error::IndexEntry(format!("`{at}` {what}"))
}

fn field<'a>(obj: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a Value, Error> {
    obj.get(key)
        .ok_or_else(|| entry(&format!("{at}.{key}"), "is missing"))
}

fn string(obj: &Map<String, Value>, at: &str, key: &str) -> Result<String, Error> {
    field(obj, at, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| entry(&format!("{at}.{key}"), "is not a string"))
}

fn uint(obj: &Map<String, Value>, at: &str, key: &str) -> Result<u64, Error> {
    field(obj, at, key)?
        .as_u64()
        .ok_or_else(|| entry(&format!("{at}.{key}"), "is not a non-negative integer"))
}

fn hash(obj: &Map<String, Value>, at: &str, key: &str) -> Result<String, Error> {
    sha256(&string(obj, at, key)?)
        .ok_or_else(|| entry(&format!("{at}.{key}"), "is not 64 hex digits"))
}

fn object<'a>(v: &'a Value, at: &str) -> Result<&'a Map<String, Value>, Error> {
    v.as_object().ok_or_else(|| entry(at, "is not an object"))
}

fn array<'a>(obj: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a Vec<Value>, Error> {
    field(obj, at, key)?
        .as_array()
        .ok_or_else(|| entry(&format!("{at}.{key}"), "is not an array"))
}

/// Reads a `requires` array, shared with `sdk.json`. `err` builds the error
/// for a field path and a reason.
pub(super) fn requires(
    v: Option<&Value>,
    at: &str,
    err: &dyn Fn(String) -> Error,
) -> Result<Vec<Requirement>, Error> {
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let items = v
        .as_array()
        .ok_or_else(|| err(format!("`{at}.requires` is not an array")))?;
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let here = format!("{at}.requires[{i}]");
        let obj = item
            .as_object()
            .ok_or_else(|| err(format!("`{here}` is not an object")))?;
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| err(format!("`{here}.name` is missing or not a string")))?;
        let abi = obj
            .get("abi")
            .and_then(Value::as_str)
            .and_then(Abi::parse)
            .ok_or_else(|| err(format!("`{here}.abi` is missing or not MAJOR.MINOR")))?;
        out.push(Requirement {
            name: name.to_owned(),
            abi,
        });
    }
    Ok(out)
}

/// Reads the `tools` of a package, shared with `sdk.json`: exactly `gcc`
/// then `qemu` for `pipos`, and absent for any other package. `err` builds
/// the error for a reason.
pub(super) fn tool_pins(
    v: Option<&Value>,
    package: &str,
    at: &str,
    err: &dyn Fn(String) -> Error,
) -> Result<Vec<ToolPin>, Error> {
    let here = format!("{at}.tools");
    let Some(v) = v else {
        if package == PIPOS {
            return Err(err(format!("`{here}` is missing")));
        }
        return Ok(Vec::new());
    };
    if package != PIPOS {
        return Err(err(format!(
            "`{here}` is present, and only `{PIPOS}` names tools"
        )));
    }
    let items = v
        .as_array()
        .ok_or_else(|| err(format!("`{here}` is not an array")))?;
    if items.len() != TOOLS.len() {
        return Err(err(format!("`{here}` must hold exactly `gcc` then `qemu`")));
    }
    let mut out = Vec::new();
    for (i, (item, want)) in items.iter().zip(TOOLS).enumerate() {
        let at = format!("{here}[{i}]");
        let obj = item
            .as_object()
            .ok_or_else(|| err(format!("`{at}` is not an object")))?;
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| err(format!("`{at}.name` is missing or not a string")))?;
        if name != want {
            return Err(err(format!(
                "`{at}.name` is `{name}`, and `{here}` must hold exactly `gcc` then `qemu`"
            )));
        }
        let version = obj
            .get("version")
            .and_then(Value::as_str)
            .ok_or_else(|| err(format!("`{at}.version` is missing or not a string")))?;
        out.push(ToolPin {
            name: name.to_owned(),
            version: version.to_owned(),
        });
    }
    Ok(out)
}

fn package(v: &Value, at: &str) -> Result<IndexPackage, Error> {
    let obj = object(v, at)?;
    let name = string(obj, at, "name")?;
    let version = Version::parse(&string(obj, at, "version")?)
        .ok_or_else(|| entry(&format!("{at}.version"), "is not X.Y.Z"))?;
    let abi = match obj.get("abi") {
        None if name == PIPOS => return Err(entry(&format!("{at}.abi"), "is missing")),
        None => None,
        Some(a) => {
            let abi = a
                .as_str()
                .and_then(Abi::parse)
                .ok_or_else(|| entry(&format!("{at}.abi"), "is not MAJOR.MINOR"))?;
            if abi != version.abi() {
                return Err(entry(
                    &format!("{at}.abi"),
                    "does not equal the first two parts of the version",
                ));
            }
            Some(abi)
        }
    };
    Ok(IndexPackage {
        requires: requires(obj.get("requires"), at, &Error::IndexEntry)?,
        tools: tool_pins(obj.get("tools"), &name, at, &Error::IndexEntry)?,
        url: string(obj, at, "url")?,
        size: uint(obj, at, "size")?,
        sha256: hash(obj, at, "sha256")?,
        name,
        version,
        abi,
    })
}

fn tool(v: &Value, at: &str) -> Result<IndexTool, Error> {
    let obj = object(v, at)?;
    let archives_at = format!("{at}.archives");
    let mut archives = BTreeMap::new();
    for (host, a) in object(field(obj, at, "archives")?, &archives_at)? {
        let here = format!("{archives_at}.{host}");
        let a = object(a, &here)?;
        archives.insert(
            host.clone(),
            Archive {
                url: string(a, &here, "url")?,
                size: uint(a, &here, "size")?,
                sha256: hash(a, &here, "sha256")?,
                root: string(a, &here, "root")?,
            },
        );
    }
    Ok(IndexTool {
        name: string(obj, at, "name")?,
        version: string(obj, at, "version")?,
        archives,
    })
}

/// Reads the text of an index. Fields it does not know are ignored.
pub fn parse(text: &str) -> Result<Index, Error> {
    let root: Value =
        serde_json::from_str(text).map_err(|e| Error::IndexRead(format!("not JSON: {e}")))?;
    let obj = root
        .as_object()
        .ok_or_else(|| Error::IndexRead("not a JSON object".to_owned()))?;
    match obj.get("format").map(Value::as_u64) {
        None => return Err(Error::IndexFormat("`format` is missing".to_owned())),
        Some(None) | Some(Some(0)) => {
            return Err(Error::IndexFormat(
                "`format` is not a positive integer".to_owned(),
            ));
        }
        Some(Some(n)) if n > FORMAT => {
            return Err(Error::IndexFormat(format!(
                "the index has format {n} and this tool reads {FORMAT}: update the tool"
            )));
        }
        Some(Some(_)) => {}
    }

    let mut packages = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, v) in array(obj, "index", "packages")?.iter().enumerate() {
        let p = package(v, &format!("packages[{i}]"))?;
        if !seen.insert((p.name.clone(), p.version)) {
            return Err(Error::IndexDuplicate {
                name: p.name,
                version: p.version.to_string(),
            });
        }
        packages.push(p);
    }

    let mut tools = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, v) in array(obj, "index", "tools")?.iter().enumerate() {
        let t = tool(v, &format!("tools[{i}]"))?;
        if !seen.insert((t.name.clone(), t.version.clone())) {
            return Err(Error::IndexDuplicate {
                name: t.name,
                version: t.version,
            });
        }
        tools.push(t);
    }

    Ok(Index { packages, tools })
}
