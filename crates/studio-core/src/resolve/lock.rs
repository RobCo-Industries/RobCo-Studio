// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads, checks and writes `robco.lock`.
//!
//! Implements RobCo-Core `docs/spec/STUDIO_PROJECT.md`, "robco.lock", as
//! read at RobCo-Core commit `a7acab5`, with "What the lock pins" and
//! `lock: tool` as read in review at RobCo-Core commit `dc294ad`.
//!
//! The writer gives the same bytes for the same resolution on every system:
//! entries sorted by name, hosts sorted by name, `\n` line ends, no time and
//! no path of the machine. It writes a temporary file in the project root
//! and renames it over the old lock, so a failure never leaves half a lock.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use toml::{Table, Value};

use super::index::{Archive, Index, IndexPackage};
use super::sdk_json::SdkJson;
use super::{Error, PIPOS, Requirement, TOOLS, Target, Version, sha256};

/// The lock `format` this module reads and writes. A larger one fails.
pub const FORMAT: i64 = 1;

/// The lock's file name in the project root.
pub const FILE: &str = "robco.lock";

/// The temporary file the writer renames over [`FILE`].
pub const TEMP_FILE: &str = "robco.lock.tmp";

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
    /// The archive's SHA-256, lower case.
    pub sha256: String,
}

/// A `[[tool]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedTool {
    /// `gcc` or `qemu`.
    pub name: String,
    /// The version the locked `pipos` package names.
    pub version: String,
    /// Host name to the SHA-256 of that host's archive, lower case.
    pub archives: BTreeMap<String, String>,
}

fn syntax(what: String) -> Error {
    Error::LockSyntax(what)
}

fn only_keys(t: &Table, at: &str, keys: &[&str]) -> Result<(), Error> {
    match t.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(k) if at.is_empty() => Err(Error::LockUnknownKey(k.clone())),
        Some(k) => Err(Error::LockUnknownKey(format!("{at}.{k}"))),
        None => Ok(()),
    }
}

fn string<'a>(t: &'a Table, at: &str, key: &str) -> Result<&'a str, Error> {
    match t.get(key) {
        None => Err(syntax(format!("`{at}{key}` is missing"))),
        Some(v) => v
            .as_str()
            .ok_or_else(|| syntax(format!("`{at}{key}` is not a string"))),
    }
}

fn tables<'a>(t: &'a Table, key: &str) -> Result<Vec<&'a Table>, Error> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_table()
                    .ok_or_else(|| syntax(format!("`{key}` is not an array of tables")))
            })
            .collect(),
        Some(_) => Err(syntax(format!("`{key}` is not an array of tables"))),
    }
}

impl Lock {
    /// Reads the text of a `robco.lock`.
    ///
    /// The checks run in this order: TOML syntax, then `format`, then
    /// unknown keys, then types. A missing `[[package]]` or `[[tool]]` list
    /// reads as empty, and [`Lock::check_stale`] then names what is missing.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let root: Table = text
            .parse()
            .map_err(|e: toml::de::Error| syntax(e.to_string().trim_end().to_owned()))?;
        match root.get("format") {
            None => return Err(Error::LockFormat("`format` is missing".to_owned())),
            Some(Value::Integer(n)) if *n > FORMAT => {
                return Err(Error::LockFormat(format!(
                    "the lock has format {n} and this tool reads {FORMAT}: update the tool"
                )));
            }
            Some(Value::Integer(n)) if *n >= 1 => {}
            Some(_) => {
                return Err(Error::LockFormat(
                    "`format` is not a positive integer".to_owned(),
                ));
            }
        }
        only_keys(&root, "", &["format", "index", "package", "tool"])?;
        let index = string(&root, "", "index")?.to_owned();

        let mut packages = Vec::new();
        let mut names = BTreeSet::new();
        for p in tables(&root, "package")? {
            only_keys(p, "package", &["name", "version", "sha256"])?;
            let name = string(p, "package.", "name")?.to_owned();
            let version = Version::parse(string(p, "package.", "version")?)
                .ok_or_else(|| syntax(format!("package `{name}`: `version` is not X.Y.Z")))?;
            let hash = sha256(string(p, "package.", "sha256")?).ok_or_else(|| {
                syntax(format!("package `{name}`: `sha256` is not 64 hex digits"))
            })?;
            if !names.insert(name.clone()) {
                return Err(syntax(format!("package `{name}` is listed twice")));
            }
            packages.push(LockedPackage {
                name,
                version,
                sha256: hash,
            });
        }

        let mut tools = Vec::new();
        let mut names = BTreeSet::new();
        for t in tables(&root, "tool")? {
            only_keys(t, "tool", &["name", "version", "archives"])?;
            let name = string(t, "tool.", "name")?.to_owned();
            let version = string(t, "tool.", "version")?.to_owned();
            let archives_table = match t.get("archives") {
                None => return Err(syntax(format!("tool `{name}`: `archives` is missing"))),
                Some(v) => v
                    .as_table()
                    .ok_or_else(|| syntax(format!("tool `{name}`: `archives` is not a table")))?,
            };
            let mut archives = BTreeMap::new();
            for (host, v) in archives_table {
                let hash = v.as_str().and_then(sha256).ok_or_else(|| {
                    syntax(format!(
                        "tool `{name}`: `archives.{host}` is not 64 hex digits"
                    ))
                })?;
                archives.insert(host.clone(), hash);
            }
            if !names.insert(name.clone()) {
                return Err(syntax(format!("tool `{name}` is listed twice")));
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

    /// Writes the lock's text: the keys in the spec's order, entries and
    /// hosts sorted by name in byte order, `\n` line ends.
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
            // A BTreeMap of Strings iterates in byte order.
            for (host, hash) in &t.archives {
                let _ = writeln!(out, "{} = {}", key(host), quote(hash));
            }
        }
        out
    }

    /// Checks the lock against the project, before anything is installed.
    ///
    /// `requires_of` gives the `requires` of a locked package, from its
    /// installed `sdk.json` or its index entry: the lock does not hold them.
    /// `host` is the running host. The first failure, by the order of the
    /// spec's list and then by name, gives `lock: stale`.
    pub fn check_stale(
        &self,
        target: &Target,
        host: &str,
        requires_of: &dyn Fn(&LockedPackage) -> Vec<Requirement>,
    ) -> Result<(), Error> {
        let stale = |d: String| Err(Error::LockStale(d));
        let mut packages: Vec<&LockedPackage> = self.packages.iter().collect();
        packages.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));

        // What the project needs, by name: what the project's root package
        // reaches by `requires`, and nothing else. The root is pipos at the
        // target ABI. Only a listed package's `requires` can be followed: a
        // requirement on a package the lock does not hold stays in `needs`,
        // and rule 2 names it. `seen` stops a cycle.
        let mut needs: BTreeMap<String, Vec<Requirement>> = BTreeMap::new();
        let mut todo: Vec<Requirement> = Vec::new();
        if target.platform == PIPOS {
            todo.push(Requirement {
                name: PIPOS.to_owned(),
                abi: target.abi,
            });
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        while let Some(r) = todo.pop() {
            let name = r.name.clone();
            needs.entry(name.clone()).or_default().push(r);
            if !seen.insert(name.clone()) {
                continue;
            }
            if let Some(p) = packages.iter().find(|p| p.name == name) {
                todo.extend(requires_of(p));
            }
        }

        // 1. A listed package no longer fits. A package's ABI is the first
        //    two parts of its version.
        for p in &packages {
            if p.name == PIPOS && target.platform != PIPOS {
                return stale(format!(
                    "package {} {} does not fit platform {}",
                    p.name, p.version, target.platform
                ));
            }
            if let Some(reqs) = needs.get(&p.name)
                && let Some(r) = reqs.iter().find(|r| !p.version.abi().serves(r.abi))
            {
                return stale(format!(
                    "package {} {} does not fit ABI {}",
                    p.name, p.version, r.abi
                ));
            }
        }
        // 2. A needed package is not listed.
        for name in needs.keys() {
            if !packages.iter().any(|p| &p.name == name) {
                return stale(format!("package {name} is needed and not listed"));
            }
        }
        // 3. A listed package is not needed.
        for p in &packages {
            if !needs.contains_key(&p.name) {
                return stale(format!("package {} is listed and not needed", p.name));
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
    /// Compares the hash of an archive with the lock's. A mismatch gives
    /// `lock: hash` and names the package. Hashes compare in lower case.
    pub fn check_hash(&self, actual: &str) -> Result<(), Error> {
        if actual.to_ascii_lowercase() == self.sha256 {
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

/// A TOML basic string.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A TOML key: bare when it can be, quoted otherwise.
fn key(s: &str) -> String {
    let bare = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if bare { s.to_owned() } else { quote(s) }
}

fn io(what: &str, e: &std::io::Error) -> Error {
    Error::LockIo(format!("{what}: {}", e.kind()))
}

/// Reads `robco.lock` from a project folder. `None` when there is none.
pub fn read(project: &Path) -> Result<Option<Lock>, Error> {
    match fs::read(project.join(FILE)) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes)
                .map_err(|_| syntax("robco.lock is not UTF-8".to_owned()))?;
            Lock::parse(&text).map(Some)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io("cannot read robco.lock", &e)),
    }
}

/// Writes `robco.lock` into a project folder: a temporary file in the same
/// folder, renamed over the old lock.
pub fn write(project: &Path, lock: &Lock) -> Result<(), Error> {
    let tmp = project.join(TEMP_FILE);
    fs::write(&tmp, lock.to_text()).map_err(|e| io("cannot write robco.lock", &e))?;
    fs::rename(&tmp, project.join(FILE)).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io("cannot replace robco.lock", &e)
    })
}

/// What a build does with the lock, before it installs anything.
///
/// No lock: calls `resolve`, writes its lock and returns it. A lock: checks
/// it for staleness and returns it unchanged. Every failure leaves the
/// folder as it was.
pub fn use_or_resolve(
    project: &Path,
    target: &Target,
    host: &str,
    requires_of: &dyn Fn(&LockedPackage) -> Vec<Requirement>,
    resolve: &dyn Fn() -> Result<Lock, Error>,
) -> Result<Lock, Error> {
    match read(project)? {
        Some(lock) => {
            lock.check_stale(target, host, requires_of)?;
            Ok(lock)
        }
        None => {
            let lock = resolve()?;
            write(project, &lock)?;
            Ok(lock)
        }
    }
}

/// The lock part of `robco update`: resolves again as if no lock existed
/// and writes the new lock in place of the old. A failure leaves the old
/// lock as it was.
pub fn update(project: &Path, resolve: &dyn Fn() -> Result<Lock, Error>) -> Result<Lock, Error> {
    let lock = resolve()?;
    write(project, &lock)?;
    Ok(lock)
}
