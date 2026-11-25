// SPDX-License-Identifier: GPL-3.0-or-later

//! Which SDK packages and tools a project builds with, and its lock file.
//!
//! Implements RobCo-Core `docs/spec/SDK_PACKAGE.md`, sections "The index",
//! "Reading the index", "A closed index", "sdk.json", "Tools a package
//! names", "What must equal the index entry", "Version and ABI rules",
//! "Resolving requires" and "Hashes", and `docs/spec/STUDIO_PROJECT.md`,
//! section "robco.lock", as read at RobCo-Core commit `e004c73`.
//!
//! - [`index`] reads the index.
//! - [`sdk_json`] reads a package's `sdk.json`.
//! - This module holds versions, the fit rules, the resolver and the
//!   closure check of an index writer.
//! - [`lock`] reads, checks and writes `robco.lock`.
//!
//! Everything here is pure code, except [`lock`], which reads and writes
//! files in a folder the caller names. Nothing downloads or starts a process.

pub mod index;
pub mod lock;
pub mod sdk_json;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::codes::ErrorCode;
use index::{Index, IndexPackage, IndexTool};
use lock::{Lock, LockedPackage, LockedTool};

/// The tools every lock lists, by name, in lock order.
pub const TOOLS: [&str; 2] = ["gcc", "qemu"];

/// The only platform a format 1 project targets, and its package.
pub const PIPOS: &str = "pipos";

/// The package of project skeletons. It must not carry `abi`, `tools` or
/// `firmware`.
pub const TEMPLATES: &str = "templates";

/// The formats of `formats` this tool knows, with the one value it reads.
/// Rule 4 of the fit rules uses this list.
pub const KNOWN_FORMATS: [(&str, i64); 7] = [
    ("boot", 1),
    ("flags", 1),
    ("project", 1),
    ("rimg", 1),
    ("runner", 1),
    ("scenario", 1),
    ("templates", 1),
];

/// Why reading the index, reading `sdk.json`, resolving or using the lock
/// failed. Each variant is one code of the spec's table; [`Error::code`]
/// gives it as the spec spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `index: read`. A local index file is missing or cannot be read.
    IndexRead(String),
    /// `index: syntax`. The bytes are not UTF-8, not JSON, or the top value
    /// is not an object.
    IndexSyntax(String),
    /// `index: format`. `format` is missing, not a positive integer, or
    /// larger than this tool reads.
    IndexFormat(String),
    /// `index: entry`. A field is missing, has the wrong type or breaks
    /// its form. Names the place.
    IndexEntry(String),
    /// `index: duplicate`. Two entries share a name and a version.
    IndexDuplicate {
        /// The name.
        name: String,
        /// The version.
        version: String,
    },
    /// `index: host`. A tool the resolution takes has no archive for the
    /// running host.
    IndexHost {
        /// The tool.
        tool: String,
        /// Its version.
        version: String,
        /// The running host.
        host: String,
    },
    /// `package: format`. `sdk.json` has a bad or larger `format`, or names
    /// a format this tool does not know.
    PackageFormat(String),
    /// `package: manifest`. `sdk.json` does not parse, a field breaks its
    /// table, or it differs from its index entry.
    PackageManifest(String),
    /// `sdk: no package`. Holds the detail of "Resolving requires": the
    /// name, the requirements on it and, when one applies, the newer tool
    /// suffix.
    NoPackage(String),
    /// `sdk: no tool`. The index has no tool with the name and version that
    /// the package names.
    NoTool {
        /// The tool.
        name: String,
        /// The version the package names.
        version: String,
    },
    /// `lock: syntax`. `robco.lock` is not UTF-8 or not TOML. Holds the line.
    LockSyntax(String),
    /// `lock: format`. `format` is missing, not a positive integer, or
    /// larger than this tool reads.
    LockFormat(String),
    /// `lock: unknown key`. Holds the dotted name.
    LockUnknownKey(String),
    /// `lock: entry`. A key is missing, has the wrong type or breaks its
    /// form, a name comes twice, a tool is not `gcc` or `qemu`, or
    /// `[tool.archives]` is empty. Holds the key, and for a bad value the
    /// value.
    LockEntry(String),
    /// `lock: stale`. Holds the detail, which names the first package or
    /// tool that fails.
    LockStale(String),
    /// `lock: hash`. An archive does not match its lock hash. Names it.
    LockHash(String),
    /// `lock: missing`. The index has no entry for a locked pin. Names it.
    LockMissing(String),
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
    /// `io: read`. The host refused to read a file that exists.
    IoRead {
        /// The file.
        path: String,
        /// What the host said.
        reason: String,
    },
    /// `io: write`. The host refused to write, create or rename a file.
    IoWrite {
        /// The file.
        path: String,
        /// What the host said.
        reason: String,
    },
}

impl Error {
    /// The error code, as the spec spells it, such as `lock: stale`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::IndexRead(_) => "index: read",
            Self::IndexSyntax(_) => "index: syntax",
            Self::IndexFormat(_) => "index: format",
            Self::IndexEntry(_) => "index: entry",
            Self::IndexDuplicate { .. } => "index: duplicate",
            Self::IndexHost { .. } => "index: host",
            Self::PackageFormat(_) => "package: format",
            Self::PackageManifest(_) => "package: manifest",
            Self::NoPackage(_) => "sdk: no package",
            Self::NoTool { .. } => "sdk: no tool",
            Self::LockSyntax(_) => "lock: syntax",
            Self::LockFormat(_) => "lock: format",
            Self::LockUnknownKey(_) => "lock: unknown key",
            Self::LockEntry(_) => "lock: entry",
            Self::LockStale(_) => "lock: stale",
            Self::LockHash(_) => "lock: hash",
            Self::LockMissing(_) => "lock: missing",
            Self::LockTool { .. } => "lock: tool",
            Self::IoRead { .. } => "io: read",
            Self::IoWrite { .. } => "io: write",
        }
    }
}

impl ErrorCode for Error {
    fn error_code(&self) -> &'static str {
        self.code()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::IndexRead(d)
            | Self::IndexSyntax(d)
            | Self::IndexFormat(d)
            | Self::IndexEntry(d)
            | Self::PackageFormat(d)
            | Self::PackageManifest(d)
            | Self::NoPackage(d)
            | Self::LockSyntax(d)
            | Self::LockFormat(d)
            | Self::LockUnknownKey(d)
            | Self::LockHash(d)
            | Self::LockMissing(d) => write!(f, "{code}: {d}"),
            Self::IndexDuplicate { name, version } => {
                write!(f, "{code}: {name} {version} is listed twice")
            }
            Self::IndexHost {
                tool,
                version,
                host,
            } => write!(f, "{code}: {tool} {version} has no archive for {host}"),
            Self::NoTool { name, version } => {
                write!(f, "{code}: the index lists no {name} {version}")
            }
            Self::LockEntry(d) | Self::LockStale(d) => write!(f, "{code}: {d}: run robco update"),
            Self::LockTool {
                name,
                locked,
                named,
            } => write!(
                f,
                "{code}: the lock pins {name} {locked} and the package names {name} {named}: run robco update"
            ),
            Self::IoRead { path, reason } | Self::IoWrite { path, reason } => {
                write!(f, "{code}: {path}: {reason}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Reads a decimal part, from 0 to 65535, with no leading zero: `0` is a
/// part and `03` is not.
fn parse_part(s: &str) -> Option<u16> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return None;
    }
    s.parse::<u16>().ok()
}

/// A package version, `X.Y.Z`, each part from 0 to 65535 with no leading
/// zero. A project's own `project.version` has other rules, in
/// [`crate::manifest`].
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
    /// Reads `X.Y.Z`. Returns `None` for anything else, `1.03.0` included.
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split('.');
        let v = Self {
            major: parse_part(it.next()?)?,
            minor: parse_part(it.next()?)?,
            patch: parse_part(it.next()?)?,
        };
        it.next().is_none().then_some(v)
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
    /// Reads `MAJOR.MINOR`, each part with no leading zero, the form of
    /// `abi` in `sdk.json`, the index and `requires`. Returns `None` for
    /// anything else.
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

/// A tool that a package names, at one exact version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPin {
    /// `gcc` or `qemu`.
    pub name: String,
    /// The version, compared as a string.
    pub version: String,
}

/// The form of a package name: 1 to 32 characters from `a` to `z`, `0` to
/// `9` and `-`, starting with a letter.
pub fn is_package_name(s: &str) -> bool {
    is_name(s)
}

/// The form of a host name, the same as a package name's.
pub fn is_host_name(s: &str) -> bool {
    is_name(s)
}

fn is_name(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// The form of a tool version: 1 to 64 bytes from `0x21` to `0x7E`, with
/// no `"` and no `\`.
pub fn is_tool_version(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| (0x21..=0x7E).contains(&b) && b != b'"' && b != b'\\')
}

/// The form of a SHA-256: 64 lower case hex digits. Upper case is refused.
pub fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The formats of `formats` that this tool does not know, by name or by
/// value, sorted by name.
pub fn unknown_formats(formats: &BTreeMap<String, i64>) -> Vec<(&str, i64)> {
    formats
        .iter()
        .filter(|(n, v)| !KNOWN_FORMATS.contains(&(n.as_str(), **v)))
        .map(|(n, v)| (n.as_str(), *v))
        .collect()
}

/// Rule 4: every format of the entry is one the tool knows.
fn rule4(p: &IndexPackage) -> bool {
    unknown_formats(&p.formats).is_empty()
}

/// Rules 1 to 3 of the fit rules, on an index entry alone. A package
/// without `abi` fits nothing.
fn fits_rules_1_to_3(target: &Target, p: &IndexPackage) -> bool {
    target.platform == PIPOS && p.name == PIPOS && p.abi.is_some_and(|abi| abi.serves(target.abi))
}

/// All four fit rules, for a project and an index entry. The resolver
/// decides on the index entry alone.
pub fn fits(target: &Target, p: &IndexPackage) -> bool {
    fits_rules_1_to_3(target, p) && rule4(p)
}

/// Rules 2, 3 and 4, for a `requires` entry at `abi` and an index entry.
pub fn fits_requirement(abi: Abi, p: &IndexPackage) -> bool {
    p.abi.is_some_and(|a| a.serves(abi)) && rule4(p)
}

/// What `robco sdk install` names on its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallPick {
    /// `PACKAGE`: the highest version.
    Newest,
    /// `PACKAGE@X.Y`: the highest version whose first two parts are these.
    Minor(u16, u16),
    /// `PACKAGE@X.Y.Z`: that version.
    Exact(Version),
}

/// The root requirement of a resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Root {
    /// A project: `pipos` at `target.abi`, by rules 1 to 4.
    Project(Target),
    /// `robco sdk install`: a package by the form of its argument, and
    /// rule 4.
    Install {
        /// The package name.
        name: String,
        /// Which versions the argument allows.
        pick: InstallPick,
        /// The argument as given, for the detail of `sdk: no package`.
        argument: String,
    },
}

impl Root {
    fn name(&self) -> &str {
        match self {
            Root::Project(_) => PIPOS,
            Root::Install { name, .. } => name,
        }
    }

    fn text(&self) -> String {
        match self {
            Root::Project(t) => format!("robco.toml abi {}", t.abi),
            Root::Install { argument, .. } => format!("command line {argument}"),
        }
    }

    /// Whether `p` fits the root, with rule 4 when `rule_4` is set.
    fn fits(&self, p: &IndexPackage, rule_4: bool) -> bool {
        let form = match self {
            Root::Project(t) => fits_rules_1_to_3(t, p),
            Root::Install { name, pick, .. } => {
                p.name == *name
                    && match pick {
                        InstallPick::Newest => true,
                        InstallPick::Minor(x, y) => (p.version.major, p.version.minor) == (*x, *y),
                        InstallPick::Exact(v) => p.version == *v,
                    }
            }
        };
        form && (!rule_4 || rule4(p))
    }
}

/// One requirement of the set: who states it, on which name, at which ABI.
/// `by` is `None` for the root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Need {
    by: Option<(String, Version)>,
    name: String,
    abi: Option<Abi>,
}

impl Need {
    fn fits(&self, root: &Root, p: &IndexPackage, rule_4: bool) -> bool {
        match (&self.by, self.abi) {
            (Some(_), Some(abi)) => {
                p.name == self.name && p.abi.is_some_and(|a| a.serves(abi)) && (!rule_4 || rule4(p))
            }
            _ => root.fits(p, rule_4),
        }
    }
}

/// The detail of `sdk: no package` for `name`, by "Resolving requires".
fn no_package(root: &Root, packages: &[IndexPackage], name: &str, on: &[&Need]) -> Error {
    let mut parts = Vec::new();
    if on.iter().any(|n| n.by.is_none()) {
        parts.push(root.text());
    }
    // `on` comes from a BTreeSet, so stated requirements are already sorted
    // by package name in byte order, then by version.
    for n in on {
        if let (Some((by, v)), Some(abi)) = (&n.by, n.abi) {
            parts.push(format!("{by} {v} abi {abi}"));
        }
    }
    let mut detail = format!("{name}: {}", parts.join(", "));
    let near = packages
        .iter()
        .filter(|p| p.name == name && !rule4(p) && on.iter().all(|n| n.fits(root, p, false)))
        .max_by_key(|p| p.version);
    if let Some(p) = near
        && let Some((format, value)) = unknown_formats(&p.formats).first()
    {
        detail.push_str(&format!(
            "; {} needs a newer tool: {format} {value}",
            p.version
        ));
    }
    Error::NoPackage(detail)
}

/// Resolves a root requirement and every `requires` it leads to, by the
/// passes of "Resolving requires", from the index entries alone.
///
/// Returns one entry per name, sorted by name in byte order. It never goes
/// back to an older version of a name picked in an earlier pass. A
/// `PACKAGE@X.Y.Z` root whose entry uses a format this tool does not know
/// fails with `package: format`.
pub fn resolve_requires<'a>(
    packages: &'a [IndexPackage],
    root: &Root,
) -> Result<Vec<&'a IndexPackage>, Error> {
    if let Root::Install {
        pick: InstallPick::Exact(v),
        name,
        ..
    } = root
        && let Some(p) = packages.iter().find(|p| p.name == *name && p.version == *v)
        && let Some((format, value)) = unknown_formats(&p.formats).first()
    {
        return Err(Error::PackageFormat(format!(
            "{name} {v} uses {format} {value}, which this tool does not know: install a newer tool"
        )));
    }

    let mut set: BTreeSet<Need> = BTreeSet::new();
    set.insert(Need {
        by: None,
        name: root.name().to_owned(),
        abi: None,
    });
    let mut picks: BTreeMap<&str, &IndexPackage> = BTreeMap::new();
    loop {
        // Step 2: every name at once, from the whole set. Names come in
        // byte order, so the first that fails is the first in byte order.
        picks.clear();
        let names: BTreeSet<&str> = set.iter().map(|n| n.name.as_str()).collect();
        for name in names {
            let on: Vec<&Need> = set.iter().filter(|n| n.name == name).collect();
            let best = packages
                .iter()
                .filter(|p| p.name == name && on.iter().all(|n| n.fits(root, p, true)))
                .max_by_key(|p| p.version);
            match best {
                Some(p) => {
                    picks.insert(&p.name, p);
                }
                None => return Err(no_package(root, packages, name, &on)),
            }
        }
        // Step 3: the requires of each pick.
        let mut added = false;
        let mut new = Vec::new();
        for p in picks.values() {
            for r in &p.requires {
                new.push(Need {
                    by: Some((p.name.clone(), p.version)),
                    name: r.name.clone(),
                    abi: Some(r.abi),
                });
            }
        }
        // SDK_PACKAGE.md, "Resolving requires": a requirement stays in the
        // set once added, even when a later pass passes over the version
        // that stated it. Nothing is ever removed.
        for n in new {
            added |= set.insert(n);
        }
        if !added {
            break;
        }
    }

    // The root and what its picks reach by `requires`.
    let mut reached: BTreeSet<&str> = BTreeSet::new();
    let mut todo = vec![root.name()];
    while let Some(name) = todo.pop() {
        if reached.insert(name)
            && let Some(p) = picks.get(name)
        {
            todo.extend(p.requires.iter().map(|r| r.name.as_str()));
        }
    }
    Ok(picks
        .into_iter()
        .filter(|(n, _)| reached.contains(n))
        .map(|(_, p)| p)
        .collect())
}

/// The index entry of each tool a `pipos` package names, `gcc` then `qemu`,
/// each with an archive for `host`.
pub fn pick_tools<'a>(
    index: &'a Index,
    package: &IndexPackage,
    host: &str,
) -> Result<Vec<&'a IndexTool>, Error> {
    let mut out = Vec::new();
    for pin in &package.tools {
        let tool = index
            .tools
            .iter()
            .find(|t| t.name == pin.name && t.version == pin.version)
            .ok_or_else(|| Error::NoTool {
                name: pin.name.clone(),
                version: pin.version.clone(),
            })?;
        if !tool.archives.contains_key(host) {
            return Err(Error::IndexHost {
                tool: tool.name.clone(),
                version: tool.version.clone(),
                host: host.to_owned(),
            });
        }
        out.push(tool);
    }
    Ok(out)
}

/// Resolves a project against an index, as if no lock existed, and returns
/// the lock to write.
///
/// `location` is the index as the caller was given it, kept for the record.
/// `host` is the running host, such as `macos-arm64`: both tools must have
/// an archive for it. The lock lists the archives of every host.
pub fn resolve(target: &Target, index: &Index, location: &str, host: &str) -> Result<Lock, Error> {
    let picks = resolve_requires(&index.packages, &Root::Project(target.clone()))?;
    let mut tools = Vec::new();
    if let Some(pipos) = picks.iter().find(|p| p.name == PIPOS) {
        for tool in pick_tools(index, pipos, host)? {
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
    }
    Ok(Lock {
        index: location.to_owned(),
        packages: picks
            .iter()
            .map(|p| LockedPackage {
                name: p.name.clone(),
                version: p.version,
                sha256: p.sha256.clone(),
            })
            .collect(),
        tools,
    })
}

/// The closure check of "A closed index", for the party that writes an
/// index. A reader never runs it.
///
/// Returns the first broken rule, in the order of `packages`: a `requires`
/// entry that no package of the index meets by rules 2 and 3, or a tool a
/// package names that no `tools` entry with at least one archive holds.
/// Rule 4 plays no part.
pub fn check_closed(packages: &[IndexPackage], tools: &[IndexTool]) -> Result<(), String> {
    for p in packages {
        for r in &p.requires {
            let met = packages
                .iter()
                .any(|q| q.name == r.name && q.abi.is_some_and(|a| a.serves(r.abi)));
            if !met {
                return Err(format!(
                    "{} {} requires {} abi {}, and no package of the index meets it",
                    p.name, p.version, r.name, r.abi
                ));
            }
        }
        for pin in &p.tools {
            let held = tools
                .iter()
                .any(|t| t.name == pin.name && t.version == pin.version && !t.archives.is_empty());
            if !held {
                return Err(format!(
                    "{} {} names {} {}, and the index holds no archive of it",
                    p.name, p.version, pin.name, pin.version
                ));
            }
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests;
