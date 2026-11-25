// SPDX-License-Identifier: GPL-3.0-or-later

//! Which SDK packages and tools a project builds with, and its lock file.
//!
//! Implements RobCo-Core `docs/spec/SDK_PACKAGE.md`, sections "The index",
//! "sdk.json" and "Version and ABI rules", and `docs/spec/STUDIO_PROJECT.md`,
//! section "robco.lock", as read at RobCo-Core commit `a7acab5`, with the
//! tool pins of "Tools a package names" and "What the lock pins" as read in
//! review at RobCo-Core commit `dc294ad`.
//!
//! - [`index`] reads the index.
//! - [`sdk_json`] reads a package's `sdk.json`.
//! - This module holds versions, the fit rules and the resolver.
//! - [`lock`] reads, checks and writes `robco.lock`.
//!
//! Everything here is pure code, except [`lock`], which reads and writes
//! files in a folder the caller names. Nothing downloads or starts a process.

pub mod index;
pub mod lock;
pub mod sdk_json;

use std::collections::BTreeMap;
use std::fmt;

use index::{Index, IndexPackage, IndexTool};
use lock::{Lock, LockedPackage, LockedTool};

/// The tools every lock lists, by name, in lock order.
pub const TOOLS: [&str; 2] = ["gcc", "qemu"];

/// The only platform a format 1 project targets.
pub const PIPOS: &str = "pipos";

/// The formats of `sdk.json` `formats` this tool knows, with the value it
/// reads. Rule 4 of the fit rules uses this list.
pub const KNOWN_FORMATS: [(&str, u64); 7] = [
    ("boot", 1),
    ("flags", 1),
    ("project", 1),
    ("rimg", 1),
    ("runner", 1),
    ("scenario", 1),
    ("templates", 1),
];

/// Why resolving, reading the index, reading `sdk.json` or using the lock
/// failed. Each variant is one error code of the specs; [`Error::code`]
/// gives it as the spec spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `index: read`. The index text is not JSON. The specs give no code
    /// for this case; see the module report.
    IndexRead(String),
    /// `index: format`. `format` is missing, not a positive integer, or
    /// larger than this tool reads.
    IndexFormat(String),
    /// `index: entry`. A field is missing or has the wrong type. Names it.
    IndexEntry(String),
    /// `index: duplicate`. Two entries share a name and a version.
    IndexDuplicate {
        /// The name.
        name: String,
        /// The version.
        version: String,
    },
    /// `index: host`. The chosen tool has no archive for the running host.
    IndexHost {
        /// The tool.
        tool: String,
        /// Its version.
        version: String,
        /// The running host.
        host: String,
    },
    /// `package: format`. `sdk.json` has a larger `format`, or names a format
    /// this tool does not know.
    PackageFormat(String),
    /// `package: manifest`. A required field of `sdk.json` is missing or has
    /// the wrong type.
    PackageManifest(String),
    /// `sdk: no package`. No package fits the platform and ABI.
    NoPackage {
        /// The platform, or the package that a `requires` entry names.
        platform: String,
        /// The ABI asked for.
        abi: Abi,
    },
    /// `sdk: no tool`. The index has no tool with the name and version that
    /// the package names.
    NoTool {
        /// The tool.
        name: String,
        /// The version the package names.
        version: String,
    },
    /// `lock: tool`. A `[[tool]]` version differs from the one the installed
    /// `pipos` package names.
    LockTool {
        /// The tool.
        name: String,
        /// The version in the lock.
        locked: String,
        /// The version the package names.
        named: String,
    },
    /// `lock: syntax`. `robco.lock` does not parse, or a key has the wrong
    /// type or value.
    LockSyntax(String),
    /// `lock: format`. `format` is missing, not a positive integer, or larger
    /// than this tool reads.
    LockFormat(String),
    /// `lock: unknown key`. Holds the dotted name.
    LockUnknownKey(String),
    /// `lock: stale`. Holds the detail, which names the first package or
    /// tool that fails.
    LockStale(String),
    /// `lock: hash`. An archive does not match its lock hash. Names it.
    LockHash(String),
    /// `lock: missing`. The index has no entry for a locked pin. Names it.
    LockMissing(String),
    /// `lock: io`. `robco.lock` could not be read or written. The specs give
    /// no code for this case; see the module report.
    LockIo(String),
}

impl Error {
    /// The error code, as the spec spells it, such as `lock: stale`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::IndexRead(_) => "index: read",
            Self::IndexFormat(_) => "index: format",
            Self::IndexEntry(_) => "index: entry",
            Self::IndexDuplicate { .. } => "index: duplicate",
            Self::IndexHost { .. } => "index: host",
            Self::PackageFormat(_) => "package: format",
            Self::PackageManifest(_) => "package: manifest",
            Self::NoPackage { .. } => "sdk: no package",
            Self::NoTool { .. } => "sdk: no tool",
            Self::LockTool { .. } => "lock: tool",
            Self::LockSyntax(_) => "lock: syntax",
            Self::LockFormat(_) => "lock: format",
            Self::LockUnknownKey(_) => "lock: unknown key",
            Self::LockStale(_) => "lock: stale",
            Self::LockHash(_) => "lock: hash",
            Self::LockMissing(_) => "lock: missing",
            Self::LockIo(_) => "lock: io",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::IndexRead(d)
            | Self::IndexFormat(d)
            | Self::IndexEntry(d)
            | Self::PackageFormat(d)
            | Self::PackageManifest(d)
            | Self::LockSyntax(d)
            | Self::LockFormat(d)
            | Self::LockUnknownKey(d)
            | Self::LockHash(d)
            | Self::LockMissing(d)
            | Self::LockIo(d) => write!(f, "{code}: {d}"),
            Self::IndexDuplicate { name, version } => {
                write!(f, "{code}: {name} {version} is listed twice")
            }
            Self::IndexHost {
                tool,
                version,
                host,
            } => write!(f, "{code}: {tool} {version} has no archive for {host}"),
            Self::NoPackage { platform, abi } => {
                write!(f, "{code}: nothing fits {platform} ABI {abi}")
            }
            Self::NoTool { name, version } => {
                write!(f, "{code}: the index lists no {name} {version}")
            }
            Self::LockTool {
                name,
                locked,
                named,
            } => write!(
                f,
                "{code}: the lock pins {name} {locked} and the package names {name} {named}: run robco update"
            ),
            Self::LockStale(d) => write!(f, "{code}: {d}: run robco update"),
        }
    }
}

impl std::error::Error for Error {}

/// Parses a decimal part: ASCII digits only, leading zeros allowed, at most
/// 65535.
fn parse_part(s: &str) -> Option<u16> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut n: u32 = 0;
    for b in s.bytes() {
        n = n.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
        if n > u32::from(u16::MAX) {
            return None;
        }
    }
    u16::try_from(n).ok()
}

/// A package version, `X.Y.Z`, each part from 0 to 65535.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// X.
    pub major: u16,
    /// Y.
    pub minor: u16,
    /// Z.
    pub patch: u16,
}

impl Version {
    /// Reads `X.Y.Z`. Returns `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split('.');
        let v = Self {
            major: parse_part(it.next()?)?,
            minor: parse_part(it.next()?)?,
            patch: parse_part(it.next()?)?,
        };
        it.next().is_none().then_some(v)
    }

    /// The first two parts, which are the ABI of a package.
    pub fn abi(self) -> Abi {
        Abi {
            major: self.major,
            minor: self.minor,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// An ABI, `MAJOR.MINOR`, each part from 0 to 65535.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Abi {
    /// MAJOR.
    pub major: u16,
    /// MINOR.
    pub minor: u16,
}

impl Abi {
    /// Reads `MAJOR.MINOR`. Returns `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        let (a, b) = s.split_once('.')?;
        Some(Self {
            major: parse_part(a)?,
            minor: parse_part(b)?,
        })
    }

    /// Rules 2 and 3 of the fit rules: a package with ABI `self` serves a
    /// need for ABI `need` when the majors are equal and the package's minor
    /// is at least the need's.
    pub fn serves(self, need: Abi) -> bool {
        self.major == need.major && self.minor >= need.minor
    }
}

impl fmt::Display for Abi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// What a project asks for: `target.platform` and `target.abi` of its
/// `robco.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `target.platform`, such as `pipos`.
    pub platform: String,
    /// `target.abi`.
    pub abi: Abi,
}

/// A `requires` entry of a package: another package, at a lowest ABI.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Requirement {
    /// The package name.
    pub name: String,
    /// The lowest ABI.
    pub abi: Abi,
}

/// Rules 1 to 3 of the fit rules, for a `pipos` package as the index lists
/// it. Rule 4 needs the package's `sdk.json`: see [`fits_sdk`].
pub fn fits(target: &Target, package: &IndexPackage) -> bool {
    target.platform == PIPOS
        && package.name == PIPOS
        && package.abi.is_some_and(|abi| abi.serves(target.abi))
}

/// All four fit rules, for a `pipos` package by its `sdk.json`.
pub fn fits_sdk(target: &Target, sdk: &sdk_json::SdkJson) -> bool {
    target.platform == PIPOS
        && sdk.name == PIPOS
        && sdk.abi.is_some_and(|abi| abi.serves(target.abi))
        && sdk_json::check_formats(&sdk.formats).is_ok()
}

/// Whether a package serves a `requires` entry: rules 2 and 3. A package
/// without `abi` has the first two parts of its version as its ABI, the
/// rule `sdk.json` gives for `pipos`.
fn fits_requirement(req: &Requirement, package: &IndexPackage) -> bool {
    req.name == package.name && package.abi.unwrap_or(package.version.abi()).serves(req.abi)
}

/// A tool that a package names, at one exact version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPin {
    /// `gcc` or `qemu`.
    pub name: String,
    /// The version, compared as a string.
    pub version: String,
}

/// Picks the index entry of a tool, `gcc` or `qemu`, for a resolved package.
///
/// This is the one place that decides which tool version a project uses.
/// It takes the version the package names in `tools`, compared byte for
/// byte, and never the newest in the index. This rule is spec text in
/// review, not yet on RobCo-Core main, and may still change.
pub fn pick_tool<'a>(
    index: &'a Index,
    package: &IndexPackage,
    name: &str,
) -> Result<&'a IndexTool, Error> {
    let pin = package
        .tools
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| {
            Error::IndexEntry(format!(
                "{} {} names no `{name}` in `tools`",
                package.name, package.version
            ))
        })?;
    index
        .tools
        .iter()
        .find(|t| t.name == pin.name && t.version == pin.version)
        .ok_or_else(|| Error::NoTool {
            name: pin.name.clone(),
            version: pin.version.clone(),
        })
}

/// Picks the newest package of `name` that serves every need.
fn pick_package<'a>(
    index: &'a Index,
    name: &str,
    needs: &[Requirement],
) -> Option<&'a IndexPackage> {
    index
        .packages
        .iter()
        .filter(|p| p.name == name && needs.iter().all(|r| fits_requirement(r, p)))
        .max_by_key(|p| p.version)
}

/// Resolves a project against an index, as if no lock existed, and returns
/// the lock to write.
///
/// `location` is the index as the caller was given it, kept for the record.
/// `host` is the running host, such as `macos-arm64`: each chosen tool must
/// have an archive for it. The lock lists the archives of every host.
pub fn resolve(target: &Target, index: &Index, location: &str, host: &str) -> Result<Lock, Error> {
    let no_package = || Error::NoPackage {
        platform: target.platform.clone(),
        abi: target.abi,
    };
    if target.platform != PIPOS {
        return Err(no_package());
    }
    // The newest pipos, then the closure of `requires`, until nothing moves.
    // Each round re-picks every package from the needs of the last round, so
    // a pick never depends on the order of the index.
    let root = Requirement {
        name: PIPOS.to_owned(),
        abi: target.abi,
    };
    let mut picks: BTreeMap<String, &IndexPackage> = BTreeMap::new();
    for _ in 0..=index.packages.len() {
        let mut needs: BTreeMap<String, Vec<Requirement>> = BTreeMap::new();
        needs
            .entry(root.name.clone())
            .or_default()
            .push(root.clone());
        for p in picks.values() {
            for r in &p.requires {
                needs.entry(r.name.clone()).or_default().push(r.clone());
            }
        }
        let mut next = BTreeMap::new();
        for (name, reqs) in &needs {
            match pick_package(index, name, reqs) {
                Some(p) => {
                    next.insert(name.clone(), p);
                }
                None if *name == root.name => return Err(no_package()),
                None => {
                    let abi = reqs.iter().map(|r| r.abi).max().unwrap_or(target.abi);
                    return Err(Error::NoPackage {
                        platform: name.clone(),
                        abi,
                    });
                }
            }
        }
        if next == picks {
            break;
        }
        picks = next;
    }

    let pipos = picks.get(PIPOS).copied().ok_or_else(no_package)?;
    let packages = picks
        .values()
        .map(|p| LockedPackage {
            name: p.name.clone(),
            version: p.version,
            sha256: p.sha256.clone(),
        })
        .collect();

    let mut tools = Vec::new();
    for name in TOOLS {
        let tool = pick_tool(index, pipos, name)?;
        if !tool.archives.contains_key(host) {
            return Err(Error::IndexHost {
                tool: tool.name.clone(),
                version: tool.version.clone(),
                host: host.to_owned(),
            });
        }
        tools.push(LockedTool {
            name: tool.name.clone(),
            version: tool.version.clone(),
            archives: tool
                .archives
                .iter()
                .map(|(h, a)| (h.clone(), a.sha256.clone()))
                .collect(),
        });
    }

    Ok(Lock {
        index: location.to_owned(),
        packages,
        tools,
    })
}

/// The host name of the running system, as the index spells it, or `None`
/// on a system the index has no name for.
pub fn running_host() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "x86_64") => Some("macos-x64"),
        ("macos", "aarch64") => Some("macos-arm64"),
        ("windows", "x86_64") => Some("windows-x64"),
        _ => None,
    }
}

/// Checks a SHA-256 string: 64 hex digits of either case. Returns it in
/// lower case, the form the specs compare and write.
fn sha256(s: &str) -> Option<String> {
    (s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_ascii_lowercase())
}

#[cfg(test)]
mod tests;
