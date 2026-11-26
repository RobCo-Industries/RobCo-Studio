// SPDX-License-Identifier: GPL-3.0-or-later

//! `robco sdk list`, by "robco sdk list" of RobCo-Core
//! `docs/spec/STUDIO_CLI.md`.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use studio_core::install::{self, Cached};
use studio_core::resolve::index::Index;
use studio_core::resolve::{TOOLS, Version, is_host_name, is_package_name, is_tool_version};

use crate::Failure;
use crate::lines::{Out, Val};

/// One package version or one tool archive. The order of the variants and
/// fields is the order of the list: packages first, then by name, then by
/// version, as numbers for a package and as bytes for a tool, then by host.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// A package: name and version.
    Package(String, Version),
    /// A tool archive: name, version and host.
    Tool(String, String, String),
}

/// Whether the cache holds an item and whether the index lists it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Seen {
    /// The cache holds it.
    pub installed: bool,
    /// The index lists it.
    pub offered: bool,
}

/// Every item of the cache and of the index, in the order of the list.
pub fn collect(cache: &Path, index: Option<&Index>) -> Result<BTreeMap<Key, Seen>, Failure> {
    let mut items: BTreeMap<Key, Seen> = BTreeMap::new();
    for key in in_cache(cache)? {
        items.entry(key).or_default().installed = true;
    }
    if let Some(index) = index {
        for p in &index.packages {
            items
                .entry(Key::Package(p.name.clone(), p.version))
                .or_default()
                .offered = true;
        }
        for t in &index.tools {
            for host in t.archives.keys() {
                items
                    .entry(Key::Tool(t.name.clone(), t.version.clone(), host.clone()))
                    .or_default()
                    .offered = true;
            }
        }
    }
    Ok(items)
}

/// The names of the subfolders of `dir`. A missing folder has none.
fn folders(dir: &Path) -> Result<Vec<String>, Failure> {
    let it = match fs::read_dir(dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_read(dir, &e)),
        Ok(it) => it,
    };
    let mut out = Vec::new();
    for entry in it {
        let entry = entry.map_err(|e| io_read(dir, &e))?;
        // A name that is not UTF-8 is no item a tool installs.
        if let Ok(name) = entry.file_name().into_string() {
            out.push(name);
        }
    }
    Ok(out)
}

fn io_read(path: &Path, e: &std::io::Error) -> Failure {
    Failure::new("io: read", format!("{}: {}", path.display(), e.kind()))
}

/// Whether `dir` holds a finished install.
fn holds(dir: &Path) -> Result<bool, Failure> {
    match install::cached(dir) {
        Ok(Cached::Holds(_)) => Ok(true),
        Ok(_) => Ok(false),
        Err(e) => Err(Failure::of(&e)),
    }
}

/// The items the cache holds: folders in the layout of "The cache folder"
/// whose names are in the form an install writes, with a valid record.
fn in_cache(cache: &Path) -> Result<Vec<Key>, Failure> {
    let mut keys = Vec::new();
    let packages = cache.join("packages");
    for name in folders(&packages)? {
        if !is_package_name(&name) {
            continue;
        }
        for v in folders(&packages.join(&name))? {
            let Some(version) = Version::parse(&v) else {
                continue;
            };
            if holds(&packages.join(&name).join(&v))? {
                keys.push(Key::Package(name.clone(), version));
            }
        }
    }
    let tools = cache.join("tools");
    for name in folders(&tools)? {
        if !TOOLS.contains(&name.as_str()) {
            continue;
        }
        for version in folders(&tools.join(&name))? {
            if !is_tool_version(&version) {
                continue;
            }
            for host in folders(&tools.join(&name).join(&version))? {
                if is_host_name(&host) && holds(&tools.join(&name).join(&version).join(&host))? {
                    keys.push(Key::Tool(name.clone(), version.clone(), host));
                }
            }
        }
    }
    Ok(keys)
}

/// Writes the `item` lines and the `result` line.
pub fn write(out: &mut Out<'_>, items: &BTreeMap<Key, Seen>) {
    for (key, seen) in items {
        let (kind, name, version, host) = match key {
            Key::Package(n, v) => ("package", n.as_str(), v.to_string(), ""),
            Key::Tool(n, v, h) => ("tool", n.as_str(), v.clone(), h.as_str()),
        };
        let mut text = format!("{kind} {name} {version}");
        if !host.is_empty() {
            text.push_str(&format!(" {host}"));
        }
        if seen.installed {
            text.push_str(" installed");
        }
        if seen.offered {
            text.push_str(" offered");
        }
        out.line(
            "item",
            vec![
                ("type", Val::s(kind)),
                ("name", Val::s(name)),
                ("version", Val::s(version)),
                ("host", Val::s(host)),
                ("installed", Val::Bool(seen.installed)),
                ("offered", Val::Bool(seen.offered)),
            ],
            &text,
        );
    }
    out.ok("sdk list", vec![("count", Val::Int(items.len() as u64))]);
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test; a failed unwrap is a failed test"
)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn the_order_of_the_list() {
        let mut keys = vec![
            Key::Tool("qemu".into(), "10.1.2".into(), "linux-x64".into()),
            Key::Tool("gcc".into(), "9".into(), "a".into()),
            Key::Tool("gcc".into(), "15".into(), "b".into()),
            Key::Tool("gcc".into(), "15".into(), "a".into()),
            Key::Package("pipos".into(), v("1.10.0")),
            Key::Package("pipos".into(), v("1.9.0")),
            Key::Package("abc".into(), v("2.0.0")),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                Key::Package("abc".into(), v("2.0.0")),
                Key::Package("pipos".into(), v("1.9.0")),
                Key::Package("pipos".into(), v("1.10.0")),
                Key::Tool("gcc".into(), "15".into(), "a".into()),
                Key::Tool("gcc".into(), "15".into(), "b".into()),
                Key::Tool("gcc".into(), "9".into(), "a".into()),
                Key::Tool("qemu".into(), "10.1.2".into(), "linux-x64".into()),
            ]
        );
    }

    #[test]
    fn a_missing_cache_lists_nothing() {
        let items = collect(Path::new("/no/such/robco/cache"), None).unwrap();
        assert!(items.is_empty());
    }
}
