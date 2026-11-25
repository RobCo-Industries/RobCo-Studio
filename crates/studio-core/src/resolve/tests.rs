// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the resolver, the index and `sdk.json` readers and the lock.
//! The fixtures are the specs' examples, copied byte for byte.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

use super::index::{self, Index, IndexPackage};
use super::lock::{self, Installed, Lock, LockedPackage};
use super::sdk_json;
use super::*;

const INDEX: &str = include_str!("../../tests/fixtures/index.json");
const SDK: &str = include_str!("../../tests/fixtures/sdk.json");
const SDK_TEMPLATES: &str = include_str!("../../tests/fixtures/sdk-templates.json");
const LOCK: &str = include_str!("../../tests/fixtures/robco.lock");
const REQUIRES: &str = include_str!("../../tests/fixtures/requires-index.json");
const OPEN: &str = include_str!("../../tests/fixtures/open-index.json");
const LIMIT: &str = include_str!("../../tests/fixtures/limit-index.json");
const LOCATION: &str = "https://example.invalid/robco/index.json";
const H: &str = "0000000000000000000000000000000000000000000000000000000000000001";

/// A folder of the test's own under the system's temp folder, removed on
/// drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "studio-resolve-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn listing(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn target(abi: &str) -> Target {
    Target {
        platform: PIPOS.to_owned(),
        abi: Abi::parse(abi).unwrap(),
    }
}

fn parse_index(text: &str) -> Result<Index, Error> {
    index::parse(text.as_bytes())
}

fn code<T: std::fmt::Debug>(r: Result<T, Error>) -> &'static str {
    r.unwrap_err().code()
}

/// The spec's index example with `pipos` entries at more versions.
fn pipos_index(versions: &[&str]) -> Index {
    let mut i = parse_index(INDEX).unwrap();
    let base = i.packages[0].clone();
    i.packages = versions
        .iter()
        .map(|v| {
            let version = Version::parse(v).unwrap();
            IndexPackage {
                version,
                abi: Some(Abi {
                    major: version.major,
                    minor: version.minor,
                }),
                ..base.clone()
            }
        })
        .collect();
    i
}

/// The entries of the spec's short example arrays, which hold only the
/// fields the resolver reads.
fn entries(text: &str) -> Vec<IndexPackage> {
    let v: Value = serde_json::from_str(text).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| IndexPackage {
            name: e["name"].as_str().unwrap().to_owned(),
            version: Version::parse(e["version"].as_str().unwrap()).unwrap(),
            abi: e["abi"].as_str().and_then(Abi::parse),
            requires: e["requires"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| Requirement {
                    name: r["name"].as_str().unwrap().to_owned(),
                    abi: Abi::parse(r["abi"].as_str().unwrap()).unwrap(),
                })
                .collect(),
            tools: Vec::new(),
            formats: e["formats"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_i64().unwrap()))
                .collect(),
            url: "u".to_owned(),
            size: 1,
            sha256: H.to_owned(),
        })
        .collect()
}

fn install(name: &str) -> Root {
    Root::Install {
        name: name.to_owned(),
        pick: InstallPick::Newest,
        argument: name.to_owned(),
    }
}

fn picked(r: Result<Vec<&IndexPackage>, Error>) -> Vec<String> {
    r.unwrap()
        .iter()
        .map(|p| format!("{} {}", p.name, p.version))
        .collect()
}

fn failure(r: Result<Vec<&IndexPackage>, Error>) -> String {
    r.unwrap_err().to_string()
}

/// The `text` blocks of the Examples of `SDK_PACKAGE.md`, the error lines.
const NO_LOG: &str = "sdk: no package: log: net 1.0.0 abi 1.4, ui 1.0.0 abi 1.5";
const NO_GFX: &str = "sdk: no package: gfx: pipos 1.4.0 abi 1.4";
const NEWER: &str = "sdk: no package: log: app 2.0.0 abi 1.5; 1.5.0 needs a newer tool: flags 2";

// The spec examples.

#[test]
fn resolve_spec_examples_parse_and_agree() {
    let i = parse_index(INDEX).unwrap();
    assert_eq!(i.packages.len(), 2);
    let s = sdk_json::parse(SDK.as_bytes()).unwrap();
    s.check_against(&i.packages[0]).unwrap();
    let t = sdk_json::parse(SDK_TEMPLATES.as_bytes()).unwrap();
    t.check_against(&i.packages[1]).unwrap();
    assert_eq!(t.abi, None);
    let l = Lock::parse(LOCK.as_bytes()).unwrap();
    assert_eq!(l.tools.len(), 2);
}

#[test]
fn resolve_spec_index_gives_spec_lock_bytes() {
    let i = parse_index(INDEX).unwrap();
    // Every host where both tools have an archive writes the same lock.
    for host in ["linux-x64", "macos-arm64", "windows-x64"] {
        let lock = resolve(&target("1.3"), &i, LOCATION, host).unwrap();
        assert_eq!(lock.to_text(), LOCK, "{host}");
    }
    assert_eq!(Lock::parse(LOCK.as_bytes()).unwrap().to_text(), LOCK);
}

#[test]
fn resolve_writes_same_bytes_twice() {
    let dir = TempDir::new("twice");
    let i = parse_index(INDEX).unwrap();
    let lock = resolve(&target("1.3"), &i, LOCATION, "linux-x64").unwrap();
    lock::write(dir.path(), &lock).unwrap();
    let first = fs::read(dir.path().join(lock::FILE)).unwrap();
    lock::write(dir.path(), &lock).unwrap();
    assert_eq!(fs::read(dir.path().join(lock::FILE)).unwrap(), first);
    assert_eq!(first, LOCK.as_bytes());
    assert_eq!(dir.listing(), vec![lock::FILE.to_owned()]);
}

#[test]
fn resolve_lock_writer_escapes_strings() {
    // The case of the spec's model: a path on Windows with a quote and a
    // control character.
    assert_eq!(
        lock::quote("C:\\idx\\\"a\"\u{1f}.json"),
        "\"C:\\\\idx\\\\\\\"a\\\"\\u001f.json\""
    );
    assert_eq!(lock::quote("\n\t\u{7f}é"), "\"\\u000a\\u0009\\u007fé\"");
    let mut lock = Lock::parse(LOCK.as_bytes()).unwrap();
    lock.index = "C:\\idx\n\u{80}".to_owned();
    let back = Lock::parse(lock.to_text().as_bytes()).unwrap();
    assert_eq!(back.index, lock.index);
}

#[test]
fn resolve_lock_is_sorted_whatever_the_order_given() {
    let mut lock = Lock::parse(LOCK.as_bytes()).unwrap();
    lock.tools.reverse();
    assert_eq!(lock.to_text(), LOCK);
    let mut reel = lock.packages[0].clone();
    reel.name = "reel".to_owned();
    lock.packages.insert(0, reel);
    let text = lock.to_text();
    assert!(text.find("name = \"pipos\"").unwrap() < text.find("name = \"reel\"").unwrap());
}

// Fit rules.

#[test]
fn resolve_fit_rules_for_abi_1_3() {
    let i = pipos_index(&["1.2.9", "1.3.0", "1.3.4", "1.4.0", "1.4.2", "2.0.0"]);
    let t = target("1.3");
    let fit: Vec<String> = i
        .packages
        .iter()
        .filter(|p| fits(&t, p))
        .map(|p| p.version.to_string())
        .collect();
    assert_eq!(fit, ["1.3.0", "1.3.4", "1.4.0", "1.4.2"]);
    let lock = resolve(&t, &i, "x", "linux-x64").unwrap();
    assert_eq!(lock.packages[0].version.to_string(), "1.4.2");
    let i = pipos_index(&["1.4.2", "1.10.0", "1.3.0", "1.9.9"]);
    let lock = resolve(&t, &i, "x", "linux-x64").unwrap();
    assert_eq!(lock.packages[0].version.to_string(), "1.10.0");
}

#[test]
fn resolve_passes_over_a_format_it_does_not_know() {
    let mut i = pipos_index(&["1.3.0", "1.4.0"]);
    i.packages[1].formats.insert("flags".to_owned(), 2);
    let lock = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap();
    assert_eq!(lock.packages[0].version.to_string(), "1.3.0");
    i.packages[1].formats.remove("flags");
    i.packages[1].formats.insert("sound".to_owned(), 1);
    let lock = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap();
    assert_eq!(lock.packages[0].version.to_string(), "1.3.0");
    i.packages[0].formats.insert("sound".to_owned(), 1);
    let e = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap_err();
    assert_eq!(
        e.to_string(),
        "sdk: no package: pipos: robco.toml abi 1.3; 1.4.0 needs a newer tool: sound 1"
    );
}

#[test]
fn resolve_never_derives_an_abi_from_a_version() {
    let i = parse_index(INDEX).unwrap();
    // templates 0.1.0 has no abi: it fits no project and no requires entry.
    assert!(!fits_requirement(
        Abi::parse("0.1").unwrap(),
        &i.packages[1]
    ));
    assert_eq!(
        resolve(&target("1.4"), &i, "x", "linux-x64")
            .unwrap_err()
            .to_string(),
        "sdk: no package: pipos: robco.toml abi 1.4"
    );
    let mut t = target("1.3");
    t.platform = "uos".to_owned();
    assert_eq!(code(resolve(&t, &i, "x", "linux-x64")), "sdk: no package");
}

// Resolving requires.

#[test]
fn resolve_requires_example_by_name_over_every_requirement() {
    let e = entries(REQUIRES);
    let want = ["app 1.0.0", "log 1.5.2", "net 1.0.0", "ui 1.0.0"];
    assert_eq!(picked(resolve_requires(&e, &install("app"))), want);
    // The order of the index and of a requires array does not count.
    let mut flipped = e.clone();
    flipped.reverse();
    for p in &mut flipped {
        p.requires.reverse();
    }
    assert_eq!(picked(resolve_requires(&flipped, &install("app"))), want);
    // Without log 1.5.2, net and ui disagree, and the detail names both.
    let short: Vec<IndexPackage> = e
        .iter()
        .filter(|p| p.version.to_string() != "1.5.2")
        .cloned()
        .collect();
    assert_eq!(failure(resolve_requires(&short, &install("app"))), NO_LOG);
    let mut back = short.clone();
    back.reverse();
    assert_eq!(failure(resolve_requires(&back, &install("app"))), NO_LOG);
    assert_eq!(
        failure(resolve_requires(&e, &install("nope"))),
        "sdk: no package: nope: command line nope"
    );
}

#[test]
fn resolve_open_index_fails_and_never_goes_back() {
    let e = entries(OPEN);
    let root = Root::Project(target("1.3"));
    assert_eq!(failure(resolve_requires(&e, &root)), NO_GFX);
    let fixed: Vec<IndexPackage> = e
        .into_iter()
        .filter(|p| p.version.to_string() != "1.4.0")
        .collect();
    assert_eq!(
        picked(resolve_requires(&fixed, &root)),
        ["gfx 1.3.0", "pipos 1.3.0"]
    );
}

#[test]
fn resolve_limit_index_names_the_newer_tool() {
    let e = entries(LIMIT);
    assert_eq!(failure(resolve_requires(&e, &install("app"))), NEWER);
    let older: Vec<IndexPackage> = e
        .into_iter()
        .filter(|p| !(p.name == "app" && p.version.to_string() == "2.0.0"))
        .collect();
    assert_eq!(
        picked(resolve_requires(&older, &install("app"))),
        ["app 1.0.0", "log 1.4.0"]
    );
}

#[test]
fn resolve_two_majors_of_one_name_fail() {
    let e = entries(
        r#"[
        {"name": "app", "version": "1.0.0", "abi": "1.0", "requires": [{"name": "log", "abi": "1.0"}], "formats": {}},
        {"name": "app", "version": "2.0.0", "abi": "2.0", "requires": [{"name": "log", "abi": "2.0"}, {"name": "ui", "abi": "1.0"}], "formats": {}},
        {"name": "ui", "version": "1.0.0", "abi": "1.0", "requires": [{"name": "log", "abi": "1.0"}], "formats": {}},
        {"name": "log", "version": "1.0.0", "abi": "1.0", "requires": [], "formats": {}},
        {"name": "log", "version": "2.0.0", "abi": "2.0", "requires": [], "formats": {}}
    ]"#,
    );
    assert!(check_closed(&e, &[]).is_ok());
    assert_eq!(
        failure(resolve_requires(&e, &install("app"))),
        "sdk: no package: log: app 2.0.0 abi 2.0, ui 1.0.0 abi 1.0"
    );
}

#[test]
fn resolve_keeps_a_requirement_whose_stater_was_passed_over() {
    // SDK_PACKAGE.md, "Resolving requires": a requirement stays in the set
    // once added. A pick only moves lower, so the root is `a` at any
    // version. Pass 2 picks a 2.0.0, which requires c abi 1.0. b then moves
    // a to 1.0.0, which requires c abi 2.0. The requirement of a 2.0.0
    // still counts, so no c fits both. Dropping it would pick c 2.0.0.
    let e = entries(
        r#"[
        {"name": "a", "version": "2.0.0", "abi": "2.0", "requires": [{"name": "b", "abi": "1.0"}, {"name": "c", "abi": "1.0"}], "formats": {}},
        {"name": "a", "version": "1.0.0", "abi": "1.0", "requires": [{"name": "b", "abi": "1.0"}, {"name": "c", "abi": "2.0"}], "formats": {}},
        {"name": "b", "version": "1.0.0", "abi": "1.0", "requires": [{"name": "a", "abi": "1.0"}], "formats": {}},
        {"name": "c", "version": "1.0.0", "abi": "1.0", "requires": [], "formats": {}},
        {"name": "c", "version": "2.0.0", "abi": "2.0", "requires": [], "formats": {}}
    ]"#,
    );
    assert_eq!(
        failure(resolve_requires(&e, &install("a"))),
        "sdk: no package: c: a 1.0.0 abi 2.0, a 2.0.0 abi 1.0"
    );
}

#[test]
fn resolve_leaves_out_a_name_no_chain_reaches() {
    // Only the root and what its picks require enter the resolution.
    let e = entries(
        r#"[
        {"name": "app", "version": "2.0.0", "abi": "2.0", "requires": [{"name": "log", "abi": "1.0"}, {"name": "ui", "abi": "1.0"}], "formats": {}},
        {"name": "ui", "version": "1.0.0", "abi": "1.0", "requires": [], "formats": {}},
        {"name": "log", "version": "1.0.0", "abi": "1.0", "requires": [], "formats": {}}
    ]"#,
    );
    assert_eq!(picked(resolve_requires(&e, &install("ui"))), ["ui 1.0.0"]);
}

#[test]
fn resolve_install_forms() {
    let mut e = entries(LIMIT);
    let minor = Root::Install {
        name: "log".to_owned(),
        pick: InstallPick::Minor(1, 4),
        argument: "log@1.4".to_owned(),
    };
    assert_eq!(picked(resolve_requires(&e, &minor)), ["log 1.4.0"]);
    let exact = Root::Install {
        name: "log".to_owned(),
        pick: InstallPick::Exact(Version::parse("1.5.0").unwrap()),
        argument: "log@1.5.0".to_owned(),
    };
    assert_eq!(code(resolve_requires(&e, &exact)), "package: format");
    e[3].formats.clear();
    assert_eq!(picked(resolve_requires(&e, &exact)), ["log 1.5.0"]);
    assert_eq!(
        failure(resolve_requires(&e, &install("gcc"))),
        "sdk: no package: gcc: command line gcc"
    );
}

// Tools.

#[test]
fn resolve_takes_the_tools_the_package_names() {
    let mut i = parse_index(INDEX).unwrap();
    let mut newer = i.tools[0].clone();
    newer.version = "16.0.0-1.1".to_owned();
    i.tools.push(newer);
    let lock = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap();
    assert_eq!(lock.tools[0].version, "15.2.1-1.1");
    assert_eq!(lock.tools[1].version, "10.1.2");
}

#[test]
fn resolve_no_tool() {
    let mut i = parse_index(INDEX).unwrap();
    i.tools[1].version = "10.1.3".to_owned();
    let e = resolve(&target("1.3"), &i, "x", "linux-x64").unwrap_err();
    assert_eq!(
        e.to_string(),
        "sdk: no tool: the index lists no qemu 10.1.2"
    );
}

#[test]
fn resolve_needs_both_tools_for_the_host() {
    let i = parse_index(INDEX).unwrap();
    // gcc has linux-arm64 and macos-x64; qemu has neither.
    for host in ["linux-arm64", "macos-x64"] {
        let e = resolve(&target("1.3"), &i, "x", host).unwrap_err();
        assert_eq!(
            e,
            Error::IndexHost {
                tool: "qemu".to_owned(),
                version: "10.1.2".to_owned(),
                host: host.to_owned(),
            }
        );
    }
}

// A closed index.

#[test]
fn resolve_closure_check_of_the_index_writer() {
    let i = parse_index(INDEX).unwrap();
    assert!(check_closed(&i.packages, &i.tools).is_ok());
    assert!(check_closed(&entries(REQUIRES), &[]).is_ok());
    assert!(check_closed(&entries(LIMIT), &[]).is_ok());
    assert_eq!(
        check_closed(&entries(OPEN), &[]).unwrap_err(),
        "pipos 1.4.0 requires gfx abi 1.4, and no package of the index meets it"
    );
    let fixed: Vec<IndexPackage> = entries(OPEN)
        .into_iter()
        .filter(|p| p.version.to_string() != "1.4.0")
        .collect();
    assert!(check_closed(&fixed, &[]).is_ok());
    let no_qemu: Vec<_> = i
        .tools
        .iter()
        .filter(|t| t.name != "qemu")
        .cloned()
        .collect();
    assert!(check_closed(&i.packages, &no_qemu).is_err());
    let mut empty = i.tools.clone();
    empty[1].archives.clear();
    assert!(check_closed(&i.packages, &empty).is_err());
}

// Reading the index.

#[test]
fn resolve_index_syntax() {
    assert_eq!(code(parse_index("not json")), "index: syntax");
    assert_eq!(code(parse_index("[]")), "index: syntax");
    assert_eq!(
        code(index::parse(b"{\"format\": 1, \xff}")),
        "index: syntax"
    );
    let dir = TempDir::new("index-read");
    assert_eq!(
        code(index::read_file(&dir.path().join("none.json"))),
        "index: read"
    );
}

#[test]
fn resolve_index_format() {
    for f in ["2", "0", "-1", "\"1\"", "1.5", "null"] {
        let text = INDEX.replacen("\"format\": 1", &format!("\"format\": {f}"), 1);
        assert_eq!(code(parse_index(&text)), "index: format", "{f}");
    }
    assert_eq!(
        code(parse_index(r#"{"packages": 1, "tools": []}"#)),
        "index: format"
    );
    // format comes before a bad entry.
    assert_eq!(
        code(parse_index(r#"{"format": 2, "packages": 1}"#)),
        "index: format"
    );
}

#[test]
fn resolve_index_entries() {
    let cases = [
        // A required field that is missing, in the order of the table.
        (
            "\"formats\": {\"rimg\": 1, \"flags\": 1, \"boot\": 1, \"runner\": 1},",
            "",
            "packages[0].formats is missing",
        ),
        (
            "\"version\": \"1.3.0\"",
            "\"version\": \"1.03.0\"",
            "packages[0].version",
        ),
        ("\"abi\": \"1.3\",", "", "packages[0].abi is missing"),
        ("\"abi\": \"1.3\"", "\"abi\": \"1.4\"", "packages[0].abi"),
        ("\"abi\": \"1.3\"", "\"abi\": \"1.03\"", "packages[0].abi"),
        (
            "\"name\": \"pipos\"",
            "\"name\": \"Pipos\"",
            "packages[0].name",
        ),
        (
            "\"url\": \"https://example.invalid/robco/pipos-1.3.0.zip\"",
            "\"url\": \"http://x/p.zip\"",
            "packages[0].url",
        ),
        (
            "\"url\": \"https://example.invalid/robco/pipos-1.3.0.zip\"",
            "\"url\": \"../p.zip\"",
            "packages[0].url",
        ),
        ("\"size\": 812345", "\"size\": -1", "packages[0].size"),
        (
            H,
            "000000000000000000000000000000000000000000000000000000000000000A",
            "packages[0].sha256",
        ),
        (
            "\"name\": \"templates\",\n      \"version\": \"0.1.0\",",
            "\"name\": \"templates\",\n      \"version\": \"0.1.0\", \"abi\": \"0.1\",",
            "packages[1].abi",
        ),
        (
            "\"name\": \"templates\",\n      \"version\": \"0.1.0\",",
            "\"name\": \"templates\",\n      \"version\": \"0.1.0\", \"tools\": [],",
            "packages[1].tools",
        ),
        (
            "\"version\": \"10.1.2\",\n",
            "\"version\": \"10.1 2\",\n",
            "tools[1].version",
        ),
        (
            "\"linux-x64\": {\"url\": \"https://example.invalid/qemu",
            "\"Linux\": {\"url\": \"https://example.invalid/qemu",
            "tools[1].archives.Linux",
        ),
        ("\"root\": \"\"}", "\"root\": 0}", "tools[1].archives"),
        (
            "{\"name\": \"gcc\", \"version\": \"15.2.1-1.1\"},",
            "",
            "packages[0].tools",
        ),
    ];
    for (from, to, place) in cases {
        let text = INDEX.replacen(from, to, 1);
        assert_ne!(text, INDEX, "{from}");
        let e = parse_index(&text).unwrap_err();
        assert_eq!(e.code(), "index: entry", "{from}");
        assert!(
            e.to_string().starts_with(&format!("index: entry: {place}")),
            "{e}"
        );
    }
    // A local path is a URL too.
    let text = INDEX.replacen(
        "https://example.invalid/robco/pipos-1.3.0.zip",
        "pkg/pipos.zip",
        1,
    );
    assert_eq!(parse_index(&text).unwrap().packages[0].url, "pkg/pipos.zip");
}

#[test]
fn resolve_index_skips_unknown_tools_and_keeps_unknown_hosts() {
    let text = INDEX.replacen(
        "\"tools\": [\n    {",
        "\"tools\": [\n    {\"name\": \"gdb\", \"archives\": 7},\n    {\"name\": \"gdb\"},\n    {",
        1,
    );
    assert_ne!(text, INDEX);
    let i = parse_index(&text).unwrap();
    assert_eq!(i.tools.len(), 2);
    let text = INDEX.replacen(
        "\"linux-x64\": {\"url\": \"https://example.invalid/qemu",
        "\"freebsd-x64\": {\"url\": \"https://example.invalid/qemu",
        1,
    );
    let i = parse_index(&text).unwrap();
    assert!(i.tools[1].archives.contains_key("freebsd-x64"));
    let lock = resolve(&target("1.3"), &i, "x", "macos-arm64").unwrap();
    assert!(lock.tools[1].archives.contains_key("freebsd-x64"));
    // A tool name that is not a string is still an error.
    let text = INDEX.replacen("\"name\": \"qemu\"", "\"name\": 7", 1);
    assert_eq!(code(parse_index(&text)), "index: entry");
}

#[test]
fn resolve_index_duplicates() {
    let i: Value = serde_json::from_str(INDEX).unwrap();
    let mut twice = i.clone();
    let p = twice["packages"][0].clone();
    twice["packages"].as_array_mut().unwrap().push(p);
    assert_eq!(code(parse_index(&twice.to_string())), "index: duplicate");
    let mut twice = i.clone();
    let t = twice["tools"][1].clone();
    twice["tools"].as_array_mut().unwrap().push(t);
    assert_eq!(code(parse_index(&twice.to_string())), "index: duplicate");
    // Duplicates come last: a bad entry after the pair is reported first.
    let mut both = i.clone();
    let p = both["packages"][0].clone();
    both["packages"].as_array_mut().unwrap().push(p);
    both["tools"][1]["version"] = Value::from("");
    assert_eq!(code(parse_index(&both.to_string())), "index: entry");
}

#[test]
fn resolve_required_formats_index_and_sdk_json() {
    let pipos = r#""formats": {"rimg": 1, "flags": 1, "boot": 1, "runner": 1}"#;
    let tpl = r#""formats": {"templates": 1, "project": 1, "scenario": 1}"#;
    for (from, file) in [(pipos, 0), (tpl, 1)] {
        for drop in [
            "rimg",
            "flags",
            "boot",
            "runner",
            "templates",
            "project",
            "scenario",
        ] {
            if !from.contains(&format!("\"{drop}\"")) {
                continue;
            }
            let to = from
                .replace(&format!("\"{drop}\": 1, "), "")
                .replace(&format!(", \"{drop}\": 1"), "");
            assert_ne!(to, from);
            let e = parse_index(&INDEX.replacen(from, &to, 1)).unwrap_err();
            assert_eq!(e.code(), "index: entry", "{drop}");
            assert!(e.to_string().contains(&format!("formats.{drop}")), "{e}");
            let sdk = if file == 0 { SDK } else { SDK_TEMPLATES };
            let e = sdk_json::parse(sdk.replacen(from, &to, 1).as_bytes()).unwrap_err();
            assert_eq!(e.code(), "package: manifest", "{drop}");
            assert!(e.to_string().contains(&format!("formats.{drop}")), "{e}");
        }
        let e = parse_index(&INDEX.replacen(from, "\"formats\": {}", 1)).unwrap_err();
        assert_eq!(e.code(), "index: entry");
    }
}

// sdk.json.

fn sdk_code(text: &str) -> &'static str {
    code(sdk_json::parse(text.as_bytes()))
}

#[test]
fn resolve_sdk_json_format_first() {
    assert_eq!(sdk_code("{"), "package: manifest");
    assert_eq!(sdk_code("[]"), "package: manifest");
    for f in ["2", "0", "\"1\"", "1.5"] {
        let text = SDK.replacen("\"format\": 1", &format!("\"format\": {f}"), 1);
        assert_eq!(sdk_code(&text), "package: format", "{f}");
    }
    let text = SDK.replacen("\"format\": 1,", "", 1);
    assert_eq!(sdk_code(&text), "package: format");
    // A format it does not know, by name or by value, comes before the
    // fields: here `name` is gone too.
    for f in ["\"flags\": 2", "\"sound\": 1", "\"flags\": \"1\""] {
        let text = SDK
            .replacen("\"flags\": 1", f, 1)
            .replacen("\"name\": \"pipos\",", "", 1);
        assert_eq!(sdk_code(&text), "package: format", "{f}");
    }
}

#[test]
fn resolve_sdk_json_fields() {
    let cases = [
        ("\"name\": \"pipos\",", ""),
        ("\"version\": \"1.3.0\"", "\"version\": \"01.3.0\""),
        ("\"abi\": \"1.3\",", ""),
        ("\"abi\": \"1.3\"", "\"abi\": \"1.4\""),
        ("\"tools\": [", "\"tool\": ["),
        ("\"firmware\": {", "\"firmwar\": {"),
        ("\"base_url\"", "\"baseurl\""),
        (
            H,
            "000000000000000000000000000000000000000000000000000000000000000F",
        ),
        ("\"path\": \"boot.json\"", "\"path\": \"LICENSES/MIT.txt\""),
        (
            "\"path\": \"boot/PIPOS.CFG\"",
            "\"path\": \"Boot/PIPOS.CFG\"",
        ),
        ("\"commit\": \"1111", "\"commit\": \"A111"),
    ];
    for (from, to) in cases {
        let text = SDK.replacen(from, to, 1);
        assert_ne!(text, SDK, "{from}");
        assert_eq!(sdk_code(&text), "package: manifest", "{from}");
    }
    let templates = [
        (
            "\"version\": \"0.1.0\",",
            "\"version\": \"0.1.0\", \"abi\": \"0.1\",",
        ),
        (
            "\"version\": \"0.1.0\",",
            "\"version\": \"0.1.0\", \"tools\": [],",
        ),
        (
            "\"version\": \"0.1.0\",",
            "\"version\": \"0.1.0\", \"firmware\": {},",
        ),
    ];
    for (from, to) in templates {
        let text = SDK_TEMPLATES.replacen(from, to, 1);
        assert_eq!(sdk_code(&text), "package: manifest", "{to}");
    }
}

#[test]
fn resolve_sdk_json_firmware_pins() {
    let fw: Value = serde_json::from_str(SDK).unwrap();
    let files = fw["firmware"]["files"].as_array().unwrap().clone();
    let with = |list: Vec<Value>| {
        let mut v = fw.clone();
        v["firmware"]["files"] = Value::from(list);
        sdk_json::parse(v.to_string().as_bytes())
    };
    // Any order parses.
    let mut turned = files.clone();
    turned.reverse();
    assert!(with(turned).is_ok());
    let msg = |r: Result<sdk_json::SdkJson, Error>| r.unwrap_err().to_string();
    assert_eq!(code(with(vec![])), "package: manifest");
    let lacks = msg(with(files[..2].to_vec()));
    assert!(lacks.contains("fixup.dat"), "{lacks}");
    let mut twice = files.clone();
    twice.push(files[0].clone());
    let e = msg(with(twice));
    assert!(e.contains("bootcode.bin") && e.contains("twice"), "{e}");
    let mut other = files.clone();
    other[2]["name"] = Value::from("start4.elf");
    let e = msg(with(other));
    assert!(e.contains("start4.elf"), "{e}");
    for e in [lacks, e] {
        assert!(e.starts_with("package: manifest"), "{e}");
    }
    let mut v = fw.clone();
    v["firmware"]["repo"] = Value::from("someone/firmware");
    assert_eq!(sdk_code(&v.to_string()), "package: manifest");
}

#[test]
fn resolve_sdk_json_must_equal_its_index_entry() {
    let i = parse_index(INDEX).unwrap();
    let s = sdk_json::parse(SDK.as_bytes()).unwrap();
    let e = &i.packages[0];
    let differs = |x: &sdk_json::SdkJson, field: &str| {
        let err = x.check_against(e).unwrap_err();
        assert_eq!(err.code(), "package: manifest");
        assert!(
            err.to_string().contains(&format!(" {field} differs")),
            "{err}"
        );
    };
    let mut x = s.clone();
    x.name = "pipos-x".to_owned();
    differs(&x, "name");
    let mut x = s.clone();
    x.version.patch = 1;
    differs(&x, "version");
    let mut x = s.clone();
    x.abi = None;
    differs(&x, "abi");
    let mut x = s.clone();
    x.tools[1].version = "10.1.3".to_owned();
    differs(&x, "tools");
    let mut x = s.clone();
    x.formats.insert("rimg".to_owned(), 2);
    differs(&x, "formats");
    // requires: the same entries in the same order.
    let r = |n: &str| Requirement {
        name: n.to_owned(),
        abi: Abi::parse("1.0").unwrap(),
    };
    let mut x = s.clone();
    let mut entry = e.clone();
    x.requires = vec![r("a"), r("b")];
    entry.requires = vec![r("b"), r("a")];
    let err = x.check_against(&entry).unwrap_err();
    assert!(err.to_string().contains(" requires differs"), "{err}");
    entry.requires = x.requires.clone();
    x.check_against(&entry).unwrap();
}

// Reading the lock.

fn lock_err(text: &str) -> Error {
    Lock::parse(text.as_bytes()).unwrap_err()
}

#[test]
fn resolve_lock_syntax_format_and_unknown_keys() {
    let e = lock_err("format = 1\nindex = \"x\"\nbad =\n");
    assert_eq!(e.code(), "lock: syntax");
    assert!(e.to_string().starts_with("lock: syntax: line 3"), "{e}");
    assert_eq!(
        Lock::parse(b"format = 1\n\xff").unwrap_err().code(),
        "lock: syntax"
    );
    for f in ["2", "0", "\"1\"", "1.0"] {
        let text = LOCK.replacen("format = 1", &format!("format = {f}"), 1);
        assert_eq!(lock_err(&text).code(), "lock: format", "{f}");
    }
    assert_eq!(
        lock_err(&LOCK.replacen("format = 1\n", "", 1)).code(),
        "lock: format"
    );
    // A later format reports its format before an unknown key.
    let text = LOCK.replacen("format = 1", "format = 2\nwhen = 1", 1);
    assert_eq!(lock_err(&text).code(), "lock: format");
    let text = LOCK.replacen("format = 1", "format = 1\nwhen = 1", 1);
    assert_eq!(lock_err(&text), Error::LockUnknownKey("when".to_owned()));
    let text = LOCK.replacen("version = \"10.1.2\"", "version = \"10.1.2\"\narch = 1", 1);
    assert_eq!(
        lock_err(&text),
        Error::LockUnknownKey("tool.arch".to_owned())
    );
    // Unknown keys come before a bad entry.
    let text = LOCK
        .replacen("sha256 = \"", "date = 1\nsha256 = \"", 1)
        .replacen("index = ", "index = 1\n#", 1);
    assert_eq!(
        lock_err(&text),
        Error::LockUnknownKey("package.date".to_owned())
    );
}

#[test]
fn resolve_lock_entries() {
    let cases = [
        (
            "index = \"https://example.invalid/robco/index.json\"\n",
            "",
            "index is missing",
        ),
        (
            "index = \"https://example.invalid/robco/index.json\"",
            "index = 1",
            "index is not a string",
        ),
        (
            "version = \"1.3.0\"",
            "version = \"1.03.0\"",
            "package.version = \"1.03.0\"",
        ),
        (
            "version = \"1.3.0\"",
            "version = 1",
            "package.version is not a string",
        ),
        (
            "name = \"pipos\"",
            "name = \"Pipos\"",
            "package.name = \"Pipos\"",
        ),
        (
            H,
            "000000000000000000000000000000000000000000000000000000000000000A",
            "package.sha256 = ",
        ),
        ("name = \"qemu\"", "name = \"gdb\"", "tool.name = \"gdb\""),
        (
            "name = \"qemu\"",
            "name = \"gcc\"",
            "tool.name = \"gcc\" is listed twice",
        ),
        (
            "version = \"10.1.2\"",
            "version = \"10 1\"",
            "tool.version = \"10 1\"",
        ),
        (
            "linux-x64 = \"0000000000000000000000000000000000000000000000000000000000000007\"",
            "\"Linux\" = \"0000000000000000000000000000000000000000000000000000000000000007\"",
            "tool.archives.\"Linux\"",
        ),
        (
            "linux-x64 = \"0000000000000000000000000000000000000000000000000000000000000007\"",
            "linux-x64 = 7",
            "tool.archives.linux-x64 is not a string",
        ),
    ];
    for (from, to, detail) in cases {
        let text = LOCK.replacen(from, to, 1);
        assert_ne!(text, LOCK, "{from}");
        let e = lock_err(&text);
        assert_eq!(e.code(), "lock: entry", "{from}");
        let shown = e.to_string();
        assert!(
            shown.starts_with(&format!("lock: entry: {detail}")),
            "{shown}"
        );
        assert!(shown.ends_with("run robco update"), "{shown}");
    }
    let twice = LOCK.replacen(
        "\n[[tool]]",
        &format!(
            "\n[[package]]\nname = \"pipos\"\nversion = \"1.4.0\"\nsha256 = \"{H}\"\n\n[[tool]]"
        ),
        1,
    );
    assert!(
        lock_err(&twice)
            .to_string()
            .contains("package.name = \"pipos\" is listed twice")
    );
    let empty = "format = 1\nindex = \"x\"\n\n[[tool]]\nname = \"gcc\"\nversion = \"1\"\n\n[tool.archives]\n";
    assert!(
        lock_err(empty)
            .to_string()
            .starts_with("lock: entry: tool.archives is empty")
    );
    let table = "format = 1\nindex = \"x\"\n\n[package]\nname = \"pipos\"\n";
    assert_eq!(lock_err(table).code(), "lock: entry");
}

#[test]
fn resolve_lock_absent_lists_are_stale_not_entry() {
    let l = Lock::parse(b"format = 1\nindex = \"x\"\n").unwrap();
    assert!(l.packages.is_empty() && l.tools.is_empty());
    let e = l
        .check_stale(&target("1.3"), "linux-x64", None)
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "lock: stale: package pipos is needed and not listed: run robco update"
    );
    let no_tools = LOCK.split("\n[[tool]]").next().unwrap().to_owned();
    let e = Lock::parse(no_tools.as_bytes())
        .unwrap()
        .check_stale(&target("1.3"), "linux-x64", None)
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "lock: stale: tool gcc is not listed: run robco update"
    );
}

#[test]
fn resolve_lock_io_errors() {
    let dir = TempDir::new("io");
    assert_eq!(lock::read(dir.path()).unwrap(), None);
    fs::create_dir(dir.path().join(lock::FILE)).unwrap();
    assert_eq!(code(lock::read(dir.path())), "io: read");
    let lock = Lock::parse(LOCK.as_bytes()).unwrap();
    assert_eq!(
        code(lock::write(&dir.path().join("none"), &lock)),
        "io: write"
    );
}

#[test]
fn resolve_lock_write_leaves_the_old_temporary_name_alone() {
    let dir = TempDir::new("oldtmp");
    let old = dir.path().join("robco.lock.tmp");
    fs::write(&old, "someone else's").unwrap();
    let lock = Lock::parse(LOCK.as_bytes()).unwrap();
    lock::write(dir.path(), &lock).unwrap();
    lock::write(dir.path(), &lock).unwrap();
    assert_eq!(fs::read_to_string(&old).unwrap(), "someone else's");
    assert_eq!(
        fs::read(dir.path().join(lock::FILE)).unwrap(),
        LOCK.as_bytes()
    );
    assert_eq!(
        dir.listing(),
        vec![lock::FILE.to_owned(), "robco.lock.tmp".to_owned()]
    );
}

#[cfg(unix)]
#[test]
fn resolve_lock_write_never_follows_a_link() {
    let dir = TempDir::new("link");
    let target = dir.path().join("target");
    fs::write(&target, "kept").unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("robco.lock.tmp")).unwrap();
    // Links at the first names the writer picks too: it passes over them.
    for n in 0..8 {
        std::os::unix::fs::symlink(
            &target,
            dir.path()
                .join(format!("{}{}.{n}", lock::TEMP_PREFIX, std::process::id())),
        )
        .unwrap();
    }
    let lock = Lock::parse(LOCK.as_bytes()).unwrap();
    lock::write(dir.path(), &lock).unwrap();
    assert_eq!(
        fs::read(dir.path().join(lock::FILE)).unwrap(),
        LOCK.as_bytes()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "kept");
}

#[test]
fn resolve_lock_failed_write_keeps_the_old_lock_and_no_temporary_file() {
    let dir = TempDir::new("failed");
    // A folder where the lock goes: the rename over it fails.
    let old = dir.path().join(lock::FILE);
    fs::create_dir(&old).unwrap();
    fs::write(old.join("inside"), "old").unwrap();
    let lock = Lock::parse(LOCK.as_bytes()).unwrap();
    assert_eq!(code(lock::write(dir.path(), &lock)), "io: write");
    assert_eq!(fs::read_to_string(old.join("inside")).unwrap(), "old");
    assert_eq!(dir.listing(), vec![lock::FILE.to_owned()]);
}

// A stale lock.

fn installed_map(
    edges: &[(&str, &str, &str)],
    abis: &[(&str, &str)],
) -> impl Fn(&LockedPackage) -> Installed {
    let edges: Vec<(String, String, String)> = edges
        .iter()
        .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
        .collect();
    let abis: Vec<(String, Abi)> = abis
        .iter()
        .map(|(n, a)| (n.to_string(), Abi::parse(a).unwrap()))
        .collect();
    move |p: &LockedPackage| Installed {
        abi: abis.iter().find(|(n, _)| *n == p.name).map(|(_, a)| *a),
        requires: edges
            .iter()
            .filter(|(by, _, _)| *by == p.name)
            .map(|(_, n, abi)| Requirement {
                name: n.clone(),
                abi: Abi::parse(abi).unwrap(),
            })
            .collect(),
    }
}

fn lock_with(extra: &[(&str, &str)]) -> Lock {
    let mut l = Lock::parse(LOCK.as_bytes()).unwrap();
    for (name, version) in extra {
        l.packages.push(LockedPackage {
            name: (*name).to_owned(),
            version: Version::parse(version).unwrap(),
            sha256: H.to_owned(),
        });
    }
    l
}

fn stale(l: &Lock, t: &Target, edges: &[(&str, &str, &str)]) -> String {
    stale_with(l, t, edges, &[("pipos", "1.3"), ("gfx", "1.3")])
}

fn stale_with(l: &Lock, t: &Target, edges: &[(&str, &str, &str)], abis: &[(&str, &str)]) -> String {
    let f = installed_map(edges, abis);
    match l.check_stale(t, "linux-x64", Some(&f)) {
        Ok(()) => "ok".to_owned(),
        Err(e) => {
            assert_eq!(e.code(), "lock: stale");
            e.to_string()
        }
    }
}

#[test]
fn resolve_stale_judges_a_package_other_than_pipos_by_its_declared_abi() {
    let l = lock_with(&[("gfx", "2.0.0")]);
    let gfx = [("gfx", "1.5")];
    // gfx 2.0.0 declares ABI 1.5: its version says nothing about its ABI.
    assert_eq!(
        stale_with(&l, &target("1.3"), &[("pipos", "gfx", "1.5")], &gfx),
        "ok"
    );
    for want in ["1.6", "2.0"] {
        assert_eq!(
            stale_with(&l, &target("1.3"), &[("pipos", "gfx", want)], &gfx),
            format!(
                "lock: stale: package gfx 2.0.0 does not fit pipos 1.3.0 abi {want}: run robco update"
            )
        );
    }
    // A package without a declared ABI fits no requires entry.
    assert_eq!(
        stale_with(&l, &target("1.3"), &[("pipos", "gfx", "1.0")], &[]),
        "lock: stale: package gfx 2.0.0 does not fit pipos 1.3.0 abi 1.0: run robco update"
    );
    // pipos is judged by its version, whatever its sdk.json says.
    let l = lock_with(&[("gfx", "1.0.0")]);
    assert_eq!(
        stale_with(
            &l,
            &target("1.3"),
            &[("gfx", "pipos", "1.4")],
            &[("gfx", "1.0"), ("pipos", "1.4")]
        ),
        "lock: stale: package pipos 1.3.0 does not fit gfx 1.0.0 abi 1.4: run robco update"
    );
    assert_eq!(
        stale_with(
            &l,
            &target("1.3"),
            &[("gfx", "pipos", "1.3"), ("pipos", "gfx", "1.0")],
            &[("gfx", "1.0")]
        ),
        "ok"
    );
    // Without installed data, a requirement on gfx cannot be judged, and no
    // ABI is guessed from its version: the check passes.
    let l = lock_with(&[("gfx", "2.0.0")]);
    assert!(l.check_stale(&target("1.3"), "linux-x64", None).is_ok());
}

#[test]
fn resolve_stale_rules() {
    let l = lock_with(&[]);
    assert_eq!(stale(&l, &target("1.3"), &[]), "ok");
    assert_eq!(stale(&l, &target("1.2"), &[]), "ok");
    // Rule 1: the manifest's requirement.
    assert_eq!(
        stale(&l, &target("1.4"), &[]),
        "lock: stale: package pipos 1.3.0 does not fit robco.toml platform pipos abi 1.4: run robco update"
    );
    let mut uos = target("1.3");
    uos.platform = "uos".to_owned();
    assert!(stale(&l, &uos, &[]).contains("does not fit robco.toml platform uos"));
    // Rule 1: a requires entry of another listed package.
    let l = lock_with(&[("gfx", "1.3.0")]);
    assert_eq!(
        stale(&l, &target("1.3"), &[("pipos", "gfx", "1.5")]),
        "lock: stale: package gfx 1.3.0 does not fit pipos 1.3.0 abi 1.5: run robco update"
    );
    assert_eq!(stale(&l, &target("1.3"), &[("pipos", "gfx", "1.2")]), "ok");
    // Only another package's entry counts: gfx naming itself at an ABI it
    // does not serve is not stale.
    assert_eq!(
        stale(
            &l,
            &target("1.3"),
            &[("pipos", "gfx", "1.2"), ("gfx", "gfx", "1.5")]
        ),
        "ok"
    );
    // Rule 2: a needed package that is not listed, from any listed package.
    assert_eq!(
        stale(
            &l,
            &target("1.3"),
            &[("pipos", "gfx", "1.2"), ("gfx", "log", "1.0")]
        ),
        "lock: stale: package log is needed and not listed: run robco update"
    );
    // Rule 3: a listed package nothing needs.
    assert_eq!(
        stale(&l, &target("1.3"), &[]),
        "lock: stale: package gfx 1.3.0 is listed and not needed: run robco update"
    );
    // Before the install, rule 3 cannot see requires, so it waits.
    assert!(l.check_stale(&target("1.3"), "linux-x64", None).is_ok());
    // Rule 4: a tool with no archive for the host.
    let e = lock_with(&[])
        .check_stale(&target("1.3"), "linux-arm64", None)
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "lock: stale: tool qemu 10.1.2 has no archive for linux-arm64: run robco update"
    );
    // The order of the list: rule 1 is named before rule 4.
    let e = lock_with(&[])
        .check_stale(&target("1.4"), "linux-arm64", None)
        .unwrap_err();
    assert!(e.to_string().contains("pipos 1.3.0 does not fit"), "{e}");
}

#[test]
fn resolve_stale_lock_fails_and_writes_nothing() {
    let dir = TempDir::new("stale");
    fs::write(dir.path().join(lock::FILE), LOCK).unwrap();
    let i = parse_index(INDEX).unwrap();
    let e = lock::use_or_resolve(dir.path(), &target("1.4"), "linux-x64", &|| {
        resolve(&target("1.4"), &i, LOCATION, "linux-x64")
    })
    .unwrap_err();
    assert_eq!(e.code(), "lock: stale");
    assert_eq!(
        fs::read(dir.path().join(lock::FILE)).unwrap(),
        LOCK.as_bytes()
    );
    assert_eq!(dir.listing(), vec![lock::FILE.to_owned()]);
}

#[test]
fn resolve_no_lock_writes_nothing_and_write_gives_the_lock_bytes() {
    let dir = TempDir::new("fresh");
    let i = parse_index(INDEX).unwrap();
    let go = || resolve(&target("1.3"), &i, LOCATION, "linux-x64");
    let r = lock::use_or_resolve(dir.path(), &target("1.3"), "linux-x64", &go).unwrap();
    let lock::Resolution::New(l) = r else {
        panic!("no lock gives a new resolution")
    };
    assert!(dir.listing().is_empty(), "{:?}", dir.listing());
    lock::write(dir.path(), &l).unwrap();
    assert_eq!(
        fs::read(dir.path().join(lock::FILE)).unwrap(),
        LOCK.as_bytes()
    );
}

#[test]
fn resolve_fitting_lock_is_kept_untouched() {
    let dir = TempDir::new("kept");
    let path = dir.path().join(lock::FILE);
    fs::write(&path, LOCK).unwrap();
    let old = fs::File::options().write(true).open(&path).unwrap();
    let past = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    old.set_modified(past).unwrap();
    drop(old);
    let r = lock::use_or_resolve(dir.path(), &target("1.2"), "linux-x64", &|| {
        unreachable!("a fitting lock is used unchanged")
    })
    .unwrap();
    assert_eq!(
        r,
        lock::Resolution::Locked(Lock::parse(LOCK.as_bytes()).unwrap())
    );
    assert_eq!(fs::read(&path).unwrap(), LOCK.as_bytes());
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), past);
    assert_eq!(dir.listing(), vec![lock::FILE.to_owned()]);
}

#[test]
fn resolve_again_writes_nothing() {
    let dir = TempDir::new("update");
    fs::write(dir.path().join(lock::FILE), b"old").unwrap();
    let i = parse_index(INDEX).unwrap();
    let e =
        lock::resolve_again(&|| resolve(&target("1.9"), &i, LOCATION, "linux-x64")).unwrap_err();
    assert_eq!(e.code(), "sdk: no package");
    let r = lock::resolve_again(&|| resolve(&target("1.3"), &i, LOCATION, "linux-x64")).unwrap();
    assert!(matches!(r, lock::Resolution::New(_)));
    assert_eq!(fs::read(dir.path().join(lock::FILE)).unwrap(), b"old");
    assert_eq!(dir.listing(), vec![lock::FILE.to_owned()]);
}

#[test]
fn resolve_lock_tool_hash_and_missing() {
    let l = Lock::parse(LOCK.as_bytes()).unwrap();
    let mut s = sdk_json::parse(SDK.as_bytes()).unwrap();
    l.check_tools(&s).unwrap();
    s.tools[0].version = "16.0.0-1.1".to_owned();
    assert_eq!(code(l.check_tools(&s)), "lock: tool");
    let p = &l.packages[0];
    p.check_hash(H).unwrap();
    assert_eq!(code(p.check_hash(&H.replace('1', "2"))), "lock: hash");
    let i = parse_index(INDEX).unwrap();
    p.find_in(&i).unwrap();
    l.tools[0].find_in(&i, "linux-x64").unwrap();
    let mut gone = i.clone();
    gone.packages.clear();
    assert_eq!(code(p.find_in(&gone)), "lock: missing");
    assert_eq!(code(l.tools[1].find_in(&i, "linux-arm64")), "lock: missing");
}

#[test]
fn resolve_versions_have_no_leading_zero() {
    assert_eq!(
        Version::parse("1.3.0").map(|v| v.to_string()).as_deref(),
        Some("1.3.0")
    );
    assert_eq!(
        Version::parse("0.0.0").map(|v| v.to_string()).as_deref(),
        Some("0.0.0")
    );
    for bad in [
        "1.03.0",
        "01.3.0",
        "1.3",
        "1.3.0.0",
        "1.3.65536",
        "+1.3.0",
        "1.3.0 ",
        "",
    ] {
        assert_eq!(Version::parse(bad), None, "{bad}");
    }
    assert_eq!(Abi::parse("1.03"), None);
    assert!(Abi::parse("65535.0").is_some());
    assert!(is_tool_version("15.2.1-1.1"));
    for bad in ["", "a b", "a\"b", "a\\b", &"x".repeat(65)] {
        assert!(!is_tool_version(bad), "{bad}");
    }
    assert!(is_host_name("linux-x64") && is_host_name("x64") && !is_host_name("9x"));
}

#[test]
fn resolve_archive_path_refuses_control_characters() {
    for bad in [
        "pkg\0.zip",
        "pkg\t.zip",
        "pkg\n.zip",
        "pkg\u{1f}.zip",
        "pkg\u{7f}.zip",
    ] {
        assert!(!index::is_archive_path(bad), "{bad:?}");
        assert!(!index::is_url(bad), "{bad:?}");
    }
    assert!(index::is_archive_path("my pkg/é~1.zip"));
    assert!(index::is_url("my pkg/é~1.zip"));
    // Through the reader: a NUL in a local url fails with `index: entry`.
    let text = INDEX.replacen(
        "https://example.invalid/robco/pipos-1.3.0.zip",
        "pkg\\u0000.zip",
        1,
    );
    let e = parse_index(&text).unwrap_err();
    assert_eq!(e.code(), "index: entry");
    assert!(
        e.to_string().starts_with("index: entry: packages[0].url"),
        "{e}"
    );
}

#[test]
fn resolve_https_url_needs_a_rest_and_no_control_character() {
    for bad in [
        "https://",
        "https://x/\0.zip",
        "https://x/\t.zip",
        "https://x/\u{7f}",
    ] {
        assert!(!index::is_url(bad), "{bad:?}");
    }
    assert!(index::is_url("https://example.invalid/p.zip"));
    let text = INDEX.replacen(
        "https://example.invalid/robco/pipos-1.3.0.zip",
        "https://",
        1,
    );
    assert_eq!(code(parse_index(&text)), "index: entry");
}

#[test]
fn resolve_sdk_json_file_path_keeps_the_archive_rules() {
    for bad in [
        "../MIT.txt",
        "/MIT.txt",
        "C:/MIT.txt",
        "LICENSES//MIT.txt",
        "LICENSES\\\\MIT.txt",
        "LICENSES/\\u0000.txt",
    ] {
        let text = SDK.replacen("\"LICENSES/MIT.txt\"", &format!("\"{bad}\""), 1);
        assert_ne!(text, SDK);
        let e = sdk_json::parse(text.as_bytes()).unwrap_err();
        assert_eq!(e.code(), "package: manifest", "{bad}");
        assert!(e.to_string().contains("breaks the path rules"), "{e}");
    }
}
