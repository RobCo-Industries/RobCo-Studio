// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads a package's `sdk.json`.
//!
//! Implements RobCo-Core `docs/spec/SDK_PACKAGE.md`, "sdk.json", "Reading
//! sdk.json" and "Firmware pins", as read at RobCo-Core commit `a7acab5`,
//! and "Tools a package names", as read in review at RobCo-Core `dc294ad`.
//! This module reads text and checks fields. Checking files and hashes on
//! disk is the installer's.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::index::IndexPackage;
use super::{Abi, Error, KNOWN_FORMATS, PIPOS, Requirement, ToolPin, Version, sha256};

/// The `sdk.json` `format` this module reads. A larger one fails.
pub const FORMAT: u64 = 1;

/// A parsed `sdk.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkJson {
    /// The package name.
    pub name: String,
    /// The version.
    pub version: Version,
    /// The ABI. Required for `pipos`, and equal to the first two parts of
    /// the version.
    pub abi: Option<Abi>,
    /// One per repository that went into the package.
    pub built_from: Vec<BuiltFrom>,
    /// Format name to format version.
    pub formats: BTreeMap<String, u64>,
    /// Other packages this one needs.
    pub requires: Vec<Requirement>,
    /// The tools the package names: `gcc` then `qemu` for `pipos`, empty
    /// for any other package.
    pub tools: Vec<ToolPin>,
    /// The firmware pins. Required for `pipos`.
    pub firmware: Option<Firmware>,
    /// The licence of each part.
    pub licences: Vec<Licence>,
    /// Every file in the archive but `sdk.json`, sorted by path.
    pub files: Vec<FileEntry>,
}

/// A `built_from` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltFrom {
    /// The repository name.
    pub repo: String,
    /// The full commit, 40 lower case hex digits.
    pub commit: String,
}

/// The firmware pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firmware {
    /// `raspberrypi/firmware`.
    pub repo: String,
    /// The release tag.
    pub tag: String,
    /// The full commit of that tag.
    pub commit: String,
    /// The download base.
    pub base_url: String,
    /// File name to SHA-256, lower case.
    pub files: Vec<(String, String)>,
}

/// A `licences` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Licence {
    /// Path prefixes in the archive.
    pub paths: Vec<String>,
    /// The SPDX identifier.
    pub spdx: String,
}

/// A `files` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// The entry name in the archive.
    pub path: String,
    /// The size in bytes.
    pub size: u64,
    /// The SHA-256, lower case.
    pub sha256: String,
}

fn bad(at: &str, what: &str) -> Error {
    Error::PackageManifest(format!("`{at}` {what}"))
}

fn obj<'a>(v: &'a Value, at: &str) -> Result<&'a Map<String, Value>, Error> {
    v.as_object()
        .ok_or_else(|| bad(at, "is missing or not an object"))
}

fn get<'a>(o: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a Value, Error> {
    o.get(key)
        .ok_or_else(|| bad(&format!("{at}{key}"), "is missing"))
}

fn string(o: &Map<String, Value>, at: &str, key: &str) -> Result<String, Error> {
    get(o, at, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| bad(&format!("{at}{key}"), "is not a string"))
}

fn arr<'a>(o: &'a Map<String, Value>, at: &str, key: &str) -> Result<&'a Vec<Value>, Error> {
    get(o, at, key)?
        .as_array()
        .ok_or_else(|| bad(&format!("{at}{key}"), "is not an array"))
}

fn hash(o: &Map<String, Value>, at: &str, key: &str) -> Result<String, Error> {
    let s = string(o, at, key)?;
    sha256(&s).ok_or_else(|| bad(&format!("{at}{key}"), "is not 64 hex digits"))
}

fn commit(o: &Map<String, Value>, at: &str) -> Result<String, Error> {
    let s = string(o, at, "commit")?;
    if s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(s)
    } else {
        Err(bad(
            &format!("{at}commit"),
            "is not 40 lower case hex digits",
        ))
    }
}

/// Checks `formats` against [`KNOWN_FORMATS`]: rule 4 of the fit rules.
/// A format the tool does not know, or a value it does not read, fails with
/// `package: format` and names the format.
pub fn check_formats(formats: &BTreeMap<String, u64>) -> Result<(), Error> {
    for (name, value) in formats {
        match KNOWN_FORMATS.iter().find(|(n, _)| n == name) {
            None => {
                return Err(Error::PackageFormat(format!(
                    "the package uses the format `{name}`, which this tool does not know: update the tool"
                )));
            }
            Some((_, known)) if value != known => {
                return Err(Error::PackageFormat(format!(
                    "the package uses `{name}` format {value}, and this tool reads {known}: update the tool"
                )));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Reads the text of an `sdk.json`. Fields it does not know are ignored.
pub fn parse(text: &str) -> Result<SdkJson, Error> {
    let root: Value = serde_json::from_str(text)
        .map_err(|e| Error::PackageManifest(format!("sdk.json is not JSON: {e}")))?;
    let o = obj(&root, "sdk.json")?;
    let format = get(o, "", "format")?
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| bad("format", "is not a positive integer"))?;
    if format > FORMAT {
        return Err(Error::PackageFormat(format!(
            "sdk.json has format {format} and this tool reads {FORMAT}: update the tool"
        )));
    }

    let name = string(o, "", "name")?;
    let version =
        Version::parse(&string(o, "", "version")?).ok_or_else(|| bad("version", "is not X.Y.Z"))?;
    let abi = match o.get("abi") {
        None if name == PIPOS => return Err(bad("abi", "is missing")),
        None => None,
        Some(v) => {
            let abi = v
                .as_str()
                .and_then(Abi::parse)
                .ok_or_else(|| bad("abi", "is not MAJOR.MINOR"))?;
            if abi != version.abi() {
                return Err(bad(
                    "abi",
                    "does not equal the first two parts of the version",
                ));
            }
            Some(abi)
        }
    };

    let mut built_from = Vec::new();
    for (i, v) in arr(o, "", "built_from")?.iter().enumerate() {
        let at = format!("built_from[{i}].");
        let e = obj(v, &at)?;
        built_from.push(BuiltFrom {
            repo: string(e, &at, "repo")?,
            commit: commit(e, &at)?,
        });
    }

    let mut formats = BTreeMap::new();
    for (k, v) in obj(get(o, "", "formats")?, "formats")? {
        let n = v
            .as_u64()
            .ok_or_else(|| bad(&format!("formats.{k}"), "is not an integer"))?;
        formats.insert(k.clone(), n);
    }

    let requires = super::index::requires(o.get("requires"), "sdk.json", &Error::PackageManifest)?;
    let tools =
        super::index::tool_pins(o.get("tools"), &name, "sdk.json", &Error::PackageManifest)?;

    let firmware = match o.get("firmware") {
        None if name == PIPOS => return Err(bad("firmware", "is missing")),
        None => None,
        Some(v) => {
            let at = "firmware.";
            let f = obj(v, "firmware")?;
            let mut files = Vec::new();
            for (i, v) in arr(f, at, "files")?.iter().enumerate() {
                let at = format!("firmware.files[{i}].");
                let e = obj(v, &at)?;
                files.push((string(e, &at, "name")?, hash(e, &at, "sha256")?));
            }
            Some(Firmware {
                repo: string(f, at, "repo")?,
                tag: string(f, at, "tag")?,
                commit: commit(f, at)?,
                base_url: string(f, at, "base_url")?,
                files,
            })
        }
    };

    let mut licences = Vec::new();
    for (i, v) in arr(o, "", "licences")?.iter().enumerate() {
        let at = format!("licences[{i}].");
        let e = obj(v, &at)?;
        let mut paths = Vec::new();
        for p in arr(e, &at, "paths")? {
            paths.push(
                p.as_str().map(str::to_owned).ok_or_else(|| {
                    bad(&format!("{at}paths"), "holds a value that is not a string")
                })?,
            );
        }
        licences.push(Licence {
            paths,
            spdx: string(e, &at, "spdx")?,
        });
    }

    let mut files: Vec<FileEntry> = Vec::new();
    for (i, v) in arr(o, "", "files")?.iter().enumerate() {
        let at = format!("files[{i}].");
        let e = obj(v, &at)?;
        let f = FileEntry {
            path: string(e, &at, "path")?,
            size: get(e, &at, "size")?
                .as_u64()
                .ok_or_else(|| bad(&format!("{at}size"), "is not a non-negative integer"))?,
            sha256: hash(e, &at, "sha256")?,
        };
        if let Some(prev) = files.last()
            && prev.path.as_bytes() >= f.path.as_bytes()
        {
            return Err(bad(
                &format!("{at}path"),
                "is not after the path before it: `files` must be sorted by path in byte order, with no repeat",
            ));
        }
        files.push(f);
    }

    check_formats(&formats)?;

    Ok(SdkJson {
        name,
        version,
        abi,
        built_from,
        formats,
        requires,
        tools,
        firmware,
        licences,
        files,
    })
}

impl SdkJson {
    /// Checks an installed `sdk.json` against its index entry: the same
    /// name, version and `tools`. A difference fails with
    /// `package: manifest`.
    pub fn check_against(&self, entry: &IndexPackage) -> Result<(), Error> {
        if self.name != entry.name || self.version != entry.version {
            return Err(Error::PackageManifest(format!(
                "sdk.json names {} {}, and the index names {} {}",
                self.name, self.version, entry.name, entry.version
            )));
        }
        if self.tools != entry.tools {
            return Err(Error::PackageManifest(format!(
                "the `tools` of sdk.json differ from those of the index entry of {} {}",
                entry.name, entry.version
            )));
        }
        Ok(())
    }
}
