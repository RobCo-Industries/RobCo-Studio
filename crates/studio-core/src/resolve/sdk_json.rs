// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads a package's `sdk.json`.
//!
//! Implements RobCo-Core `docs/spec/SDK_PACKAGE.md`, "sdk.json", "Which
//! package carries which field", "Reading sdk.json", "What must equal the
//! index entry", "Tools a package names" and "Firmware pins", as read at
//! RobCo-Core commit `e004c73`. This module reads text and checks fields.
//! Checking files and hashes on disk is the installer's.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::index::{self, IndexPackage};
use super::{
    Abi, Error, KNOWN_FORMATS, PIPOS, Requirement, TEMPLATES, ToolPin, Version, is_package_name,
    is_sha256,
};

/// The `sdk.json` `format` this module reads. A larger one fails.
pub const FORMAT: u64 = 1;

/// A parsed `sdk.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkJson {
    /// The package name.
    pub name: String,
    /// The version.
    pub version: Version,
    /// The ABI. Required for `pipos`, absent for `templates`.
    pub abi: Option<Abi>,
    /// One per repository that went into the package.
    pub built_from: Vec<BuiltFrom>,
    /// Format name to format version.
    pub formats: BTreeMap<String, i64>,
    /// Other packages this one needs, in file order.
    pub requires: Vec<Requirement>,
    /// The tools the package names: `gcc` then `qemu` for `pipos`, empty
    /// for any other package.
    pub tools: Vec<ToolPin>,
    /// The firmware pins. Required for `pipos`, absent for `templates`.
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
    /// File name to SHA-256.
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
    /// The SHA-256.
    pub sha256: String,
}

/// The repository of the firmware pins.
const FIRMWARE_REPO: &str = "raspberrypi/firmware";

/// The firmware files a `pipos` package pins, each once, in any order.
const FIRMWARE_FILES: [&str; 3] = ["bootcode.bin", "start.elf", "fixup.dat"];

fn bad(at: &str, what: &str) -> Error {
    Error::PackageManifest(format!("sdk.json: {at} {what}"))
}

fn manifest(d: String) -> Error {
    Error::PackageManifest(format!("sdk.json: {d}"))
}

fn obj<'a>(v: &'a Value, at: &str) -> Result<&'a Map<String, Value>, Error> {
    v.as_object().ok_or_else(|| bad(at, "is not an object"))
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
    if is_sha256(&s) {
        Ok(s)
    } else {
        Err(bad(
            &format!("{at}{key}"),
            "is not 64 lower case hex digits",
        ))
    }
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

/// Step 3 for `formats`: a format this tool does not know, by its name or
/// by its value, fails with `package: format` and names it. A value that
/// is not a positive integer is malformed, so its message gives no advice
/// about a newer tool. A `formats` that is missing or not an object is
/// left to the field checks.
fn check_formats(v: Option<&Value>) -> Result<(), Error> {
    let Some(formats) = v.and_then(Value::as_object) else {
        return Ok(());
    };
    for (name, value) in formats {
        let Some(n) = value.as_u64().filter(|n| *n >= 1) else {
            return Err(Error::PackageFormat(format!(
                "sdk.json: formats.{name} = {value} is not a positive integer"
            )));
        };
        let known = i64::try_from(n).is_ok_and(|n| KNOWN_FORMATS.contains(&(name.as_str(), n)));
        if !known {
            return Err(Error::PackageFormat(format!(
                "sdk.json uses {name} {n}, which this tool does not know: install a newer tool"
            )));
        }
    }
    Ok(())
}

/// Reads the bytes of an `sdk.json`, in the order of "Reading sdk.json":
/// the file parses, `format`, `formats`, then each field in the order of
/// the table. Fields it does not know are ignored.
pub fn parse(bytes: &[u8]) -> Result<SdkJson, Error> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| manifest("the file is not UTF-8".to_owned()))?;
    let root: Value =
        serde_json::from_str(text).map_err(|e| manifest(format!("the file is not JSON: {e}")))?;
    let o = root
        .as_object()
        .ok_or_else(|| manifest("the top value is not an object".to_owned()))?;
    match o.get("format").map(Value::as_u64) {
        None => {
            return Err(Error::PackageFormat(
                "sdk.json: format is missing".to_owned(),
            ));
        }
        Some(Some(n)) if n > FORMAT => {
            return Err(Error::PackageFormat(format!(
                "sdk.json has format {n} and this tool reads {FORMAT}: install a newer tool"
            )));
        }
        Some(Some(n)) if n >= 1 => {}
        Some(_) => {
            return Err(Error::PackageFormat(
                "sdk.json: format is not a positive integer".to_owned(),
            ));
        }
    }
    check_formats(o.get("formats"))?;

    let name = string(o, "", "name")?;
    if !is_package_name(&name) {
        return Err(bad("name", &format!("= {name:?} is not a package name")));
    }
    let version = Version::parse(&string(o, "", "version")?)
        .ok_or_else(|| bad("version", "is not X.Y.Z with no leading zero"))?;
    let abi = index::abi(o.get("abi"), &name, version, "", &manifest)?;

    let mut built_from = Vec::new();
    for (i, v) in arr(o, "", "built_from")?.iter().enumerate() {
        let at = format!("built_from[{i}].");
        let e = obj(v, &at)?;
        built_from.push(BuiltFrom {
            repo: string(e, &at, "repo")?,
            commit: commit(e, &at)?,
        });
    }

    let formats = index::formats(o.get("formats"), &name, "", &manifest)?;
    let requires = index::requires(o.get("requires"), "", &manifest)?;
    let tools = index::tool_pins(o.get("tools"), &name, "", &manifest)?;

    let firmware = match o.get("firmware") {
        None if name == PIPOS => return Err(bad("firmware", "is missing")),
        None => None,
        Some(_) if name == TEMPLATES => {
            return Err(bad(
                "firmware",
                "is present, and templates must not carry it",
            ));
        }
        Some(v) => {
            let at = "firmware.";
            let f = obj(v, "firmware")?;
            let repo = string(f, at, "repo")?;
            if repo != FIRMWARE_REPO {
                return Err(bad(
                    "firmware.repo",
                    &format!("= {repo:?} is not {FIRMWARE_REPO:?}"),
                ));
            }
            let tag = string(f, at, "tag")?;
            let commit = commit(f, at)?;
            let base_url = string(f, at, "base_url")?;
            let mut files = Vec::new();
            for (i, v) in arr(f, at, "files")?.iter().enumerate() {
                let at = format!("firmware.files[{i}].");
                let e = obj(v, &at)?;
                let file = string(e, &at, "name")?;
                if !FIRMWARE_FILES.contains(&file.as_str()) {
                    return Err(bad(
                        &format!("{at}name = {file:?}"),
                        "is not one of bootcode.bin, start.elf, fixup.dat",
                    ));
                }
                if files.iter().any(|(n, _)| *n == file) {
                    return Err(bad(&format!("{at}name = {file:?}"), "is listed twice"));
                }
                files.push((file, hash(e, &at, "sha256")?));
            }
            if let Some(missing) = FIRMWARE_FILES
                .iter()
                .find(|n| !files.iter().any(|(f, _)| f == *n))
            {
                return Err(bad("firmware.files", &format!("lacks {missing}")));
            }
            Some(Firmware {
                repo,
                tag,
                commit,
                base_url,
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
        // A path is an entry name, so it keeps the path rules of the archive.
        if !index::is_archive_path(&f.path) {
            return Err(bad(
                &format!("{at}path = {:?}", f.path),
                "breaks the path rules of the archive",
            ));
        }
        // Strictly ascending byte order: a path out of order or twice fails.
        if let Some(prev) = files.last()
            && prev.path.as_bytes() >= f.path.as_bytes()
        {
            return Err(bad(
                &format!("{at}path = {:?}", f.path),
                "is not after the path before it: files must be sorted by path in byte order, with no repeat",
            ));
        }
        files.push(f);
    }

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
    /// Checks an installed `sdk.json` against its index entry, by "What must
    /// equal the index entry": `name`, `version`, `abi`, `requires` and
    /// `tools` in order, and `formats` by name. A difference fails with
    /// `package: manifest` and names the field.
    ///
    /// Both structs are taken apart with no `..`, so a field added to
    /// either one later does not compile until it is placed here.
    pub fn check_against(&self, entry: &IndexPackage) -> Result<(), Error> {
        let SdkJson {
            name,
            version,
            abi,
            requires,
            tools,
            formats,
            // Only in sdk.json: the index has no such field.
            built_from: _,
            firmware: _,
            licences: _,
            files: _,
        } = self;
        let IndexPackage {
            name: index_name,
            version: index_version,
            abi: index_abi,
            requires: index_requires,
            tools: index_tools,
            formats: index_formats,
            // Only in the index: they describe the archive.
            url: _,
            size: _,
            sha256: _,
        } = entry;
        let differ = |field: &str| {
            Err(Error::PackageManifest(format!(
                "sdk.json: {field} differs from the index entry of {index_name} {index_version}"
            )))
        };
        if name != index_name {
            return differ("name");
        }
        if version != index_version {
            return differ("version");
        }
        if abi != index_abi {
            return differ("abi");
        }
        if requires != index_requires {
            return differ("requires");
        }
        if tools != index_tools {
            return differ("tools");
        }
        if formats != index_formats {
            return differ("formats");
        }
        Ok(())
    }
}
