// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads, checks and writes `robco.lock`.
//!
//! Implements RobCo-Core `docs/spec/STUDIO_PROJECT.md`, "robco.lock" and
//! its sections "Keys", "Reading the lock", "How the tool writes the lock",
//! "What the lock pins" and "A stale lock", as read at RobCo-Core commit
//! `e004c73`.
//!
//! The writer follows "How the tool writes the lock" exactly, so the same
//! resolution gives the same bytes on every system. It writes a temporary
//! file in the project root and renames it over the old lock, so a failure
//! never leaves half a lock.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use toml::{Table, Value};

use super::index::{Archive, Index, IndexPackage};
use super::sdk_json::SdkJson;
use super::{
    Abi, Error, PIPOS, Requirement, TOOLS, Target, Version, is_host_name, is_package_name,
    is_sha256, is_tool_version,
};

/// What the installed `sdk.json` of a locked package says that the lock
/// does not: its declared `abi` and its `requires`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    /// The `abi` of the installed `sdk.json`, absent when it has none.
    pub abi: Option<Abi>,
    /// The `requires` of the installed `sdk.json`.
    pub requires: Vec<Requirement>,
}

/// Gives what the installed `sdk.json` of a locked package says.
pub type InstalledOf<'a> = dyn Fn(&LockedPackage) -> Installed + 'a;

/// The ABI of a locked `pipos` package, the first two parts of its version.
/// STUDIO_PROJECT.md, "A stale lock", rule 1, allows this for `pipos` only:
/// "The lock has no `abi` key, so the ABI of a locked `pipos` package is the
/// first two parts of its `version`". SDK_PACKAGE.md: "A tool never works out
/// an ABI from a version". Returns `None` for any other package.
fn locked_pipos_abi(p: &LockedPackage) -> Option<Abi> {
    (p.name == PIPOS).then_some(Abi {
        major: p.version.major,
        minor: p.version.minor,
    })
}

/// The lock `format` this module reads and writes. A larger one fails.
pub const FORMAT: i64 = 1;

/// The lock's file name in the project root.
pub const FILE: &str = "robco.lock";

/// The start of each temporary file the writer renames over [`FILE`]. The
/// process id and a counter follow, so no two writers share a name.
pub const TEMP_PREFIX: &str = "robco.lock.tmp.";

/// How many names the writer tries when one already exists.
const TEMP_TRIES: u32 = 16;

/// A parsed or resolved `robco.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    /// The index as the tool was given it. For the record only.
    pub index: String,
    /// One entry per package.
    pub packages: Vec<LockedPackage>,
    /// One entry per tool.
    pub tools: Vec<LockedTool>,
}

/// A `[[package]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPackage {
    /// The package name.
    pub name: String,
    /// The exact version.
    pub version: Version,
    /// The archive's SHA-256.
    pub sha256: String,
}

/// A `[[tool]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedTool {
    /// `gcc` or `qemu`.
    pub name: String,
    /// The version the locked `pipos` package names.
    pub version: String,
    /// Host name to the SHA-256 of that host's archive.
    pub archives: BTreeMap<String, String>,
}

fn entry(d: String) -> Error {
    Error::LockEntry(d)
}

/// The 1-based line of a byte offset.
fn line_of(bytes: &[u8], offset: usize) -> usize {
    let end = offset.min(bytes.len());
    bytes[..end].iter().filter(|&&b| b == b'\n').count() + 1
}

fn only_keys(t: &Table, at: &str, keys: &[&str]) -> Result<(), Error> {
    match t.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(k) if at.is_empty() => Err(Error::LockUnknownKey(k.clone())),
        Some(k) => Err(Error::LockUnknownKey(format!("{at}.{k}"))),
        None => Ok(()),
    }
}

/// The tables of an array of tables, or `None` when the value is another
/// type.
fn tables(v: &Value) -> Option<Vec<&Table>> {
    v.as_array()?.iter().map(Value::as_table).collect()
}

/// A string key, checked against its form. The detail names the key, and
/// for a bad value the value.
fn formed<'a>(
    t: &'a Table,
    at: &str,
    key: &str,
    ok: &dyn Fn(&str) -> bool,
) -> Result<&'a str, Error> {
    match t.get(key) {
        None => Err(entry(format!("{at}.{key} is missing"))),
        Some(Value::String(s)) if ok(s) => Ok(s),
        Some(Value::String(s)) => Err(entry(format!("{at}.{key} = {}", quote(s)))),
        Some(_) => Err(entry(format!("{at}.{key} is not a string"))),
    }
}

impl Lock {
    /// Reads the bytes of a `robco.lock`, in the order of "Reading the
    /// lock": syntax, `format`, unknown keys, then `index`, each
    /// `[[package]]` and each `[[tool]]`, key by key. An absent
    /// `[[package]]` or `[[tool]]` reads as empty, and [`Lock::check_stale`]
    /// then names what is missing.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let text = std::str::from_utf8(bytes).map_err(|e| {
            Error::LockSyntax(format!(
                "line {}: not UTF-8",
                line_of(bytes, e.valid_up_to())
            ))
        })?;
        let root: Table = text.parse().map_err(|e: toml::de::Error| {
            Error::LockSyntax(format!(
                "line {}: {}",
                e.span().map_or(1, |s| line_of(bytes, s.start)),
                e.message().trim()
            ))
        })?;
        match root.get("format") {
            None => return Err(Error::LockFormat("format is missing".to_owned())),
            Some(Value::Integer(n)) if *n > FORMAT => {
                return Err(Error::LockFormat(format!(
                    "the lock has format {n} and this tool reads {FORMAT}: install a newer tool"
                )));
            }
            Some(Value::Integer(n)) if *n >= 1 => {}
            Some(_) => {
                return Err(Error::LockFormat(
                    "format is not a positive integer".to_owned(),
                ));
            }
        }

        // Unknown keys, at the top, in an entry or under one. The keys of
        // `[tool.archives]` are host names, which "Keys" checks as values.
        only_keys(&root, "", &["format", "index", "package", "tool"])?;
        for p in root.get("package").and_then(tables).unwrap_or_default() {
            only_keys(p, "package", &["name", "version", "sha256"])?;
        }
        for t in root.get("tool").and_then(tables).unwrap_or_default() {
            only_keys(t, "tool", &["name", "version", "archives"])?;
        }

        let index = match root.get("index") {
            None => return Err(entry("index is missing".to_owned())),
            Some(Value::String(s)) => s.clone(),
            Some(_) => return Err(entry("index is not a string".to_owned())),
        };

        let list = |key: &str| -> Result<Vec<&Table>, Error> {
            match root.get(key) {
                None => Ok(Vec::new()),
                Some(v) => {
                    tables(v).ok_or_else(|| entry(format!("{key} is not an array of tables")))
                }
            }
        };

        let mut packages = Vec::new();
        let mut names = BTreeSet::new();
        for p in list("package")? {
            let name = formed(p, "package", "name", &is_package_name)?.to_owned();
            if !names.insert(name.clone()) {
                return Err(entry(format!(
                    "package.name = {} is listed twice",
                    quote(&name)
                )));
            }
            let version = Version::parse(formed(p, "package", "version", &|s| {
                Version::parse(s).is_some()
            })?)
            .ok_or_else(|| entry("package.version".to_owned()))?;
            let sha256 = formed(p, "package", "sha256", &is_sha256)?.to_owned();
            packages.push(LockedPackage {
                name,
                version,
                sha256,
            });
        }

        let mut tools = Vec::new();
        let mut names = BTreeSet::new();
        for t in list("tool")? {
            let name = formed(t, "tool", "name", &|s| TOOLS.contains(&s))?.to_owned();
            if !names.insert(name.clone()) {
                return Err(entry(format!(
                    "tool.name = {} is listed twice",
                    quote(&name)
                )));
            }
            let version = formed(t, "tool", "version", &is_tool_version)?.to_owned();
            let table = match t.get("archives") {
                None => return Err(entry("tool.archives is missing".to_owned())),
                Some(Value::Table(a)) => a,
                Some(_) => return Err(entry("tool.archives is not a table".to_owned())),
            };
            if table.is_empty() {
                return Err(entry("tool.archives is empty".to_owned()));
            }
            let mut archives = BTreeMap::new();
            for host in table.keys() {
                if !is_host_name(host) {
                    return Err(entry(format!("tool.archives.{}", quote(host))));
                }
                let hash = formed(table, "tool.archives", host, &is_sha256)?;
                archives.insert(host.clone(), hash.to_owned());
            }
            tools.push(LockedTool {
                name,
                version,
                archives,
            });
        }

        Ok(Self {
            index,
            packages,
            tools,
        })
    }

    /// The lock's bytes, by "How the tool writes the lock": `format`, then
    /// `index`, then the packages sorted by name, then the tools sorted by
    /// name, each with its hosts sorted by name; one blank line before each
    /// header, line feeds only, no comment.
    pub fn to_text(&self) -> String {
        let mut packages: Vec<&LockedPackage> = self.packages.iter().collect();
        packages.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        let mut tools: Vec<&LockedTool> = self.tools.iter().collect();
        tools.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));

        let mut out = String::new();
        // Writing to a String cannot fail, so the results are dropped.
        let _ = writeln!(out, "format = {FORMAT}");
        let _ = writeln!(out, "index = {}", quote(&self.index));
        for p in packages {
            let _ = writeln!(out, "\n[[package]]");
            let _ = writeln!(out, "name = {}", quote(&p.name));
            let _ = writeln!(out, "version = {}", quote(&p.version.to_string()));
            let _ = writeln!(out, "sha256 = {}", quote(&p.sha256));
        }
        for t in tools {
            let _ = writeln!(out, "\n[[tool]]");
            let _ = writeln!(out, "name = {}", quote(&t.name));
            let _ = writeln!(out, "version = {}", quote(&t.version));
            let _ = writeln!(out, "\n[tool.archives]");
            // A BTreeMap of Strings iterates in byte order. A host name has
            // the form of a bare key.
            for (host, hash) in &t.archives {
                let _ = writeln!(out, "{host} = {}", quote(hash));
            }
        }
        out
    }

    /// Checks the lock against the project by "A stale lock".
    ///
    /// Before the listed packages are installed, `installed_of` is `None`,
    /// and only the parts that need no installed `sdk.json` run: the
    /// manifest's requirement on `pipos`, a missing `pipos`, and the tools.
    /// Once they are installed, `installed_of` gives each listed package's
    /// declared `abi` and `requires`, and every rule runs. The ABI of a
    /// locked `pipos` comes from its version; the ABI of any other package
    /// is its declared `abi`, and a package without one fits no `requires`
    /// entry. The first failure, in the order of the spec's list and then by
    /// name, gives `lock: stale`.
    pub fn check_stale(
        &self,
        target: &Target,
        host: &str,
        installed_of: Option<&InstalledOf<'_>>,
    ) -> Result<(), Error> {
        let stale = |d: String| Err(Error::LockStale(d));
        let mut packages: Vec<&LockedPackage> = self.packages.iter().collect();
        packages.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        // What each listed package declares, once installed.
        let installed: Vec<(&LockedPackage, Installed)> = match installed_of {
            Some(f) => packages.iter().map(|p| (*p, f(p))).collect(),
            None => Vec::new(),
        };
        // Every `requires` entry of every listed package, with who states it.
        let stated: Vec<(&LockedPackage, &Requirement)> = installed
            .iter()
            .flat_map(|(p, i)| i.requires.iter().map(move |r| (*p, r)))
            .collect();

        // 1. A listed package no longer fits a requirement on it.
        for p in &packages {
            let pipos_abi = locked_pipos_abi(p);
            if let Some(abi) = pipos_abi
                && (target.platform != PIPOS || !abi.serves(target.abi))
            {
                return stale(format!(
                    "package {} {} does not fit robco.toml platform {} abi {}",
                    p.name, p.version, target.platform, target.abi
                ));
            }
            // The ABI a `requires` entry is judged against: `pipos` from its
            // version, any other package from its installed `sdk.json`.
            let abi = pipos_abi.or_else(|| {
                installed
                    .iter()
                    .find(|(q, _)| q.name == p.name)
                    .and_then(|(_, i)| i.abi)
            });
            // STUDIO_PROJECT.md, "A stale lock", rule 1: "a `requires` entry
            // of another listed package". A package's entry on its own name
            // does not count.
            for (by, r) in &stated {
                if r.name == p.name && by.name != p.name && !abi.is_some_and(|a| a.serves(r.abi)) {
                    return stale(format!(
                        "package {} {} does not fit {} {} abi {}",
                        p.name, p.version, by.name, by.version, r.abi
                    ));
                }
            }
        }
        // 2. A needed package is not listed.
        let mut needed: BTreeSet<&str> = stated.iter().map(|(_, r)| r.name.as_str()).collect();
        if target.platform == PIPOS {
            needed.insert(PIPOS);
        }
        for name in &needed {
            if !packages.iter().any(|p| p.name == *name) {
                return stale(format!("package {name} is needed and not listed"));
            }
        }
        // 3. A listed package the manifest does not need: one that no chain
        //    of `requires` from `pipos` reaches. It needs the `requires`.
        if installed_of.is_some() {
            let mut reached: BTreeSet<&str> = BTreeSet::new();
            let mut todo: Vec<&str> = Vec::new();
            if target.platform == PIPOS {
                todo.push(PIPOS);
            }
            while let Some(name) = todo.pop() {
                if reached.insert(name) {
                    todo.extend(
                        stated
                            .iter()
                            .filter(|(by, _)| by.name == name)
                            .map(|(_, r)| r.name.as_str()),
                    );
                }
            }
            for p in &packages {
                if !reached.contains(p.name.as_str()) {
                    return stale(format!(
                        "package {} {} is listed and not needed",
                        p.name, p.version
                    ));
                }
            }
        }
        // 4. A tool is missing, or has no archive for the running host.
        for name in TOOLS {
            match self.tools.iter().find(|t| t.name == name) {
                None => return stale(format!("tool {name} is not listed")),
                Some(t) if !t.archives.contains_key(host) => {
                    return stale(format!(
                        "tool {} {} has no archive for {host}",
                        t.name, t.version
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// Checks, once `pipos` is installed, that each `[[tool]]` has the
    /// version its `sdk.json` names. A difference gives `lock: tool`.
    pub fn check_tools(&self, sdk: &SdkJson) -> Result<(), Error> {
        for pin in &sdk.tools {
            if let Some(t) = self.tools.iter().find(|t| t.name == pin.name)
                && t.version != pin.version
            {
                return Err(Error::LockTool {
                    name: t.name.clone(),
                    locked: t.version.clone(),
                    named: pin.version.clone(),
                });
            }
        }
        Ok(())
    }
}

impl LockedPackage {
    /// Compares the hash of an archive with the lock's, as strings, byte
    /// for byte. A mismatch gives `lock: hash` and names the package.
    pub fn check_hash(&self, actual: &str) -> Result<(), Error> {
        if actual == self.sha256 {
            Ok(())
        } else {
            Err(Error::LockHash(format!(
                "package {} {} does not match its lock hash",
                self.name, self.version
            )))
        }
    }

    /// Finds the index entry with this name, version and hash, to download
    /// it. None gives `lock: missing`.
    pub fn find_in<'a>(&self, index: &'a Index) -> Result<&'a IndexPackage, Error> {
        index
            .packages
            .iter()
            .find(|p| p.name == self.name && p.version == self.version && p.sha256 == self.sha256)
            .ok_or_else(|| {
                Error::LockMissing(format!(
                    "the index lists no package {} {} with the locked hash",
                    self.name, self.version
                ))
            })
    }
}

impl LockedTool {
    /// Finds the index archive of this tool for a host, with the locked
    /// version and hash, to download it. None gives `lock: missing`.
    pub fn find_in<'a>(&self, index: &'a Index, host: &str) -> Result<&'a Archive, Error> {
        let hash = self.archives.get(host);
        index
            .tools
            .iter()
            .filter(|t| t.name == self.name && t.version == self.version)
            .filter_map(|t| t.archives.get(host))
            .find(|a| Some(&a.sha256) == hash)
            .ok_or_else(|| {
                Error::LockMissing(format!(
                    "the index lists no {host} archive of tool {} {} with the locked hash",
                    self.name, self.version
                ))
            })
    }
}

/// A TOML basic string by "How the tool writes the lock": `\` and `"`
/// escaped, each character below U+0020 and U+007F as `\u00` and two lower
/// case hex digits, every other character as itself.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 || u32::from(c) == 0x7F => {
                let _ = write!(out, "\\u00{:02x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Reads `robco.lock` from a project folder. `None` when there is none. A
/// file the host refuses to open or read gives `io: read`.
pub fn read(project: &Path) -> Result<Option<Lock>, Error> {
    let path = project.join(FILE);
    match fs::read(&path) {
        Ok(bytes) => Lock::parse(&bytes).map(Some),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::IoRead {
            path: path.display().to_string(),
            reason: e.kind().to_string(),
        }),
    }
}

/// Writes `robco.lock` into a project folder: step 10 of "The steps of
/// update, build, run and test". Call it only for a [`Resolution::New`], and
/// only once steps 7 to 9 passed: every package and tool the command installs
/// is installed and checked. A command that fails before this step writes no
/// lock, so a first build keeps no lock and `robco update` keeps the old one.
///
/// It writes a new temporary file in the
/// same folder, renamed over the old lock. The temporary file gets a name no
/// one else has and is created new, so an existing file or link of that name
/// is never opened. A failure removes the temporary file, leaves the old
/// lock as it was and gives `io: write`.
pub fn write(project: &Path, lock: &Lock) -> Result<(), Error> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let path = project.join(FILE);
    let fail = |p: &Path, e: &std::io::Error| Error::IoWrite {
        path: p.display().to_string(),
        reason: e.kind().to_string(),
    };
    let mut tries = 0;
    let (tmp, mut file) = loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = project.join(format!("{TEMP_PREFIX}{}.{n}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == ErrorKind::AlreadyExists && tries + 1 < TEMP_TRIES => {
                tries += 1;
            }
            Err(e) => return Err(fail(&tmp, &e)),
        }
    };
    let written = file
        .write_all(lock.to_text().as_bytes())
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);
    let done = written
        .map_err(|e| fail(&tmp, &e))
        .and_then(|()| fs::rename(&tmp, &path).map_err(|e| fail(&path, &e)));
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}

/// Where the lock a command uses came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The project's `robco.lock`, read at step 5. The command never writes it.
    Locked(Lock),
    /// A new resolution from the index, step 6. The command writes it with
    /// [`write`] at step 10, after its installs passed.
    New(Lock),
}

impl Resolution {
    /// The lock, wherever it came from.
    pub fn lock(&self) -> &Lock {
        match self {
            Self::Locked(l) | Self::New(l) => l,
        }
    }
}

/// Steps 5 and 6 of a build, run or test. Writes nothing.
///
/// No lock: calls `resolve` and returns [`Resolution::New`]. A lock: checks
/// the parts of "A stale lock" that need no `requires`, and returns it as
/// [`Resolution::Locked`]. The caller runs [`Lock::check_stale`] again with
/// the `requires` once the packages are installed.
pub fn use_or_resolve(
    project: &Path,
    target: &Target,
    host: &str,
    resolve: &dyn Fn() -> Result<Lock, Error>,
) -> Result<Resolution, Error> {
    match read(project)? {
        Some(lock) => {
            lock.check_stale(target, host, None)?;
            Ok(Resolution::Locked(lock))
        }
        None => resolve().map(Resolution::New),
    }
}

/// Step 6 of `robco update`: resolves again as if no lock existed. It does
/// not read the old lock and writes nothing; the caller writes the new lock
/// with [`write`] at step 10.
pub fn resolve_again(resolve: &dyn Fn() -> Result<Lock, Error>) -> Result<Resolution, Error> {
    resolve().map(Resolution::New)
}
